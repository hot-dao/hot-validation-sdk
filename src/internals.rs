use crate::{metrics, ChainId, Validation};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use futures_util::future::BoxFuture;
use futures_util::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;

pub const HOT_VERIFY_METHOD_NAME: &str = "hot_verify";
pub const MPC_HOT_WALLET_CONTRACT: &str = "mpc.hot.tg";
pub const MPC_GET_WALLET_METHOD: &str = "get_wallet";
pub const TIMEOUT: Duration = Duration::from_secs(10);

pub fn uid_to_wallet_id(uid: &str) -> anyhow::Result<String> {
    let uid_bytes = hex::decode(uid)?;
    let sha256_bytes = Sha256::new_with_prefix(uid_bytes).finalize();
    let uid_b58 = bs58::encode(sha256_bytes.as_slice()).into_string();
    Ok(uid_b58)
}

impl Validation {
    pub(crate) async fn verify_auth_method(
        self: Arc<Self>,
        wallet_id: String,
        auth_method: AuthMethod,
        message_body: String,
        message_hex: String,
        user_payload: String,
    ) -> anyhow::Result<()> {
        let _timer = metrics::performance::RPC_SINGLE_VERIFY_DURATION.start_timer();

        let status = match auth_method.chain_id {
            ChainId::Near => {
                let message_bs58 = hex::decode(&message_hex)
                    .map(|message_bytes| bs58::encode(message_bytes).into_string())?;

                let verify_args = VerifyArgs {
                    wallet_id: Some(wallet_id),
                    msg_hash: message_bs58,
                    metadata: auth_method.metadata.clone(),
                    user_payload,
                    msg_body: message_body,
                };
                self.near
                    .clone()
                    .verify(auth_method.account_id.as_str(), verify_args)
                    .await
                    .context("Validation on Near failed")?
            }
            ChainId::Stellar => {
                let verify_args = VerifyArgs {
                    wallet_id: Some(wallet_id),
                    msg_hash: message_hex,
                    metadata: auth_method.metadata.clone(),
                    user_payload,
                    msg_body: message_body,
                };
                self.stellar
                    .clone()
                    .verify(auth_method.account_id.as_str(), verify_args)
                    .await
                    .context("Validation on Stellar failed")?
            }
            ChainId::Evm(_) => {
                let verify_args = VerifyArgs {
                    wallet_id: Some(wallet_id),
                    msg_hash: message_hex,
                    metadata: auth_method.metadata.clone(),
                    user_payload,
                    msg_body: message_body,
                };
                let validation = self.evm.get(&auth_method.chain_id).ok_or(anyhow::anyhow!(
                    "EVM validation is not configured for chain {:?}",
                    auth_method.chain_id
                ))?;
                validation
                    .verify(auth_method.account_id.as_str(), verify_args)
                    .await
                    .context(format!(
                        "Validation on EVM chain ({:?}) failed",
                        auth_method.chain_id
                    ))?
            }
        };

        if status {
            Ok(())
        } else {
            Err(anyhow!(
                "Authentication method {:?} returned False",
                auth_method
            ))
        }
    }
}

/// Arguments for `get_wallet` method on Near `mpc.hot.tg` smart contract.
#[derive(Debug, Serialize)]
pub struct GetWalletArgs {
    pub(crate) wallet_id: String,
}

/// `account_id` is the smart contract address, and `chain_id` is the internal identifier for the chain.
/// Together, they indicate where to call `hot_verify`.
#[derive(Debug, Deserialize, PartialEq, Clone, Eq, Hash)]
pub struct AuthMethod {
    pub account_id: String,
    pub metadata: Option<String>,
    pub chain_id: ChainId,
}

/// The output of `get_wallet` on Near `mpc.hot.tg` smart contract.
#[derive(Debug, Deserialize, PartialEq, Clone, Eq, Hash)]
pub struct WalletAuthMethods {
    pub access_list: Vec<AuthMethod>,
    pub key_gen: usize,
    pub block_height: u64,
}

/// An input to the `hot_verify` method. A proof that a message is correct and can be signed.
#[derive(Debug, Serialize, Clone)]
pub struct VerifyArgs {
    pub msg_body: String,
    /// The hash of a refund message, supplied by user as a base85-encoded string.
    pub msg_hash: String,
    /// Used in Near only, otherwise no bytes.
    pub wallet_id: Option<String>,
    /// On EVM: Encoded nonce. On Near something else.
    pub user_payload: String,
    /// Used in Near only, otherwise no bytes.
    pub metadata: Option<String>,
}

/// An interface to a particular RPC server.
#[async_trait]
pub(crate) trait SingleVerifier: Send + Sync + 'static {
    /// An identification of the verifier (rpc endpoint). Used only for logging.
    fn get_endpoint(&self) -> String;

    /// Call `hot_verify` on the `auth_contract_id` with the specified args.
    async fn verify(&self, auth_contract_id: &str, args: VerifyArgs) -> anyhow::Result<bool>;
}

/// An interface, to call `hot_verify` concurrently on each `SingleVerifier`,
/// and checking whether there's at least `threshold` successes.
pub(crate) struct ThresholdVerifier<T: SingleVerifier> {
    pub(crate) threshold: usize,
    pub(crate) verifiers: Vec<Arc<T>>,
}

impl<T: SingleVerifier> ThresholdVerifier<T> {
    /// We can request data from a `SingleVerifier`. Each verifier casts a vote on the data it has returned.
    /// We collect all the votes and return a data with at least `threshold` votes.
    /// This logic was abstracted because we might call `verify`, `get_wallet_auth` or something else in the future.
    ///
    /// `functor` should return an `Option<R>`,
    /// with `None` being a vote for no data (when a server is unavailable), and `Some(R)` being a vote for `R`.
    pub(crate) async fn threshold_call<F, R>(&self, functor: F) -> anyhow::Result<R>
    where
        R: Eq + Hash + Clone,
        F: Clone + FnOnce(Arc<T>) -> BoxFuture<'static, Option<R>>,
    {
        let threshold = self.threshold;
        let total = self.verifiers.len();

        let mut counts: HashMap<R, usize> = HashMap::new();
        let mut responses = stream::iter(self.verifiers.iter().cloned())
            .map(|caller| functor.clone()(caller))
            .buffer_unordered(total);

        while let Some(opt_response) = responses.next().await {
            if let Some(response) = opt_response {
                let entry = counts.entry(response.clone()).or_insert(0);
                *entry += 1;

                // as soon as any variant reaches the threshold, return it
                if *entry >= threshold {
                    return Ok(response);
                }
            }
        }

        // if we exit the loop, nobody hit the threshold
        Err(anyhow!("No consensus for threshold call"))
    }

    pub async fn verify(&self, auth_contract_id: &str, args: VerifyArgs) -> anyhow::Result<bool> {
        let auth_contract_id = Arc::new(auth_contract_id.to_string());
        let functor = move |verifier: Arc<T>| -> BoxFuture<'static, Option<bool>> {
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
    use super::*;
    use async_trait::async_trait;
    use futures_util::future::BoxFuture;
    use tokio::time::{sleep, timeout, Duration};

    #[derive(Clone)]
    struct DummyVerifier {
        delay: Duration,
        resp: Option<u8>,
    }

    #[async_trait]
    impl SingleVerifier for DummyVerifier {
        fn get_endpoint(&self) -> String {
            "dummy".into()
        }

        async fn verify(&self, _auth_contract_id: &str, _args: VerifyArgs) -> anyhow::Result<bool> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn threshold_reaches_consensus() {
        let verifiers = vec![
            Arc::new(DummyVerifier {
                delay: Duration::from_millis(10),
                resp: Some(1),
            }),
            Arc::new(DummyVerifier {
                delay: Duration::from_millis(20),
                resp: Some(1),
            }),
            Arc::new(DummyVerifier {
                delay: Duration::from_millis(50),
                resp: Some(2),
            }),
        ];
        let tv = ThresholdVerifier {
            threshold: 2,
            verifiers,
        };

        let functor = |v: Arc<DummyVerifier>| -> BoxFuture<'static, Option<u8>> {
            Box::pin(async move {
                sleep(v.delay).await;
                v.resp
            })
        };

        let result = tv.threshold_call(functor).await.unwrap();
        assert_eq!(result, 1);
    }

    #[tokio::test]
    async fn threshold_no_consensus() {
        let verifiers = vec![
            Arc::new(DummyVerifier {
                delay: Duration::from_millis(10),
                resp: Some(1),
            }),
            Arc::new(DummyVerifier {
                delay: Duration::from_millis(20),
                resp: Some(2),
            }),
            Arc::new(DummyVerifier {
                delay: Duration::from_millis(30),
                resp: None,
            }),
        ];
        let tv = ThresholdVerifier {
            threshold: 2,
            verifiers,
        };

        let functor = |v: Arc<DummyVerifier>| -> BoxFuture<'static, Option<u8>> {
            Box::pin(async move {
                sleep(v.delay).await;
                v.resp
            })
        };

        let err = tv.threshold_call(functor).await.unwrap_err();
        assert_eq!(err.to_string(), "No consensus for threshold call");
    }

    #[tokio::test]
    async fn threshold_returns_early() {
        let verifiers = vec![
            Arc::new(DummyVerifier {
                delay: Duration::from_millis(20),
                resp: Some(1),
            }),
            Arc::new(DummyVerifier {
                delay: Duration::from_millis(150),
                resp: Some(1),
            }),
            Arc::new(DummyVerifier {
                delay: Duration::from_millis(200),
                resp: Some(2),
            }),
        ];
        let tv = ThresholdVerifier {
            threshold: 2,
            verifiers,
        };

        let functor = |v: Arc<DummyVerifier>| -> BoxFuture<'static, Option<u8>> {
            Box::pin(async move {
                sleep(v.delay).await;
                v.resp
            })
        };

        let result = timeout(Duration::from_millis(180), tv.threshold_call(functor))
            .await
            .expect("timed out")
            .unwrap();
        assert_eq!(result, 1);
    }

    #[derive(Clone)]
    struct BoolVerifier {
        delay: Duration,
        result: Result<bool, ()>,
    }

    #[async_trait]
    impl SingleVerifier for BoolVerifier {
        fn get_endpoint(&self) -> String {
            "bool".into()
        }

        async fn verify(&self, _auth_contract_id: &str, _args: VerifyArgs) -> anyhow::Result<bool> {
            sleep(self.delay).await;
            match self.result {
                Ok(b) => Ok(b),
                Err(_) => Err(anyhow!("boom")),
            }
        }
    }

    #[tokio::test]
    async fn verify_reaches_consensus_true() {
        let verifiers = vec![
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(10),
                result: Ok(true),
            }),
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(20),
                result: Ok(true),
            }),
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(50),
                result: Ok(false),
            }),
        ];
        let tv = ThresholdVerifier {
            threshold: 2,
            verifiers,
        };

        let res = tv
            .verify(
                "dummy",
                VerifyArgs {
                    msg_body: String::new(),
                    msg_hash: String::new(),
                    wallet_id: None,
                    user_payload: String::new(),
                    metadata: None,
                },
            )
            .await
            .unwrap();
        assert!(res);
    }

    #[tokio::test]
    async fn verify_reaches_consensus_false() {
        let verifiers = vec![
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(10),
                result: Ok(false),
            }),
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(20),
                result: Ok(false),
            }),
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(30),
                result: Ok(true),
            }),
        ];
        let tv = ThresholdVerifier {
            threshold: 2,
            verifiers,
        };

        let res = tv
            .verify(
                "dummy",
                VerifyArgs {
                    msg_body: String::new(),
                    msg_hash: String::new(),
                    wallet_id: None,
                    user_payload: String::new(),
                    metadata: None,
                },
            )
            .await
            .unwrap();
        assert!(!res);
    }

    #[tokio::test]
    async fn verify_no_consensus() {
        let verifiers = vec![
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(10),
                result: Ok(true),
            }),
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(20),
                result: Err(()),
            }),
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(30),
                result: Ok(false),
            }),
        ];
        let tv = ThresholdVerifier {
            threshold: 2,
            verifiers,
        };

        let err = tv
            .verify(
                "dummy",
                VerifyArgs {
                    msg_body: String::new(),
                    msg_hash: String::new(),
                    wallet_id: None,
                    user_payload: String::new(),
                    metadata: None,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "No consensus for threshold call");
    }

    #[tokio::test]
    async fn verify_returns_early() {
        let verifiers = vec![
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(20),
                result: Ok(true),
            }),
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(150),
                result: Ok(true),
            }),
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(200),
                result: Ok(false),
            }),
        ];
        let tv = ThresholdVerifier {
            threshold: 2,
            verifiers,
        };

        let result = timeout(
            Duration::from_millis(180),
            tv.verify(
                "dummy",
                VerifyArgs {
                    msg_body: String::new(),
                    msg_hash: String::new(),
                    wallet_id: None,
                    user_payload: String::new(),
                    metadata: None,
                },
            ),
        )
        .await
        .expect("timed out")
        .unwrap();
        assert!(result);
    }
}
