//! TON verification logic.
//!
//! TON smart contracts are called in sequence to validate a proof. The list of
//! calls comes from [`AuthMethod::metadata`] as a JSON array of objects with a
//! method name and arguments. Each intermediate call returns the address of the
//! next contract. The final call returns a boolean result.

use crate::internals::{SingleVerifier, ThresholdVerifier, VerifyArgs};
use anyhow::Result;
use anyhow::{bail, ensure, Context};
use async_trait::async_trait;
use serde::{de, Deserialize, Deserializer};
use std::sync::Arc;
use tonlib_client::client::{TonClient, TonClientBuilder, TonClientInterface};
use tonlib_client::tl::{SmcMethodId, TonFunction, TonResult, TvmNumber, TvmSlice, TvmStackEntry};
use tonlib_core::cell::{ArcCell, BagOfCells};
use tonlib_core::TonAddress;

/// Sequence of contract calls required for validation.
#[derive(Debug, Deserialize)]
struct TonValidationStructure(Vec<TonValidationStep>);

#[derive(Debug, Deserialize)]
/// Single contract method invocation.
struct TonValidationStep {
    /// Method to call on the contract.
    method: String,
    /// Arguments to pass to the method.
    args: Vec<StackArgType>,
}

impl TonValidationStep {
    pub fn convert_to_function(&self, id: i64) -> TonFunction {
        let method = SmcMethodId::Name {
            name: self.method.clone().into(),
        };

        let stack = self
            .args
            .iter()
            .map(|x| x.clone().into())
            .collect::<Vec<_>>();

        TonFunction::SmcRunGetMethod { id, method, stack }
    }
}

/// Argument of a TON contract call.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StackArgType {
    /// Numeric argument encoded as string.
    Num(String),
    /// Base58 encoded slice.
    #[serde(deserialize_with = "deserialize_base58")]
    Slice(Vec<u8>),
}

/// Helper for base58 encoded byte slices.
fn deserialize_base58<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    let string = String::deserialize(deserializer)?;
    bs58::decode(string).into_vec().map_err(de::Error::custom)
}

impl From<StackArgType> for TvmStackEntry {
    fn from(value: StackArgType) -> Self {
        match value {
            StackArgType::Num(string) => TvmStackEntry::Number {
                number: TvmNumber { number: string },
            },
            StackArgType::Slice(bytes) => TvmStackEntry::Slice {
                slice: TvmSlice { bytes },
            },
        }
    }
}

/// Wrapper around a [`TonClient`] that implements [`SingleVerifier`].
#[derive(Clone)]
pub(crate) struct TonSingleVerifier {
    client: TonClient,
}

impl ThresholdVerifier<TonSingleVerifier> {
    /// Build a [`ThresholdVerifier`] for TON.
    ///
    /// `pool_size` defines the number of connections in the underlying
    /// [`TonClient`].
    pub async fn new_ton(threshold: usize, pool_size: usize) -> Result<Self> {
        ensure!(threshold > 0, "Threshold must be greater than 0");
        ensure!(
            threshold <= pool_size,
            "Threshold must be less than pool size"
        );

        let ton = TonClientBuilder::new()
            .with_pool_size(pool_size)
            .build()
            .await?;
        ton.sync().await?;

        let verifiers = (0..pool_size)
            .map(|_| Arc::new(TonSingleVerifier::new(ton.clone())))
            .collect();

        Ok(Self {
            threshold,
            verifiers,
        })
    }
}

#[async_trait]
impl SingleVerifier for TonSingleVerifier {
    fn get_endpoint(&self) -> String {
        "TON ADNL".to_string()
    }

    /// Execute the validation sequence described in metadata.
    async fn verify(&self, auth_contract_id: &str, args: VerifyArgs) -> Result<bool> {
        let treasury_address = TonAddress::from_base64_url(auth_contract_id)?;

        let validation_structure = {
            let Some(metadata) = args.metadata else {
                bail!("Metadata is required for stellar validation");
            };
            serde_json::from_str::<TonValidationStructure>(&metadata)
                .context("Failed to parse Stellar Validation structure from metadata")?
        };

        let is_verify_success = match validation_structure.0.split_last() {
            Some((last, rest)) => {
                let mut current_address = treasury_address;
                for step in rest {
                    let address = self.do_intermediate_step(&current_address, step).await?;
                    current_address = address;
                }

                self.do_final_step(&current_address, last).await?
            }
            None => bail!("Validation structure is empty"),
        };

        Ok(is_verify_success)
    }
}

impl TonSingleVerifier {
    /// `TonClient` internally stores a pool of ADNL connections, and for each request it takes a random one.
    /// So we pass the same object.
    fn new(client: TonClient) -> Self {
        Self { client }
    }

    /// Extracts the single cell returned by a TON RPC call.
    fn get_root_from_ton_result(ton_result: &TonResult) -> Result<ArcCell> {
        let root = match ton_result {
            TonResult::SmcRunResult(result) => match &result.stack.elements[0] {
                TvmStackEntry::Slice { slice } => {
                    let bag_of_cells = BagOfCells::parse(&slice.bytes)?;
                    ensure!(
                        bag_of_cells.roots.len() == 1,
                        "only one root is expected in the ton result"
                    );
                    bag_of_cells.roots[0].clone()
                }
                _ => bail!("Expected slice from smc call"),
            },
            _ => bail!("Expected smc call run result"),
        };
        Ok(root)
    }

    /// Perform a single call on `address` according to `step`.
    async fn do_step(&self, address: &TonAddress, step: &TonValidationStep) -> Result<ArcCell> {
        let smc_state = self.client.smc_load(address).await?;
        let function = step.convert_to_function(smc_state.id);
        let ton_result = smc_state.conn.invoke(&function).await?;
        let root = Self::get_root_from_ton_result(&ton_result)?;
        Ok(root)
    }

    /// Execute a step that returns the next contract address.
    async fn do_intermediate_step(
        &self,
        address: &TonAddress,
        step: &TonValidationStep,
    ) -> Result<TonAddress> {
        let root = self.do_step(address, step).await?;
        let address = root.parser().load_address()?;
        Ok(address)
    }

    /// Execute the last step and return its boolean result.
    async fn do_final_step(&self, address: &TonAddress, step: &TonValidationStep) -> Result<bool> {
        let root = self.do_step(address, step).await?;
        let bit = root.parser().load_bit()?;
        Ok(bit)
    }
}

#[cfg(test)]
mod tests {
    use crate::ton::{StackArgType, TonValidationStep, TonValidationStructure};
    use anyhow::Result;
    use serde_json::json;

    #[test]
    fn test_validation_structure_serialization() -> Result<()> {
        let num_value = "1751330324000000000360";
        let slice_base58 = "61TAeGhv8GNwmz8Rqg6Ut3Zf3Zm4LDXCRQWJnd4pvdwf";
        let slice_bytes = bs58::decode(slice_base58).into_vec()?;

        let metadata = json!([
            {
                "method": "get_deposit_jetton_address",
                "args": [
                    {
                        "num": num_value
                    }
                ]
            },
            {
                "method": "verify_withdraw",
                "args": [
                    {
                        "slice": slice_base58
                    }
                ]
            }
        ]);

        let actual: TonValidationStructure = serde_json::from_value(metadata)?;

        let expected = TonValidationStructure(vec![
            TonValidationStep {
                method: "get_deposit_jetton_address".to_string(),
                args: vec![StackArgType::Num(num_value.to_string())],
            },
            TonValidationStep {
                method: "verify_withdraw".to_string(),
                args: vec![StackArgType::Slice(slice_bytes)],
            },
        ]);

        assert_eq!(format!("{:#?}", actual), format!("{:#?}", expected));

        Ok(())
    }

    #[tokio::test]
    async fn test_validation() -> Result<()> {
        // TODO
        Ok(())
    }
}
