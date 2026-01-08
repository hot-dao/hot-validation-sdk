#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
mod verifiers;

mod http_client;
mod metrics;
mod threshold_verifier;

pub use hot_validation_primitives::*;

use crate::threshold_verifier::ThresholdVerifier;
use crate::verifiers::cosmos::CosmosVerifier;
use crate::verifiers::evm::EvmVerifier;
use crate::verifiers::near::NearVerifier;
use crate::verifiers::solana::SolanaVerifier;
use crate::verifiers::stellar::StellarVerifier;
use crate::verifiers::ton::TonVerifier;
use anyhow::{bail, ensure, Context, Result};
use futures_util::future::try_join_all;
use hot_validation_primitives::bridge::HotVerifyResult;
use hot_validation_primitives::uid::WalletId;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::instrument;

pub const HOT_VERIFY_METHOD_NAME: &str = "hot_verify";
pub const MPC_HOT_WALLET_CONTRACT: &str = "mpc.hot.tg";
pub const MPC_GET_WALLET_METHOD: &str = "get_wallet";

/// `account_id` is the smart contract address, and `chain_id` is the internal identifier for the chain.
/// Together, they indicate where to call `hot_verify`.
#[derive(Debug, Deserialize, PartialEq, Clone, Eq, Hash)]
pub struct AuthMethod {
    pub account_id: String,
    /// Used to override what method is called on the `account_id`.
    pub metadata: Option<String>,
}

/// The output of `get_wallet` on Near `mpc.hot.tg` smart contract.
#[derive(Debug, Deserialize, PartialEq, Clone, Eq, Hash)]
pub struct WalletAuthMethods {
    pub access_list: Vec<AuthMethod>,
}

/// The logic that prevents signing arbitrary messages.
#[derive(Clone)]
pub struct Validation {
    pub near: Arc<ThresholdVerifier<NearVerifier>>,
    pub cosmos: HashMap<ChainId, Arc<ThresholdVerifier<CosmosVerifier>>>,
    pub evm: HashMap<ChainId, Arc<ThresholdVerifier<EvmVerifier>>>,
    pub stellar: Arc<ThresholdVerifier<StellarVerifier>>,
    pub ton: Arc<ThresholdVerifier<TonVerifier>>,
    pub solana: Arc<ThresholdVerifier<SolanaVerifier>>,
}

impl Validation {
    pub fn new(configs: &HashMap<ChainId, ChainValidationConfig>) -> Result<Self> {
        let client: Arc<reqwest::Client> = Arc::new(reqwest::Client::new());
        for (chain_id, config) in configs {
            metrics::set_threshold_delta(*chain_id, config.servers.len(), config.threshold);
        }

        let near = {
            let config = configs
                .get(&ChainId::Near)
                .expect("No near config (chain_id = 0) found")
                .clone();
            let verifier = ThresholdVerifier::new_near(config, &client);
            Arc::new(verifier)
        };

        let stellar = {
            let config = configs
                .get(&ChainId::Stellar)
                .expect("No stellar config (chain_id = 1100) found")
                .clone();
            let verifier = ThresholdVerifier::new_stellar(config)?;
            Arc::new(verifier)
        };

        let cosmos = configs
            .clone()
            .into_iter()
            // Some chains that are not EVM (Ton, Cosmos) are being included too, but we dont really care.
            .filter(|(id, _)| id.is_cosmos())
            .map(|(id, config)| {
                let threshold_verifier = {
                    let verifier = ThresholdVerifier::new_cosmos(config, &client.clone(), id);
                    Arc::new(verifier)
                };
                (id, threshold_verifier)
            })
            .collect();

        let evm = configs
            .clone()
            .into_iter()
            // Some chains that are not EVM (Ton, Cosmos) are being included too, but we dont really care.
            .filter(|(id, _)| matches!(id, ChainId::Evm(_)))
            .map(|(id, config)| {
                let threshold_verifier = {
                    let verifier = ThresholdVerifier::new_evm(config, &client.clone(), id);
                    Arc::new(verifier)
                };
                (id, threshold_verifier)
            })
            .collect();

        let ton = {
            let config = configs
                .get(&ChainId::TON_V2)
                .expect("No ton config (chain_id = 1117) found")
                .clone();
            let verifier = ThresholdVerifier::new_ton(config, &client);
            Arc::new(verifier)
        };

        let solana = {
            let config = configs
                .get(&ChainId::Solana)
                .expect("No solana config found");
            let verifier = ThresholdVerifier::new_solana(config);
            Arc::new(verifier)
        };

        let validation = Self {
            near,
            cosmos,
            evm,
            stellar,
            ton,
            solana,
        };
        Ok(validation)
    }

    #[instrument(
        skip(self, message),
        fields(message_hex = %hex::encode(&message)),
        err(Debug)
    )]
    pub async fn verify(
        self: &Arc<Self>,
        wallet_id: WalletId,
        message: Vec<u8>,
        proof: ProofModel,
    ) -> Result<()> {
        let _timer = metrics::RPC_VERIFY_TOTAL_DURATION.start_timer();

        let wallet = self
            .near
            .get_wallet_auth_methods(wallet_id.clone())
            .await
            .context(format!("Couldn't get auth methods for wallet {wallet_id}"))?;

        ensure!(
            proof.user_payloads.len() == wallet.access_list.len(),
            "Length of provided user payloads ({}) doesn't match with required wallet authorization ({})",
            proof.user_payloads.len(),
            wallet.access_list.len()
        );

        try_join_all(
            wallet
                .access_list
                .into_iter()
                .zip(proof.user_payloads.into_iter())
                .map(|(auth_method, user_payload)| {
                    self.verify_auth_method(
                        wallet_id.clone(),
                        auth_method,
                        proof.message_body.clone(),
                        message.clone(),
                        user_payload,
                    )
                }),
        )
        .await?;

        Ok(())
    }

    #[instrument(
        skip(self, message),
        fields(message_hex = %hex::encode(&message)),
        err(Debug)
    )]
    pub(crate) async fn verify_auth_method(
        self: &Arc<Self>,
        wallet_id: WalletId,
        auth_method: AuthMethod,
        message_body: String,
        message: Vec<u8>,
        user_payload: String,
    ) -> Result<()> {
        let _timer = metrics::RPC_SINGLE_VERIFY_DURATION.start_timer();

        metrics::tick_metrics_verify_total_attempts(ChainId::Near);
        let status = self
            .near
            .verify(
                wallet_id.clone(),
                auth_method.clone(),
                message,
                message_body,
                user_payload,
            )
            .await
            .context("Could not get HotVerifyResult from NEAR")?;
        metrics::tick_metrics_verify_success_attempts(ChainId::Near);

        let status = match status {
            HotVerifyResult::AuthCall(auth_call) => {
                metrics::tick_metrics_verify_total_attempts(auth_call.chain_id);
                let status = match auth_call.chain_id {
                    ChainId::Stellar => {
                        let verifier = &self.stellar;
                        verifier
                            .verify(auth_call.contract_id, auth_call.method, auth_call.input)
                            .await?
                    }

                    chain_id if chain_id.is_cosmos() => {
                        let verifier =
                            self.cosmos.get(&auth_call.chain_id).ok_or(anyhow::anyhow!(
                                "Cosmos validation is not configured for chain {:?}",
                                auth_call.chain_id
                            ))?;
                        verifier
                            .verify(auth_call.contract_id, auth_call.method, auth_call.input)
                            .await?
                    }

                    ChainId::Ton | ChainId::TON_V2 => {
                        let verifier = &self.ton;
                        verifier
                            .verify(auth_call.contract_id, auth_call.method, auth_call.input)
                            .await?
                    }

                    ChainId::Solana => {
                        let verifier = &self.solana;
                        verifier
                            .verify(auth_call.contract_id, auth_call.method, auth_call.input)
                            .await?
                    }

                    ChainId::Evm(_) => {
                        let verifier = self.evm.get(&auth_call.chain_id).ok_or(anyhow::anyhow!(
                            "EVM validation is not configured for chain {:?}",
                            auth_call.chain_id
                        ))?;
                        verifier
                            .verify(auth_call.contract_id, auth_call.method, auth_call.input)
                            .await?
                    }

                    ChainId::Near => {
                        bail!("Auth call should not lead to NEAR")
                    }
                };
                metrics::tick_metrics_verify_success_attempts(auth_call.chain_id);
                status
            }
            HotVerifyResult::Result(status) => status,
        };

        ensure!(
            status,
            "Auth method {auth_method:?} failed for wallet_id {wallet_id}"
        );

        Ok(())
    }
}

#[cfg(any(test, feature = "test-data"))]
pub mod test_data {
    use crate::Validation;
    use hot_validation_primitives::{ChainId, ChainValidationConfig};
    use std::collections::HashMap;
    use std::sync::Arc;

    #[must_use]
    pub fn ton_rpc() -> String {
        dotenv::var("TON_RPC")
            .unwrap_or_else(|_| "https://toncenter.com/api/v2/jsonRPC".to_string())
    }

    #[must_use]
    pub fn near_rpc() -> String {
        dotenv::var("NEAR_RPC").unwrap_or_else(|_| "https://rpc.mainnet.near.org".to_string())
    }

    #[must_use]
    pub fn bnb_rpc() -> String {
        dotenv::var("BNB_RPC").unwrap_or_else(|_| "https://bsc.therpc.io".to_string())
    }

    #[must_use]
    pub fn base_rpc() -> String {
        dotenv::var("BASE_RPC").unwrap_or_else(|_| "https://base.llamarpc.com".to_string())
    }

    #[must_use]
    pub fn create_validation_object() -> Arc<Validation> {
        let configs = HashMap::from([
            (
                ChainId::Near,
                ChainValidationConfig {
                    threshold: 2,
                    servers: vec![
                        "https://rpc.near.org".to_string(),
                        "http://ffooooo-bbbaaaar:3030/".to_string(),
                        "https://nearrpc.aurora.dev".to_string(),
                        "https://1rpc.io/near".to_string(),
                        "https://allthatnode.com/protocol/near.dsrv".to_string(),
                        near_rpc(),
                    ],
                },
            ),
            (
                ChainId::Stellar,
                ChainValidationConfig {
                    threshold: 1,
                    servers: vec!["https://mainnet.sorobanrpc.com".to_string()],
                },
            ),
            (
                ChainId::Evm(1),
                ChainValidationConfig {
                    threshold: 1,
                    servers: vec![
                        "https://eth.drpc.org".to_string(),
                        "http://bad-rpc:8545".to_string(),
                    ],
                },
            ),
            (
                ChainId::Evm(8453),
                ChainValidationConfig {
                    threshold: 1,
                    servers: vec![
                        "https://1rpc.io/base".to_string(),
                        "http://bad-rpc:8545".to_string(),
                        base_rpc(),
                    ],
                },
            ),
            (
                ChainId::Evm(56),
                ChainValidationConfig {
                    threshold: 1,
                    servers: vec!["https://bsc.blockrazor.xyz".to_string(), bnb_rpc()],
                },
            ),
            (
                ChainId::TON_V2,
                ChainValidationConfig {
                    threshold: 1,
                    servers: vec![
                        "https://toncenter.com/api/v2/jsonRPC".to_string(),
                        ton_rpc(),
                    ],
                },
            ),
            (
                ChainId::Solana,
                ChainValidationConfig {
                    threshold: 1,
                    servers: vec!["https://api.mainnet-beta.solana.com".to_string()],
                },
            ),
            (
                ChainId::Evm(4444_118),
                ChainValidationConfig {
                    threshold: 1,
                    servers: vec!["https://juno-api.stakeandrelax.net".to_string()],
                },
            ),
        ]);

        let validation = Validation::new(&configs).unwrap();
        Arc::new(validation)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::should_panic_without_expect)]

    // TODO: use anyhow::Result;
    use super::*; // TODO: remove
    use crate::test_data::{create_validation_object, near_rpc};
    use base64::prelude::BASE64_STANDARD;
    use base64::Engine;
    use hot_validation_primitives::bridge::{
        CompletedWithdrawal, CompletedWithdrawalAction, DepositAction, DepositData, HotVerifyBridge,
    };
    use std::str::FromStr;

    #[tokio::test]
    async fn validate_on_near() {
        let validation = create_validation_object();

        let wallet_id = WalletId::from_str("A8NpkSkn1HZPYjxJRCpD4iPhDHzP81bbduZTqPpHmEgn").unwrap();
        let message =
            hex::decode("57f42da8350f6a7c6ad567d678355a3bbd17a681117e7a892db30656d5caee32")
                .unwrap();
        let proof = ProofModel {
            message_body: "S8safEk4JWgnJsVKxans4TqBL796cEuV5GcrqnFHPdNW91AupymrQ6zgwEXoeRb6P3nyaSskoFtMJzaskXTDAnQUTKs5dGMWQHsz7irQJJ2UA2aDHSQ4qxgsU3h1U83nkq4rBstK8PL1xm6WygSYihvBTmuaMjuKCK6JT1tB4Uw71kGV262kU914YDwJa53BiNLuVi3s2rj5tboEwsSEpyJo9x5diq4Ckmzf51ZjZEDYCH8TdrP1dcY4FqkTCBA7JhjfCTToJR5r74ApfnNJLnDhTxkvJb4ReR9T9Ga7hPNazCFGE8Xq1deu44kcPjXNvb1GJGWLAZ5k1wxq9nnARb3bvkqBTmeYiDcPDamauhrwYWZkMNUsHtoMwF6286gcmY3ZgE3jja1NGuYKYQHnvscUqcutuT9qH".to_string(),
            user_payloads: vec![r#"{"auth_method":0,"signatures":["HZUhhJamfp8GJLL8gEa2F2qZ6TXPu4PYzzWkDqsTQsMcW9rQsG2Hof4eD2Vex6he2fVVy3UNhgi631CY8E9StAH"]}"#.to_string()],
        };

        validation.verify(wallet_id, message, proof).await.unwrap();
    }

    #[should_panic]
    #[tokio::test]
    async fn two_auth_methods_fail_with_bad_rpc() {
        let configs = HashMap::from([
            (
                ChainId::Near,
                ChainValidationConfig {
                    threshold: 2,
                    servers: vec![
                        "https://rpc.near.org".to_string(),
                        "http://ffooooo-bbbaaaar:3030/".to_string(),
                        "https://nearrpc.aurora.dev".to_string(),
                        "https://1rpc.io/near".to_string(),
                        "https://allthatnode.com/protocol/near.dsrv".to_string(),
                        near_rpc(),
                    ],
                },
            ),
            (
                ChainId::Evm(8453),
                ChainValidationConfig {
                    threshold: 1,
                    servers: vec!["http://bad-rpc:8545".to_string()],
                },
            ),
        ]);
        let validation = Arc::new(Validation::new(&configs).unwrap());

        let wallet_id = WalletId::from_str("GjEEr1744i8BCjSpXTfcdd8GCvRiz1QHpQ7egP3QLESQ").unwrap();
        let message =
            hex::decode("6484f06d86d1aee5ee53411f6033181eb0c5cde57081a798f4f6bfbe01a443e4")
                .unwrap();
        let proof = ProofModel {
            message_body: String::new(),
            user_payloads: vec![
                "{\"signatures\": [\"2r4RNC49RGA6Wqo5VzZtATBs3jMvqZCo5NYfJGkDpHZd598Zvt7kFfiuH8yr26CynzSMsgoHYoMUF5h31dSVHAT1\"], \"auth_method\": 0}".to_string(),
                "00000000000000000000000000000000000000000000005e9def3f04597b183c0000000000000000000000000000000000000000000000000000000000000000".to_string()
            ],
        };

        validation.verify(wallet_id, message, proof).await.unwrap();
    }

    /// wallet id for testing. It has only one auth method, which is `bridge.kuksag.tg` with `hot_verify_locker_state` method.
    fn staging_wallet_id() -> WalletId {
        WalletId::from_str("EvXjdccDCzZfofBsk6NL8LKKNSa6RcBmrXqjymM9mmnn").unwrap()
    }

    #[tokio::test]
    async fn bridge_deposit_validation_evm() -> Result<()> {
        let validation = create_validation_object();

        let wallet_id = staging_wallet_id();
        let message =
            hex::decode("c4ea3c95f2171df3fa5a6f8452d1bbbbd0608abe68fdcea7f25a04516c50cba6")?;
        let payload = HotVerifyBridge::Deposit(DepositAction {
            chain_id: ChainId::Evm(56),
            data: DepositData {
                sender: Some(vec![0; 32]),
                receiver: Some(vec![0; 32]),
                token_id: Some(vec![]),
                amount: Some(0),
                nonce: 1_754_431_900_000_000_013_182,
            },
        });
        let json = serde_json::to_value(&payload)?;
        dbg!(&json);
        let proof = ProofModel {
            message_body: String::new(),
            user_payloads: vec![json.to_string()],
        };

        validation.verify(wallet_id, message, proof).await?;
        Ok(())
    }

    #[tokio::test]
    async fn bridge_deposit_validation_stellar() -> Result<()> {
        let validation = create_validation_object();

        let wallet_id = staging_wallet_id();
        let message =
            hex::decode("c9a9f00772fcf664b4a8fefb93170d1a6f0e9843a2a816797bab71b6a99ca881")?;
        let payload = HotVerifyBridge::Deposit(DepositAction {
            chain_id: ChainId::Stellar,
            data: DepositData {
                sender: Some(vec![0; 32]),
                receiver: Some(vec![0; 32]),
                token_id: Some(vec![]),
                amount: Some(0),
                nonce: 1_754_531_354_365_901_458_000,
            },
        });
        let json = serde_json::to_value(&payload)?;
        dbg!(&json);
        let proof = ProofModel {
            message_body: String::new(),
            user_payloads: vec![json.to_string()],
        };

        validation.verify(wallet_id, message, proof).await?;

        Ok(())
    }

    #[tokio::test]
    async fn bridge_deposit_validation_ton() -> Result<()> {
        let validation = create_validation_object();

        let wallet_id = staging_wallet_id();
        let message =
            hex::decode("bcb143828f64d7e4bf0b6a8e66a2a2d03c916c16e9e9034419ae778b9f699d3c")?;
        let payload = HotVerifyBridge::Deposit(DepositAction {
            chain_id: ChainId::TON_V2,
            data: DepositData {
                sender: Some(vec![0; 32]),
                receiver: Some(vec![0; 32]),
                token_id: Some(vec![]),
                amount: Some(0),
                nonce: 1_753_218_716_000_000_003_679,
            },
        });
        let json = serde_json::to_value(&payload)?;
        dbg!(&json);
        let proof = ProofModel {
            message_body: String::new(),
            user_payloads: vec![json.to_string()],
        };

        validation.verify(wallet_id, message, proof).await?;

        Ok(())
    }

    #[tokio::test]
    async fn bridge_withdraw_removal_validation_ton() -> Result<()> {
        let validation = create_validation_object();

        let wallet_id = staging_wallet_id();
        let message =
            hex::decode("c45c5f7a9abba84c7ae06d1fe29e043e47dec94319d996e19d9e62757bd5fb5a")?;
        let payload = HotVerifyBridge::ClearCompletedWithdrawal(CompletedWithdrawalAction {
            chain_id: ChainId::TON_V2,
            data: CompletedWithdrawal {
                nonce: 1_753_218_716_000_000_003_679,
                receiver_address: Some(
                    "UQA3zc65LQyIR9SoDniLaZA0UDPudeiNs6P06skYcCuCtw8I".to_string(),
                ),
            },
        });
        let json = serde_json::to_value(&payload)?;
        dbg!(&json);
        let proof = ProofModel {
            message_body: String::new(),
            user_payloads: vec![json.to_string()],
        };

        validation.verify(wallet_id, message, proof).await?;

        Ok(())
    }

    #[tokio::test]
    async fn bridge_withdraw_removal_validation_stellar() -> Result<()> {
        let validation = create_validation_object();

        let wallet_id = staging_wallet_id();
        let message =
            hex::decode("8b7a6c9c9ea6efad319a472f3447a1d1847ddc0188959e4167821135f9f0ba52")?;

        let payload = HotVerifyBridge::ClearCompletedWithdrawal(CompletedWithdrawalAction {
            chain_id: ChainId::Stellar,
            data: CompletedWithdrawal {
                nonce: 1_754_631_474_000_000_070_075,
                receiver_address: Some("dontcare".to_string()),
            },
        });
        let json = serde_json::to_value(&payload)?;
        dbg!(&json);
        let proof = ProofModel {
            message_body: String::new(),
            user_payloads: vec![json.to_string()],
        };

        validation.verify(wallet_id, message, proof).await?;

        Ok(())
    }

    #[tokio::test]
    async fn bridge_withdraw_removal_validation_evm() -> Result<()> {
        let validation = create_validation_object();

        let wallet_id = staging_wallet_id();
        let message =
            hex::decode("8bd51d3368eeabd76957a0666c06fac90e9b1d2e366ece0a1229c15cc8e9d76a")?;

        let payload = HotVerifyBridge::ClearCompletedWithdrawal(CompletedWithdrawalAction {
            chain_id: ChainId::Evm(56),
            data: CompletedWithdrawal {
                nonce: 1_754_790_996_000_000_073_027,
                receiver_address: Some("dontcare".to_string()),
            },
        });
        let json = serde_json::to_value(&payload)?;
        dbg!(&json);
        let proof = ProofModel {
            message_body: String::new(),
            user_payloads: vec![json.to_string()],
        };

        validation.verify(wallet_id, message, proof).await?;

        Ok(())
    }

    #[tokio::test]
    async fn bridge_deposit_validation_solana() -> Result<()> {
        let validation = create_validation_object();

        let wallet_id = staging_wallet_id();
        let message =
            hex::decode("bcb143828f64d7e4bf0b6a8e66a2a2d03c916c16e9e9034419ae778b9f699d3c")?;
        let payload = HotVerifyBridge::Deposit(DepositAction {
            chain_id: ChainId::Solana,
            data: DepositData {
                sender: Some(
                    bs58::decode("5eMysQ7ywu4D8pmN5RtDoPxbu5YbiEThQy8gaBcmMoho")
                        .into_vec()?
                        .try_into()
                        .unwrap(),
                ),
                receiver: Some(
                    bs58::decode("BJu6S7gT4gnx7AXPnghM7aYiS5dPfSUixqAZJq1Uqf4V")
                        .into_vec()?
                        .try_into()
                        .unwrap(),
                ),
                token_id: Some(
                    bs58::decode("BYPsjxa3YuZESQz1dKuBw1QSFCSpecsm8nCQhY5xbU1Z").into_vec()?,
                ),
                amount: Some(10_000_000),
                nonce: 1_757_984_522_000_007_228,
            },
        });
        let json = serde_json::to_value(&payload)?;
        dbg!(&json);
        let proof = ProofModel {
            message_body: String::new(),
            user_payloads: vec![json.to_string()],
        };

        validation.verify(wallet_id, message, proof).await?;

        Ok(())
    }

    #[tokio::test]
    async fn bridge_completed_withdrawal_validation_solana() -> Result<()> {
        let validation = create_validation_object();

        let wallet_id = staging_wallet_id();
        let message =
            hex::decode("170a154a02aa91beb4b2d29175028d8684ee38585b418f36600cdeeb6ca05a1c")?;

        let payload = HotVerifyBridge::ClearCompletedWithdrawal(CompletedWithdrawalAction {
            chain_id: ChainId::Solana,
            data: CompletedWithdrawal {
                nonce: 1_749_390_032_000_000_032_243,
                receiver_address: Some("5eMysQ7ywu4D8pmN5RtDoPxbu5YbiEThQy8gaBcmMoho".to_string()),
            },
        });
        let json = serde_json::to_value(&payload)?;
        dbg!(&json);
        let proof = ProofModel {
            message_body: String::new(),
            user_payloads: vec![json.to_string()],
        };

        validation.verify(wallet_id, message, proof).await?;

        Ok(())
    }

    #[tokio::test]
    async fn bridge_deposit_validation_cosmos_juno() -> Result<()> {
        let validation = create_validation_object();

        let uid = staging_wallet_id();
        let message = BASE64_STANDARD.decode("utaIqDt2xuY7c2V+b2JU1B+I5dJ10EbaFzvmLpjpx+U=")?;
        let payload = HotVerifyBridge::Deposit(DepositAction {
            chain_id: ChainId::Evm(4444_118),
            data: DepositData {
                sender: Some(vec![0; 32]),
                receiver: Some(vec![0; 32]),
                token_id: Some(vec![]),
                amount: Some(0),
                nonce: 1764175051000000000008,
            },
        });
        let json = serde_json::to_value(&payload)?;
        let proof = ProofModel {
            message_body: String::new(),
            user_payloads: vec![json.to_string()],
        };

        validation.verify(uid, message, proof).await?;

        Ok(())
    }
}
