use crate::verifiers::Verifier;
use crate::{metrics, Validation};
use anyhow::Result;
use anyhow::{anyhow, Context};
use futures_util::future::BoxFuture;
use futures_util::{stream, StreamExt};
use hot_validation_primitives::bridge::evm::EvmInputData;
use hot_validation_primitives::bridge::solana::SolanaInputData;
use hot_validation_primitives::bridge::stellar::StellarInputData;
use hot_validation_primitives::bridge::ton::TonInputData;
use hot_validation_primitives::bridge::HotVerifyResult;
use hot_validation_primitives::ChainId;
use rand::prelude::SliceRandom;
use rand::rngs::StdRng;
use rand::SeedableRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;

pub const HOT_VERIFY_METHOD_NAME: &str = "hot_verify";
pub const MPC_HOT_WALLET_CONTRACT: &str = "mpc.hot.tg";
pub const MPC_GET_WALLET_METHOD: &str = "get_wallet";
pub const TIMEOUT: Duration = Duration::from_millis(750);

pub fn uid_to_wallet_id(uid: &str) -> Result<String> {
    let uid_bytes = hex::decode(uid)?;
    let sha256_bytes = Sha256::new_with_prefix(uid_bytes).finalize();
    let uid_b58 = bs58::encode(sha256_bytes.as_slice()).into_string();
    Ok(uid_b58)
}

impl Validation {
    async fn handle_near(
        self: Arc<Self>,
        wallet_id: String,
        auth_method: &AuthMethod,
        message_hex: String,
        message_body: String,
        user_payload: String,
    ) -> Result<bool> {
        #[derive(Debug, Deserialize)]
        struct MethodName {
            method: String,
        }

        let message_bs58 = hex::decode(&message_hex)
            .map(|message_bytes| bs58::encode(message_bytes).into_string())?;

        // Mostly used with omni bridge workflows because there's another method name.
        let method_name = if let Some(metadata) = &auth_method.metadata {
            let method_name = serde_json::from_str::<MethodName>(metadata)?;
            method_name.method
        } else {
            HOT_VERIFY_METHOD_NAME.to_string()
        };

        let verify_args = VerifyArgs {
            wallet_id: Some(wallet_id.clone()),
            msg_hash: message_bs58,
            metadata: auth_method.metadata.clone(),
            user_payload: user_payload.clone(),
            msg_body: message_body.clone(),
        };

        let status = self
            .near
            .clone()
            .verify(auth_method.account_id.clone(), method_name, verify_args)
            .await
            .context("Could not get HotVerifyResult from NEAR")?;

        let status = match status {
            HotVerifyResult::AuthCall(auth_call) => match auth_call.chain_id {
                ChainId::Stellar => {
                    self.handle_stellar(
                        &auth_call.contract_id,
                        &auth_call.method,
                        auth_call.input.try_into()?,
                    )
                    .await?
                }
                ChainId::Ton | ChainId::TON_V2 => {
                    self.handle_ton(
                        &auth_call.contract_id,
                        &auth_call.method,
                        auth_call.input.try_into()?,
                    )
                    .await?
                }
                ChainId::Evm(_) => {
                    self.handle_evm(
                        auth_call.chain_id,
                        &auth_call.contract_id,
                        &auth_call.method,
                        auth_call.input.try_into()?,
                    )
                    .await?
                }
                ChainId::Solana => {
                    self.handle_solana(
                        &auth_call.contract_id,
                        &auth_call.method,
                        auth_call.input.try_into()?,
                    )
                    .await?
                }
                ChainId::Near => {
                    unimplemented!("Auth call should not lead to NEAR")
                }
            },
            HotVerifyResult::Result(status) => status,
        };
        Ok(status)
    }

    async fn handle_stellar(
        self: Arc<Self>,
        auth_contract_id: &str,
        method_name: &str,
        input: StellarInputData,
    ) -> Result<bool> {
        let status = self
            .stellar
            .clone()
            .verify(auth_contract_id, method_name, input)
            .await
            .context("Validation on Stellar failed")?;
        Ok(status)
    }

    async fn handle_solana(
        self: Arc<Self>,
        auth_contract_id: &str,
        method_name: &str,
        input: SolanaInputData,
    ) -> Result<bool> {
        let status = self
            .solana
            .clone()
            .verify(auth_contract_id, method_name, input)
            .await
            .context("Validation on Stellar failed")?;
        Ok(status)
    }

    async fn handle_evm(
        self: Arc<Self>,
        chain_id: ChainId,
        auth_contract_id: &str,
        method_name: &str,
        input: EvmInputData,
    ) -> Result<bool> {
        let validation = self.evm.get(&chain_id).ok_or(anyhow::anyhow!(
            "EVM validation is not configured for chain {:?}",
            chain_id
        ))?;
        let status = validation
            .verify(auth_contract_id, method_name, input)
            .await?;
        Ok(status)
    }

    async fn handle_ton(
        self: Arc<Self>,
        auth_contract_id: &str,
        method_name: &str,
        input: TonInputData,
    ) -> Result<bool> {
        let status = self
            .ton
            .clone()
            .verify(auth_contract_id, method_name, input)
            .await
            .context("Validation on Ton failed")?;
        Ok(status)
    }

    pub(crate) async fn verify_auth_method(
        self: Arc<Self>,
        wallet_id: String,
        auth_method: AuthMethod,
        message_body: String,
        message_hex: String,
        user_payload: String,
    ) -> Result<()> {
        let _timer = metrics::RPC_SINGLE_VERIFY_DURATION.start_timer();

        // TODO: DRY
        // TODO: Hypothesis: auth method is always a NEAR contract.
        let status = match auth_method.chain_id {
            ChainId::Near => {
                self.handle_near(
                    wallet_id,
                    &auth_method,
                    message_hex,
                    message_body,
                    user_payload,
                )
                .await?
            }
            ChainId::Stellar => {
                self.handle_stellar(
                    &auth_method.account_id,
                    HOT_VERIFY_METHOD_NAME,
                    StellarInputData::from_parts(message_hex, user_payload)?,
                )
                .await?
            }
            ChainId::Ton | ChainId::TON_V2 => {
                unimplemented!("It's not expected to call TON as the auth method")
            }
            ChainId::Evm(_) => {
                self.handle_evm(
                    auth_method.chain_id,
                    &auth_method.account_id,
                    HOT_VERIFY_METHOD_NAME,
                    EvmInputData::from_parts(message_hex, user_payload)?,
                )
                .await?
            }
            ChainId::Solana => {
                unimplemented!("It's not expected to call Solana as the auth method")
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
    /// Used to override what method is called on the `account_id`.
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
    /// In some cases, we need to know the exact message that we trying to sign.
    pub msg_body: String,
    /// The hash of the message that we try to sign.
    pub msg_hash: String,
    /// The wallet id, that initates the signing
    pub wallet_id: Option<String>,
    /// The actual data, that authorizes signing
    pub user_payload: String,
    /// Additional field for the future, in case we need to override something
    pub metadata: Option<String>,
}

/// An interface, to call `hot_verify` concurrently on each `SingleVerifier`,
/// and checking whether there's at least `threshold` successes.
pub(crate) struct ThresholdVerifier<T: Verifier> {
    pub(crate) threshold: usize,
    pub(crate) verifiers: Vec<Arc<T>>,
}

impl<T: Verifier> ThresholdVerifier<T> {
    /// We can request data from a `SingleVerifier`. Each verifier casts a vote on the data it has returned.
    /// We collect all the votes and return a data with at least `threshold` votes.
    /// This logic was abstracted because we might call `verify`, `get_wallet_auth` or something else in the future.
    ///
    /// `functor` should return an `Option<R>`,
    /// with `None` being a vote for no data (when a server is unavailable), and `Some(R)` being a vote for `R`.
    pub(crate) async fn threshold_call<F, R>(&self, functor: F) -> anyhow::Result<R>
    where
        R: Eq + Hash + Clone + Debug,
        F: Clone + FnOnce(Arc<T>) -> BoxFuture<'static, Result<R>>,
    {
        let threshold = self.threshold;
        let total = self.verifiers.len();

        let mut counts: HashMap<R, usize> = HashMap::new();
        let mut rng = StdRng::from_os_rng();

        let mut verifiers = self.verifiers.clone();
        verifiers.shuffle(&mut rng);
        let mut responses = stream::iter(self.verifiers.iter().cloned())
            .map(|caller| functor.clone()(caller))
            .buffer_unordered(total);

        let mut errors = vec![];
        while let Some(result_response) = responses.next().await {
            match result_response {
                Ok(response) => {
                    let entry = counts.entry(response.clone()).or_insert(0);
                    *entry += 1;

                    // as soon as any variant reaches the threshold, return it
                    if *entry >= threshold {
                        return Ok(response);
                    }
                }
                Err(err) => errors.push(err),
            }
        }

        if !errors.is_empty() {
            tracing::warn!("threshold call encountered errors: {:?}", errors);
        }

        // if we exit the loop, nobody hit the threshold
        Err(anyhow!(
            "No consensus for threshold call, got: {:?}, errors: {:?}",
            counts,
            errors
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use anyhow::Result;

    use futures_util::future::BoxFuture;
    use tokio::time::{sleep, timeout, Duration};

    #[derive(Clone)]
    struct DummyVerifier {
        delay: Duration,
        resp: Option<u8>,
    }

    impl Verifier for DummyVerifier {
        fn get_endpoint(&self) -> String {
            "dummy".into()
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

        let functor = |v: Arc<DummyVerifier>| -> BoxFuture<'static, Result<u8>> {
            Box::pin(async move {
                sleep(v.delay).await;
                v.resp.ok_or(anyhow!("No response"))
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

        let functor = |v: Arc<DummyVerifier>| -> BoxFuture<'static, Result<u8>> {
            Box::pin(async move {
                sleep(v.delay).await;
                v.resp.ok_or(anyhow!("No response"))
            })
        };

        let err = tv.threshold_call(functor).await.unwrap_err();
        assert!(err.to_string().contains("No consensus for threshold call"));
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

        let functor = |v: Arc<DummyVerifier>| -> BoxFuture<'static, Result<u8>> {
            Box::pin(async move {
                sleep(v.delay).await;
                v.resp.ok_or(anyhow!("No response"))
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

    impl BoolVerifier {
        async fn verify(&self, _auth_contract_id: &str, _args: VerifyArgs) -> anyhow::Result<bool> {
            sleep(self.delay).await;
            match self.result {
                Ok(b) => Ok(b),
                Err(()) => Err(anyhow!("boom")),
            }
        }
    }

    impl Verifier for BoolVerifier {
        fn get_endpoint(&self) -> String {
            "bool".into()
        }
    }

    impl ThresholdVerifier<BoolVerifier> {
        pub async fn verify(
            &self,
            auth_contract_id: &str,
            args: VerifyArgs,
        ) -> anyhow::Result<bool> {
            let auth_contract_id = Arc::new(auth_contract_id.to_string());
            let functor = move |verifier: Arc<BoolVerifier>| -> BoxFuture<'static, Result<bool>> {
                let auth = auth_contract_id.clone();
                let args = args.clone();
                Box::pin(async move { verifier.verify(&auth, args).await })
            };

            let result = self.threshold_call(functor).await?;
            Ok(result)
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
        assert!(err.to_string().contains("No consensus for threshold call"));
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
