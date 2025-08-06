use crate::internals::{SingleVerifier, ThresholdVerifier, HOT_VERIFY_METHOD_NAME, TIMEOUT};
use crate::{ChainValidationConfig, VerifyArgs};
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::future::BoxFuture;
use soroban_client::account::{Account, AccountBehavior};
use soroban_client::contract::{ContractBehavior, Contracts};
use soroban_client::keypair::{Keypair, KeypairBehavior};
use soroban_client::network::{NetworkPassphrase, Networks};
use soroban_client::transaction::ScVal;
use soroban_client::transaction_builder::{TransactionBuilder, TransactionBuilderBehavior};
use soroban_client::xdr::{ScBytes, ScString};
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

    fn build_contract_call(auth_contract_id: &str, args: VerifyArgs) -> Result<xdr::Operation> {
        let msg_hash = hex::decode(args.msg_hash).context("msg_hash is not valid hex")?;
        let user_payload =
            hex::decode(args.user_payload).context("user_payload is not valid hex")?;

        let contract = Contracts::new(auth_contract_id).map_err(|e| anyhow::anyhow!(e))?;

        let sc_args = vec![
            ScVal::String(ScString(msg_hash.try_into()?)),
            ScVal::Bytes(ScBytes(user_payload.try_into()?)),
        ];

        let operation = contract.call(HOT_VERIFY_METHOD_NAME, Some(sc_args));

        Ok(operation)
    }

    async fn verify(&self, auth_contract_id: &str, args: VerifyArgs) -> Result<bool> {
        let operation = Self::build_contract_call(auth_contract_id, args)?;

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

    pub async fn verify(&self, auth_contract_id: &str, args: VerifyArgs) -> Result<bool> {
        let auth_contract_id = Arc::new(auth_contract_id.to_string());
        let functor =
            move |verifier: Arc<StellarSingleVerifier>| -> BoxFuture<'static, Option<bool>> {
                let auth = auth_contract_id.clone();
                let args = args.clone();
                Box::pin(async move {
                    match verifier.verify(&auth, args).await {
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
    use crate::internals::VerifyArgs;
    use crate::stellar::StellarSingleVerifier;

    #[tokio::test]
    async fn single_verifier() {
        let args = VerifyArgs {
            msg_body: "".to_string(),
            wallet_id: None,
            msg_hash: "".into(),
            metadata: None,
            user_payload: "000000000000005ee4a2fbf444c19970b2289e4ab3eb2ae2e73063a5f5dfc450db7b07413f2d905db96414e0c33eb204".into(),
        };
        let auth_contract_id = "CCLWL5NYSV2WJQ3VBU44AMDHEVKEPA45N2QP2LL62O3JVKPGWWAQUVAG";
        let validation =
            StellarSingleVerifier::new("https://mainnet.sorobanrpc.com".to_string()).unwrap();

        validation.verify(auth_contract_id, args).await.unwrap();
    }
}
