use crate::http_client::TIMEOUT;
use crate::threshold_verifier::{Identifiable, ThresholdVerifier};
use crate::verifiers::Verifier;
use crate::ChainValidationConfig;
use anyhow::{Context, Result};
use async_trait::async_trait;
use hot_validation_primitives::bridge::stellar::StellarInputData;
use hot_validation_primitives::bridge::InputData;
use hot_validation_primitives::ExtendedChainId;
use soroban_client::account::{Account, AccountBehavior};
use soroban_client::contract::{ContractBehavior, Contracts};
use soroban_client::keypair::{Keypair, KeypairBehavior};
use soroban_client::network::{NetworkPassphrase, Networks};
use soroban_client::transaction::ScVal;
use soroban_client::transaction_builder::{TransactionBuilder, TransactionBuilderBehavior};
use soroban_client::{xdr, Options, Server};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct StellarVerifier {
    client: Arc<Server>,
    server: String,
}

impl Identifiable for StellarVerifier {
    fn id(&self) -> String {
        self.server.clone()
    }
}

impl StellarVerifier {
    pub fn new(server: String) -> Result<Self> {
        let client = Arc::new(Server::new(&server, Options::default())?);
        Ok(Self { client, server })
    }

    fn create_transaction_builder() -> Result<TransactionBuilder> {
        // We could have saved it as a struct field, but the transaction builder
        //   is not `sync` because of Rc<RefCell<T>>.
        // It's easier to build it every time.

        // Some boilerplate code specific to Stellar RPC.
        let source_account = {
            let kp = Keypair::random().map_err(|e| anyhow::anyhow!(e.to_string()))?;
            Rc::new(RefCell::new(
                // Exact values do not matter, we just have to fill in placeholders.
                Account::new(&kp.public_key(), "1").map_err(|e| anyhow::anyhow!(e.to_string()))?,
            ))
        };

        let timeout_secs: i64 = TIMEOUT
            .as_secs()
            .try_into()
            .map_err(|_| anyhow::anyhow!("timeout exceeds i64::MAX seconds"))?;

        let transaction_builder = TransactionBuilder::new(source_account, Networks::public(), None)
            .fee(100u32) // An exact value doesn't matter, it's just a placeholder.
            .set_timeout(timeout_secs)
            .map_err(|e| anyhow::anyhow!(e))?
            .clone();

        Ok(transaction_builder)
    }

    fn build_contract_call(
        auth_contract_id: &str,
        method_name: &str,
        input: StellarInputData,
    ) -> Result<xdr::Operation> {
        let contract = Contracts::new(auth_contract_id).map_err(|e| anyhow::anyhow!(e))?;
        let sc_args: Vec<ScVal> = TryFrom::try_from(input).context("Failed to convert input")?;
        let operation = contract.call(method_name, Some(sc_args));
        Ok(operation)
    }
}

#[async_trait]
impl Verifier for StellarVerifier {
    fn chain_id(&self) -> ExtendedChainId {
        ExtendedChainId::Stellar
    }

    async fn verify(
        &self,
        auth_contract_id: String,
        method_name: String,
        input_data: InputData,
    ) -> Result<bool> {
        let input: StellarInputData = input_data.try_into()?;

        let operation = Self::build_contract_call(&auth_contract_id, &method_name, input)?;

        let tx = Self::create_transaction_builder()?
            .add_operation(operation)
            .build();

        let simulation = self.client.simulate_transaction(&tx, None).await?;

        // if there was an RPC‐side error, show it:
        if let Some(err) = simulation.error {
            anyhow::bail!("simulation failed: {err:?}");
        }
        // extract the return‐value:
        if let Some((ScVal::Bool(b), _auths)) = simulation.to_result() {
            Ok(b)
        } else {
            anyhow::bail!("unexpected simulation result: {simulation:?}");
        }
    }
}

impl ThresholdVerifier<StellarVerifier> {
    pub fn new_stellar(config: ChainValidationConfig) -> Result<Self> {
        let threshold = config.threshold;
        let servers = config.servers;
        let verifiers = servers
            .iter()
            .map(|s| StellarVerifier::new(s.clone()).map(Arc::new))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            threshold,
            verifiers,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::verifiers::stellar::StellarVerifier;
    use crate::verifiers::Verifier;
    use crate::HOT_VERIFY_METHOD_NAME;
    use anyhow::Result;
    use hot_validation_primitives::bridge::stellar::{StellarInputArg, StellarInputData};
    use hot_validation_primitives::bridge::HotVerifyAuthCall;

    #[tokio::test]
    async fn single_verifier() -> Result<()> {
        let msg_hash = String::new();
        let user_payload = "000000000000005ee4a2fbf444c19970b2289e4ab3eb2ae2e73063a5f5dfc450db7b07413f2d905db96414e0c33eb204".to_string();
        let auth_contract_id =
            "CCLWL5NYSV2WJQ3VBU44AMDHEVKEPA45N2QP2LL62O3JVKPGWWAQUVAG".to_string();
        let validation = StellarVerifier::new("https://mainnet.sorobanrpc.com".to_string())?;

        validation
            .verify(
                auth_contract_id,
                HOT_VERIFY_METHOD_NAME.to_string(),
                StellarInputData::from_parts(msg_hash, user_payload)?.into(),
            )
            .await?;

        Ok(())
    }

    #[tokio::test]
    async fn single_verifier_bridge() -> Result<()> {
        let msg_hash = String::new();
        let user_payload = "000000000000005f1d038ae3e890ca50c9a9f00772fcf664b4a8fefb93170d1a6f0e9843a2a816797bab71b6a99ca881".to_string();
        let auth_contract_id = "CCLWL5NYSV2WJQ3VBU44AMDHEVKEPA45N2QP2LL62O3JVKPGWWAQUVAG";
        let validation = StellarVerifier::new("https://mainnet.sorobanrpc.com".to_string())?;

        validation
            .verify(
                auth_contract_id.to_string(),
                HOT_VERIFY_METHOD_NAME.to_string(),
                StellarInputData::from_parts(msg_hash, user_payload)?.into(),
            )
            .await?;

        Ok(())
    }

    #[tokio::test]
    async fn stellar_locker_nonce_executed() -> Result<()> {
        let nonce = 1_754_631_474_000_000_070_075_u128;
        let auth_contract_id = "CCLWL5NYSV2WJQ3VBU44AMDHEVKEPA45N2QP2LL62O3JVKPGWWAQUVAG";
        let validation = StellarVerifier::new("https://mainnet.sorobanrpc.com".to_string())?;

        validation
            .verify(
                auth_contract_id.to_string(),
                "is_executed".to_string(),
                StellarInputData(vec![StellarInputArg::U128(nonce)]).into(),
            )
            .await?;

        Ok(())
    }

    #[test]
    fn check_stellar_bridge_validation_format() {
        let x = r#"
            {
                  "chain_id": 1100,
                  "contract_id": "CCLWL5NYSV2WJQ3VBU44AMDHEVKEPA45N2QP2LL62O3JVKPGWWAQUVAG",
                  "input": [
                    {
                      "type": "string",
                      "value": ""
                    },
                    {
                      "type": "bytes",
                      "value": "0x000000000000005f1d038ae3e890ca50c9a9f00772fcf664b4a8fefb93170d1a6f0e9843a2a816797bab71b6a99ca881"
                    }
                  ],
                  "method": "hot_verify"
                }
        "#.to_string();
        serde_json::from_str::<HotVerifyAuthCall>(&x).unwrap();
    }
}
