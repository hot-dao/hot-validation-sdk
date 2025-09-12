mod evm;
mod internals;
mod metrics;
mod near;
mod stellar;
mod ton;

pub use hot_validation_primitives::*;

use crate::evm::EvmSingleVerifier;
use crate::internals::{uid_to_wallet_id, ThresholdVerifier, VerifyArgs};
use crate::near::NearSingleVerifier;
use crate::stellar::StellarSingleVerifier;
use crate::ton::TonSingleVerifier;
use anyhow::{bail, Context, Result};
use futures_util::future::try_join_all;
use hot_validation_rpc_healthcheck::observer::Observer;
use std::collections::HashMap;
use std::sync::Arc;

/// The logic that prevents signing arbitrary messages.
#[derive(Clone)]
pub struct Validation {
    near: Arc<ThresholdVerifier<NearSingleVerifier>>,
    evm: HashMap<ChainId, Arc<ThresholdVerifier<EvmSingleVerifier>>>,
    stellar: Arc<ThresholdVerifier<StellarSingleVerifier>>,
    ton: Arc<ThresholdVerifier<TonSingleVerifier>>,
    health_check_observer: Arc<Observer>,
}

impl Validation {
    pub fn metrics(&self) -> Arc<Observer> {
        self.health_check_observer.clone()
    }

    pub fn new(configs: HashMap<ChainId, ChainValidationConfig>) -> Result<Self> {
        let client: Arc<reqwest::Client> = Arc::new(reqwest::Client::new());

        let near_config = configs
            .get(&ChainId::Near)
            .expect("No near config (chain_id = 0) found")
            .clone();

        let near_validation = {
            let verifier = ThresholdVerifier::new_near(near_config, client.clone());
            Arc::new(verifier)
        };

        // TODO: Logic separation
        let stellar_config = configs
            .get(&ChainId::Stellar)
            .expect("No stellar config (chain_id = 1100) found")
            .clone();

        let stellar_validation = {
            let verifier = ThresholdVerifier::new_stellar(stellar_config)?;
            Arc::new(verifier)
        };

        let evm_validation = configs
            .clone()
            .into_iter()
            .filter(|(id, _)| matches!(id, ChainId::Evm(_)))
            .map(|(id, config)| {
                let threshold_verifier = {
                    let verifier = ThresholdVerifier::new_evm(config, client.clone());
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
            let verifier = ThresholdVerifier::new_ton(config, client.clone());
            Arc::new(verifier)
        };

        let health_check_observer = Arc::new(Observer::new(configs));

        let validation = Self {
            near: near_validation, // TODO: Direct naming
            evm: evm_validation,
            stellar: stellar_validation,
            ton,
            health_check_observer,
        };
        Ok(validation)
    }

    pub async fn verify(
        self: Arc<Self>,
        uid: String,
        message_hex: String,
        proof: ProofModel,
    ) -> Result<()> {
        let _timer = metrics::RPC_VERIFY_TOTAL_DURATION.start_timer();

        let wallet_id = uid_to_wallet_id(&uid).context("Couldn't convert uid to wallet_id")?;
        // TODO: unnecessary threshold call, i.e. all validation logic should be done linearly,
        // and threshold should be checked on the result, not on intermediate steps.
        let wallet = self
            .near
            .clone()
            .get_wallet_auth_methods(&wallet_id)
            .await
            .context("Couldn't get wallet info")?;

        if proof.user_payloads.len() != wallet.access_list.len() {
            bail!(
                "Length of provided user payloads ({}) doesn't match with required wallet authorization ({})",
                proof.user_payloads.len(),
                wallet.access_list.len()
            );
        }

        let result = try_join_all(
            wallet
                .access_list
                .into_iter()
                .zip(proof.user_payloads.into_iter())
                .map(|(auth_method, user_payload)| {
                    self.clone().verify_auth_method(
                        wallet_id.clone(),
                        auth_method,
                        proof.message_body.clone(),
                        message_hex.clone(),
                        user_payload,
                    )
                }),
        )
        .await;

        result?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_validation_object() -> Arc<Validation> {
        let configs = HashMap::from([
            (
                ChainId::Near,
                ChainValidationConfig {
                    threshold: 2,
                    servers: vec![
                        "http://167.235.180.39:3030/".to_string(),
                        "https://rpc.near.org".to_string(),
                        "http://ffooooo-bbbaaaar:3030/".to_string(),
                        "https://nearrpc.aurora.dev".to_string(),
                        "https://1rpc.io/near".to_string(),
                        "https://allthatnode.com/protocol/near.dsrv".to_string(),
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
                    ],
                },
            ),
            (
                ChainId::Evm(56),
                ChainValidationConfig {
                    threshold: 1,
                    servers: vec!["https://bsc.drpc.org".to_string()],
                },
            ),
            (
                ChainId::TON_V2,
                ChainValidationConfig {
                    threshold: 1,
                    servers: vec!["https://toncenter.com/api/v2".to_string()],
                },
            ),
        ]);

        let validation = Validation::new(configs).unwrap();
        Arc::new(validation)
    }

    #[tokio::test]
    async fn validate_on_near() {
        let validation = create_validation_object();

        let uid = "0887d14fbe253e8b6a7b8193f3891e04f88a9ed744b91f4990d567ffc8b18e5f".to_string();
        let message =
            "57f42da8350f6a7c6ad567d678355a3bbd17a681117e7a892db30656d5caee32".to_string();
        let proof = ProofModel {
            message_body: "S8safEk4JWgnJsVKxans4TqBL796cEuV5GcrqnFHPdNW91AupymrQ6zgwEXoeRb6P3nyaSskoFtMJzaskXTDAnQUTKs5dGMWQHsz7irQJJ2UA2aDHSQ4qxgsU3h1U83nkq4rBstK8PL1xm6WygSYihvBTmuaMjuKCK6JT1tB4Uw71kGV262kU914YDwJa53BiNLuVi3s2rj5tboEwsSEpyJo9x5diq4Ckmzf51ZjZEDYCH8TdrP1dcY4FqkTCBA7JhjfCTToJR5r74ApfnNJLnDhTxkvJb4ReR9T9Ga7hPNazCFGE8Xq1deu44kcPjXNvb1GJGWLAZ5k1wxq9nnARb3bvkqBTmeYiDcPDamauhrwYWZkMNUsHtoMwF6286gcmY3ZgE3jja1NGuYKYQHnvscUqcutuT9qH".to_string(),
            user_payloads: vec![r#"{"auth_method":0,"signatures":["HZUhhJamfp8GJLL8gEa2F2qZ6TXPu4PYzzWkDqsTQsMcW9rQsG2Hof4eD2Vex6he2fVVy3UNhgi631CY8E9StAH"]}"#.to_string()],
        };

        validation.verify(uid, message, proof).await.unwrap();
    }

    #[tokio::test]
    async fn validate_on_base() {
        let validation = create_validation_object();

        let uid = "6c2015fd2a1a858144749d55d0f38f0632b8342f59a2d44ee374d64047b0f4f4".to_string();
        let message =
            "ef32edffb454d2a3172fd0af3fdb0e43fac5060a929f1b83b6de2b73754e3f45".to_string();
        let proof = ProofModel {
            message_body: "S8safEk4JWgnJsVKxans4TqBL796cEuV5GcrqnFHPdNW91AupymrQ6zgwEXoeRb6P3nyaSskoFtMJzaskXTDAnQUTKs5dGMWQHsz7irQJJ2UA2aDHSQ4qxgsU3h1U83nkq4rBstK8PL1xm6WygSYihvBTmuaMjuKCK6JT1tB4Uw71kGV262kU914YDwJa53BiNLuVi3s2rj5tboEwsSEpyJo9x5diq4Ckmzf51ZjZEDYCH8TdrP1dcY4FqkTCBA7JhjfCTToJR5r74ApfnNJLnDhTxkvJb4ReR9T9Ga7hPNazCFGE8Xq1deu44kcPjXNvb1GJGWLAZ5k1wxq9nnARb3bvkqBTmeYiDcPDamauhrwYWZkMNUsHtoMwF6286gcmY3ZgE3jja1NGuYKYQHnvscUqcutuT9qH".to_string(),
            user_payloads: vec!["00000000000000000000000000000000000000000000005e095d2c286c4414050000000000000000000000000000000000000000000000000000000000000000".to_string()],
        };

        validation.verify(uid, message, proof).await.unwrap();
    }

    #[tokio::test]
    async fn two_auth_methods() {
        let validation = create_validation_object();

        let uid = "114e0efee6a1c73dbc8403264db8537d38fdfa7bdf81ed6fcf4841b93b9a2b6a".to_string();
        let message =
            "6484f06d86d1aee5ee53411f6033181eb0c5cde57081a798f4f6bfbe01a443e4".to_string();
        let proof = ProofModel {
            message_body: "".to_string(),
            user_payloads: vec![
                "{\"signatures\": [\"2r4RNC49RGA6Wqo5VzZtATBs3jMvqZCo5NYfJGkDpHZd598Zvt7kFfiuH8yr26CynzSMsgoHYoMUF5h31dSVHAT1\"], \"auth_method\": 0}".to_string(),
                "00000000000000000000000000000000000000000000005e9def3f04597b183c0000000000000000000000000000000000000000000000000000000000000000".to_string()
            ],
        };

        validation.verify(uid, message, proof).await.unwrap();
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
                        "http://167.235.180.39:3030/".to_string(),
                        "https://rpc.near.org".to_string(),
                        "http://ffooooo-bbbaaaar:3030/".to_string(),
                        "https://nearrpc.aurora.dev".to_string(),
                        "https://1rpc.io/near".to_string(),
                        "https://allthatnode.com/protocol/near.dsrv".to_string(),
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
        let validation = Arc::new(Validation::new(configs).unwrap());

        let uid = "114e0efee6a1c73dbc8403264db8537d38fdfa7bdf81ed6fcf4841b93b9a2b6a".to_string();
        let message =
            "6484f06d86d1aee5ee53411f6033181eb0c5cde57081a798f4f6bfbe01a443e4".to_string();
        let proof = ProofModel {
            message_body: "".to_string(),
            user_payloads: vec![
                "{\"signatures\": [\"2r4RNC49RGA6Wqo5VzZtATBs3jMvqZCo5NYfJGkDpHZd598Zvt7kFfiuH8yr26CynzSMsgoHYoMUF5h31dSVHAT1\"], \"auth_method\": 0}".to_string(),
                "00000000000000000000000000000000000000000000005e9def3f04597b183c0000000000000000000000000000000000000000000000000000000000000000".to_string()
            ],
        };

        validation.verify(uid, message, proof).await.unwrap();
    }

    #[tokio::test]
    async fn validate_on_stellar() {
        let validation = create_validation_object();

        let uid = "bfe2d1d813e759844d1f0617639c986a52427a5965a1e72392cd0f6b4d556074".to_string();
        let message = "".to_string();
        let proof = ProofModel {
            message_body: "".to_string(),
            user_payloads: vec!["000000000000005ee4a2fbf444c19970b2289e4ab3eb2ae2e73063a5f5dfc450db7b07413f2d905db96414e0c33eb204".to_string()],
        };

        validation.verify(uid, message, proof).await.unwrap();
    }

    #[tokio::test]
    async fn bridge_deposit_validation_evm() -> Result<()> {
        let validation = create_validation_object();

        let uid = "9d02632f3fe9d7b89504e6d00174c1d4402900a23020c7f96d289c2f1a5af533".to_string();
        let message =
            "c4ea3c95f2171df3fa5a6f8452d1bbbbd0608abe68fdcea7f25a04516c50cba6".to_string();
        let proof = ProofModel {
            message_body: "".to_string(),
            user_payloads: vec![
                "{\"Deposit\":{\"chain_id\":56,\"nonce\":\"1754431900000000013182\"}}".to_string(),
            ],
        };

        validation.verify(uid, message, proof).await?;
        Ok(())
    }

    #[tokio::test]
    async fn bridge_deposit_validation_stellar() -> Result<()> {
        let validation = create_validation_object();

        let uid = "9d02632f3fe9d7b89504e6d00174c1d4402900a23020c7f96d289c2f1a5af533".to_string();
        let message =
            "c9a9f00772fcf664b4a8fefb93170d1a6f0e9843a2a816797bab71b6a99ca881".to_string();
        let proof = ProofModel {
            message_body: "".to_string(),
            user_payloads: vec![
                "{\"Deposit\":{\"chain_id\":1100,\"nonce\":\"1754531354365901458000\"}}"
                    .to_string(),
            ],
        };

        validation.verify(uid, message, proof).await?;

        Ok(())
    }

    #[tokio::test]
    async fn bridge_deposit_validation_ton() -> Result<()> {
        let validation = create_validation_object();

        let uid = "f44a64989027d8fea9037e190efe7ad830b9646acac406402f8771bec83d5b36".to_string();
        let message =
            "bcb143828f64d7e4bf0b6a8e66a2a2d03c916c16e9e9034419ae778b9f699d3c".to_string();
        let proof = ProofModel {
            message_body: "".to_string(),
            user_payloads: vec![
                "{\"Deposit\":{\"chain_id\":1117,\"nonce\":\"1753218716000000003679\"}}"
                    .to_string(),
            ],
        };

        validation.verify(uid, message, proof).await?;

        Ok(())
    }

    #[tokio::test]
    async fn bridge_withdraw_removal_validation_ton() -> Result<()> {
        let validation = create_validation_object();

        let uid = "f44a64989027d8fea9037e190efe7ad830b9646acac406402f8771bec83d5b36".to_string();
        let message =
            "c45c5f7a9abba84c7ae06d1fe29e043e47dec94319d996e19d9e62757bd5fb5a".to_string();
        let proof = ProofModel {
            message_body: "".to_string(),
            user_payloads: vec![json!({
                "ClearCompletedWithdrawal": {
                    "Ton": {
                        "user_ton_address": "UQA3zc65LQyIR9SoDniLaZA0UDPudeiNs6P06skYcCuCtw8I",
                        "chain_id": 1117,
                        "nonce": "1753218716000000003679",
                    }
                }
            })
            .to_string()],
        };

        validation.verify(uid, message, proof).await?;

        Ok(())
    }

    #[tokio::test]
    async fn bridge_withdraw_removal_validation_stellar() -> Result<()> {
        let validation = create_validation_object();

        let uid = "9d02632f3fe9d7b89504e6d00174c1d4402900a23020c7f96d289c2f1a5af533".to_string();
        let message =
            "8b7a6c9c9ea6efad319a472f3447a1d1847ddc0188959e4167821135f9f0ba52".to_string();

        let proof = ProofModel {
            message_body: "".to_string(),
            user_payloads: vec![r#"
                    {
                      "Withdraw": {
                        "chain_id": 1100,
                        "nonce": "1754631474000000070075"
                      }
                    }
                "#
            .to_string()],
        };

        validation.verify(uid, message, proof).await?;

        Ok(())
    }

    #[tokio::test]
    async fn bridge_withdraw_removal_validation_evm() -> Result<()> {
        let validation = create_validation_object();

        let uid = "9d02632f3fe9d7b89504e6d00174c1d4402900a23020c7f96d289c2f1a5af533".to_string();
        let message =
            "8bd51d3368eeabd76957a0666c06fac90e9b1d2e366ece0a1229c15cc8e9d76a".to_string();

        let proof = ProofModel {
            message_body: "".to_string(),
            user_payloads: vec![r#"
                    {
                      "Withdraw": {
                        "chain_id": 56,
                        "nonce": "1754790996000000073027"
                      }
                    }
                "#
            .to_string()],
        };

        validation.verify(uid, message, proof).await?;

        Ok(())
    }
}
