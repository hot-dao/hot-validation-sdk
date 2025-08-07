use crate::internals::{SingleVerifier, ThresholdVerifier, TIMEOUT};
use crate::ChainValidationConfig;
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::future::BoxFuture;
use hot_validation_primitives::bridge::stellar::StellarInputData;
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
pub(crate) struct StellarSingleVerifier {
    client: Arc<Server>,
    server: String,
}

impl StellarSingleVerifier {
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

        let transaction_builder = TransactionBuilder::new(source_account, Networks::public(), None)
            .fee(100u32) // An exact value doesn't matter, it's just a placeholder.
            .set_timeout(TIMEOUT.as_secs() as i64)
            .map_err(|e| anyhow::anyhow!(e))?
            .clone();

        Ok(transaction_builder)
    }

    fn build_contract_call(
        auth_contract_id: &str,
        method_name: String,
        input: StellarInputData,
    ) -> Result<xdr::Operation> {
        let contract = Contracts::new(auth_contract_id).map_err(|e| anyhow::anyhow!(e))?;
        let sc_args: Vec<ScVal> = TryFrom::try_from(input).context("Failed to convert input")?;
        let operation = contract.call(&method_name, Some(sc_args));
        Ok(operation)
    }

    async fn verify(
        &self,
        auth_contract_id: &str,
        method_name: String,
        input: StellarInputData,
    ) -> Result<bool> {
        let operation = Self::build_contract_call(auth_contract_id, method_name, input)?;

        let tx = Self::create_transaction_builder()?
            .add_operation(operation)
            .build();

        let simulation = self.client.simulate_transaction(tx, None).await?;

        // if there was an RPC‐side error, show it:
        if let Some(err) = simulation.error {
            anyhow::bail!("simulation failed: {:?}", err);
        }
        // extract the return‐value:
        if let Some((ScVal::Bool(b), _auths)) = simulation.to_result() {
            Ok(b)
        } else {
            anyhow::bail!("unexpected simulation result: {:?}", simulation);
        }
    }
}

#[async_trait]
impl SingleVerifier for StellarSingleVerifier {
    fn get_endpoint(&self) -> String {
        self.server.clone()
    }
}

impl ThresholdVerifier<StellarSingleVerifier> {
    pub fn new_stellar(config: ChainValidationConfig) -> Result<Self> {
        let threshold = config.threshold;
        let servers = config.servers;
        if threshold > servers.len() {
            panic!(
                "There should be at least {} servers, got {}",
                threshold,
                servers.len()
            )
        }
        let verifiers = servers
            .iter()
            .map(|s| StellarSingleVerifier::new(s.clone()).map(Arc::new))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            threshold,
            verifiers,
        })
    }

    pub async fn verify(
        &self,
        auth_contract_id: &str,
        method_name: &str,
        input: StellarInputData,
    ) -> Result<bool> {
        let auth_contract_id = Arc::new(auth_contract_id.to_string());
        let functor =
            move |verifier: Arc<StellarSingleVerifier>| -> BoxFuture<'static, Option<bool>> {
                let auth = auth_contract_id.clone();
                let method_name = method_name.to_string();
                Box::pin(async move {
                    match verifier.verify(&auth, method_name, input).await {
                        Ok(true) => Some(true),
                        Ok(false) => {
                            tracing::warn!("Verification failed for {}", verifier.get_endpoint());
                            Some(false)
                        }
                        Err(e) => {
                            tracing::warn!("{}", e);
                            None
                        }
                    }
                })
            };

        let result = self.threshold_call(functor).await?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use crate::internals::HOT_VERIFY_METHOD_NAME;
    use crate::stellar::{StellarInputData, StellarSingleVerifier};
    use anyhow::Result;
    use hot_validation_primitives::bridge::HotVerifyAuthCall;

    #[tokio::test]
    async fn single_verifier() -> Result<()> {
        let msg_hash = "".to_string();
        let user_payload = "000000000000005ee4a2fbf444c19970b2289e4ab3eb2ae2e73063a5f5dfc450db7b07413f2d905db96414e0c33eb204".to_string();
        let auth_contract_id = "CCLWL5NYSV2WJQ3VBU44AMDHEVKEPA45N2QP2LL62O3JVKPGWWAQUVAG";
        let validation = StellarSingleVerifier::new("https://mainnet.sorobanrpc.com".to_string())?;

        validation
            .verify(
                auth_contract_id,
                HOT_VERIFY_METHOD_NAME.to_string(),
                StellarInputData::from_parts(msg_hash, user_payload)?,
            )
            .await?;

        Ok(())
    }

    #[tokio::test]
    async fn single_verifier_bridge() -> Result<()> {
        let msg_hash = "".to_string();
        let user_payload = "000000000000005f1d038ae3e890ca50c9a9f00772fcf664b4a8fefb93170d1a6f0e9843a2a816797bab71b6a99ca881".to_string();
        let auth_contract_id = "CCLWL5NYSV2WJQ3VBU44AMDHEVKEPA45N2QP2LL62O3JVKPGWWAQUVAG";
        let validation = StellarSingleVerifier::new("https://mainnet.sorobanrpc.com".to_string())?;

        validation
            .verify(
                auth_contract_id,
                HOT_VERIFY_METHOD_NAME.to_string(),
                StellarInputData::from_parts(msg_hash, user_payload)?,
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
