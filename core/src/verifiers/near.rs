use crate::metrics::{tick_metrics_verify_success_attempts, tick_metrics_verify_total_attempts};
use crate::threshold_verifier::ThresholdVerifier;
use crate::verifiers::VerifierTag;
use crate::{
    metrics, AuthMethod, ChainValidationConfig, GetWalletArgs, Validation, VerifyArgs,
    WalletAuthMethods, HOT_VERIFY_METHOD_NAME, MPC_GET_WALLET_METHOD, MPC_HOT_WALLET_CONTRACT,
    TIMEOUT,
};
use anyhow::{Context, Result};
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use futures_util::future::BoxFuture;
use hot_validation_primitives::bridge::HotVerifyResult;
use hot_validation_primitives::ChainId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize)]
struct RpcParams {
    request_type: String,
    finality: String,
    account_id: String,
    method_name: String,
    args_base64: String,
}

impl RpcParams {
    pub fn build(account_id: String, method_name: String, args_base64: String) -> Self {
        Self {
            request_type: "call_function".to_string(),
            finality: "final".to_string(),
            account_id,
            method_name,
            args_base64,
        }
    }
}

#[derive(Serialize)]
struct RpcRequest {
    jsonrpc: String,
    id: String,
    method: String,
    params: RpcParams,
}

impl RpcRequest {
    pub fn build(account_id: String, method_name: String, args_base64: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: "dontcare".to_string(),
            method: "query".to_string(),
            params: RpcParams::build(account_id, method_name, args_base64),
        }
    }
}

#[derive(Clone)]
pub(crate) struct NearVerifier {
    client: Arc<reqwest::Client>,
    server: String,
}

impl NearVerifier {
    fn new(client: Arc<reqwest::Client>, server: String) -> Self {
        Self { client, server }
    }

    async fn get_wallet(&self, wallet_id: String) -> Result<WalletAuthMethods> {
        tick_metrics_verify_total_attempts(ChainId::Near);
        let method_args = GetWalletArgs { wallet_id };
        let args_base64 = BASE64_STANDARD.encode(serde_json::to_vec(&method_args)?);
        let rpc_args = RpcRequest::build(
            MPC_HOT_WALLET_CONTRACT.to_string(),
            MPC_GET_WALLET_METHOD.to_string(),
            args_base64,
        );
        let wallet_model = self
            .call_rpc(serde_json::to_value(&rpc_args)?)
            .await
            .context(format!("get_wallet failed when calling {}", self.server))?;
        let wallet_model = serde_json::from_slice::<WalletAuthMethods>(wallet_model.as_slice())?;
        tick_metrics_verify_success_attempts(ChainId::Near);
        Ok(wallet_model)
    }

    async fn call_rpc(&self, json: serde_json::Value) -> Result<Vec<u8>> {
        let response = self
            .client
            .post(&self.server)
            .json(&json)
            .timeout(TIMEOUT)
            .send()
            .await?;

        if response.status().is_success() {
            let value = response.json::<serde_json::Value>().await?;
            // Intended.
            //  Call result is bytes, which are wrapped in "Result", which is wrapped in "Result"
            let value = value
                .get("result")
                .context(format!("missing result: {value}"))?;
            let value = value
                .get("result")
                .context(format!("missing result: {value}"))?;
            let value = serde_json::from_value::<Vec<u8>>(value.clone())?;
            Ok(value)
        } else {
            Err(anyhow::anyhow!(
                "Failed to call {}: {}",
                self.server,
                response.status()
            ))
        }
    }

    async fn verify(
        &self,
        auth_contract_id: String,
        method_name: String,
        args: &VerifyArgs,
    ) -> Result<HotVerifyResult> {
        let args_base64 = BASE64_STANDARD.encode(serde_json::to_vec(args)?);
        let rpc_args = RpcRequest::build(auth_contract_id, method_name, args_base64);
        let rpc_args_json = serde_json::to_value(&rpc_args)?;
        let result_bytes = self.call_rpc(rpc_args_json).await?;
        let result_json = serde_json::from_slice::<serde_json::Value>(result_bytes.as_slice())?;
        // There is some bs bug, where serializing from serde_json::Value doesn't work correctly
        // with `serde_with/SerHexSeq` due to some owned bytes... so using from_slice/from_str instead.
        let result = serde_json::from_slice::<HotVerifyResult>(result_bytes.as_slice()).context(
            format!("Failed to deserialize verify result: {result_json}"),
        )?;
        Ok(result)
    }
}

impl VerifierTag for NearVerifier {
    fn get_endpoint(&self) -> &str {
        self.server.as_str()
    }
}

impl ThresholdVerifier<NearVerifier> {
    pub(crate) fn new_near(
        near_validation_config: ChainValidationConfig,
        client: &Arc<reqwest::Client>,
    ) -> Self {
        let threshold = near_validation_config.threshold;
        let servers = near_validation_config.servers;
        assert!(
            (threshold <= servers.len()),
            "There should be at least {} servers, got {}",
            threshold,
            servers.len()
        );
        let callers = servers
            .iter()
            .map(|s| {
                let verifier = NearVerifier::new(client.clone(), s.clone());
                Arc::new(verifier)
            })
            .collect();
        Self {
            threshold,
            verifiers: callers,
        }
    }

    pub async fn get_wallet_auth_methods(
        self: Arc<Self>,
        wallet_id: &str,
    ) -> Result<WalletAuthMethods> {
        let _timer = metrics::RPC_GET_AUTH_METHODS_DURATION.start_timer();

        let functor =
            |verifier: Arc<NearVerifier>| -> BoxFuture<'static, Result<WalletAuthMethods>> {
                let wallet_id = wallet_id.to_string();
                Box::pin(async move {
                    verifier.get_wallet(wallet_id).await.context(format!(
                        "Error calling `get_wallet` with {}",
                        verifier.sanitized_endpoint()
                    ))
                })
            };

        self.threshold_call(functor).await
    }

    pub async fn verify(
        &self,
        auth_contract_id: String,
        method_name: String,
        args: VerifyArgs,
    ) -> Result<HotVerifyResult> {
        let args = Arc::new(args);
        let functor =
            move |verifier: Arc<NearVerifier>| -> BoxFuture<'static, Result<HotVerifyResult>> {
                Box::pin(async move {
                    verifier
                        .verify(auth_contract_id, method_name, &args)
                        .await
                        .context(format!(
                            "Error calling near `verify` with {}",
                            verifier.sanitized_endpoint()
                        ))
                })
            };

        let result = self.threshold_call(functor).await?;
        Ok(result)
    }
}

impl Validation {
    pub(crate) async fn handle_near(
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
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(clippy::should_panic_without_expect)]
    use super::*;
    use crate::{uid_to_wallet_id, AuthMethod, ChainId, HOT_VERIFY_METHOD_NAME};

    pub(crate) fn near_rpc() -> String {
        dotenv::var("NEAR_RPC").unwrap_or_else(|_| "https://rpc.mainnet.near.org".to_string())
    }

    #[tokio::test]
    async fn near_single_verifier() {
        let client = Arc::new(reqwest::Client::new());
        let rpc_caller = NearVerifier::new(client, near_rpc());

        let wallet_id = "A8NpkSkn1HZPYjxJRCpD4iPhDHzP81bbduZTqPpHmEgn".to_string();
        let auth_contract_id: &str = "keys.auth.hot.tg";

        let args = VerifyArgs {
            msg_body: String::new(),
            msg_hash: "6vLRVXiHvroXw1LEU1BNhz7QSaG73U41WM45m87X55H3".to_string(),
            wallet_id: Some(wallet_id),
            user_payload: r#"{"auth_method":0,"signatures":["HZUhhJamfp8GJLL8gEa2F2qZ6TXPu4PYzzWkDqsTQsMcW9rQsG2Hof4eD2Vex6he2fVVy3UNhgi631CY8E9StAH"]}"#.to_string(),
            metadata: None,
        };

        rpc_caller
            .verify(
                auth_contract_id.to_string(),
                HOT_VERIFY_METHOD_NAME.to_string(),
                &args,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    #[should_panic]
    async fn near_single_verifier_bad_wallet() {
        let client = Arc::new(reqwest::Client::new());
        let rpc_caller = NearVerifier::new(client, near_rpc());

        let wallet_id = "B8NpkSkn1HZPYjxJRCpD4iPhDHzP81bbduZTqPpHmEgn".to_string();
        let auth_contract_id: &str = "keys.auth.hot.tg";

        let args = VerifyArgs {
            msg_body: String::new(),
            msg_hash: "6vLRVXiHvroXw1LEU1BNhz7QSaG73U41WM45m87X55H3".to_string(),
            wallet_id: Some(wallet_id),
            user_payload: r#"{"auth_method":0,"signatures":["HZUhhJamfp8GJLL8gEa2F2qZ6TXPu4PYzzWkDqsTQsMcW9rQsG2Hof4eD2Vex6he2fVVy3UNhgi631CY8E9StAH"]}"#.to_string(),
            metadata: None,
        };

        rpc_caller
            .verify(
                auth_contract_id.to_string(),
                HOT_VERIFY_METHOD_NAME.to_string(),
                &args,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    #[should_panic]
    async fn near_single_verifier_bad_auth_contract() {
        let client = Arc::new(reqwest::Client::new());
        let rpc_caller = NearVerifier::new(client, near_rpc());

        let wallet_id = "A8NpkSkn1HZPYjxJRCpD4iPhDHzP81bbduZTqPpHmEgn".to_string();
        let auth_contract_id: &str = "123123.auth.hot.tg";

        let args = VerifyArgs {
            msg_body: String::new(),
            msg_hash: "6vLRVXiHvroXw1LEU1BNhz7QSaG73U41WM45m87X55H3".to_string(),
            wallet_id: Some(wallet_id),
            user_payload: r#"{"auth_method":0,"signatures":["HZUhhJamfp8GJLL8gEa2F2qZ6TXPu4PYzzWkDqsTQsMcW9rQsG2Hof4eD2Vex6he2fVVy3UNhgi631CY8E9StAH"]}"#.to_string(),
            metadata: None,
        };

        rpc_caller
            .verify(
                auth_contract_id.to_string(),
                HOT_VERIFY_METHOD_NAME.to_string(),
                &args,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn near_single_verifier_bad_msg_hash() -> Result<()> {
        let client = Arc::new(reqwest::Client::new());
        let rpc_caller = NearVerifier::new(client, near_rpc());

        let wallet_id = "A8NpkSkn1HZPYjxJRCpD4iPhDHzP81bbduZTqPpHmEgn".to_string();
        let auth_contract_id: &str = "keys.auth.hot.tg";

        let args = VerifyArgs {
            msg_body: String::new(),
            msg_hash: "7vLRVXiHvroXw1LEU1BNhz7QSaG73U41WM45m87X55H3".to_string(),
            wallet_id: Some(wallet_id),
            user_payload: r#"{"auth_method":0,"signatures":["HZUhhJamfp8GJLL8gEa2F2qZ6TXPu4PYzzWkDqsTQsMcW9rQsG2Hof4eD2Vex6he2fVVy3UNhgi631CY8E9StAH"]}"#.to_string(),
            metadata: None,
        };

        let result = rpc_caller
            .verify(
                auth_contract_id.to_string(),
                HOT_VERIFY_METHOD_NAME.to_string(),
                &args,
            )
            .await?
            .as_result()?;
        assert!(!result);
        Ok(())
    }

    #[tokio::test]
    async fn near_threshold_verifier() {
        let rpc_validation = ThresholdVerifier::new_near(
            ChainValidationConfig {
                threshold: 2,
                servers: vec![
                    "https://rpc.mainnet.near.org".to_string(),
                    "https://rpc.near.org".to_string(),
                    "https://nearrpc.aurora.dev".to_string(),
                    near_rpc(),
                ],
            },
            &Arc::new(reqwest::Client::new()),
        );

        let wallet_id = "A8NpkSkn1HZPYjxJRCpD4iPhDHzP81bbduZTqPpHmEgn".to_string();
        let auth_contract_id: &str = "keys.auth.hot.tg";
        let args = VerifyArgs {
            msg_body: String::new(),
            msg_hash: "6vLRVXiHvroXw1LEU1BNhz7QSaG73U41WM45m87X55H3".to_string(),
            wallet_id: Some(wallet_id),
            user_payload: r#"{"auth_method":0,"signatures":["HZUhhJamfp8GJLL8gEa2F2qZ6TXPu4PYzzWkDqsTQsMcW9rQsG2Hof4eD2Vex6he2fVVy3UNhgi631CY8E9StAH"]}"#.to_string(),
            metadata: None,
        };

        rpc_validation
            .verify(
                auth_contract_id.to_string(),
                HOT_VERIFY_METHOD_NAME.to_string(),
                args,
            )
            .await
            .unwrap();
    }

    #[should_panic]
    #[tokio::test]
    async fn near_threshold_verifier_all_rpcs_bad() {
        let rpc_validation = ThresholdVerifier::new_near(
            ChainValidationConfig {
                threshold: 2,
                servers: vec![
                    "https://hello.com".to_string(),
                    "https://hello.com".to_string(),
                    "https://hello.com".to_string(),
                    "https://hello.com".to_string(),
                ],
            },
            &Arc::new(reqwest::Client::new()),
        );

        let wallet_id = "A8NpkSkn1HZPYjxJRCpD4iPhDHzP81bbduZTqPpHmEgn".to_string();
        let auth_contract_id: &str = "keys.auth.hot.tg";
        let args = VerifyArgs {
            msg_body: String::new(),
            msg_hash: "6vLRVXiHvroXw1LEU1BNhz7QSaG73U41WM45m87X55H3".to_string(),
            wallet_id: Some(wallet_id),
            user_payload: r#"{"auth_method":0,"signatures":["HZUhhJamfp8GJLL8gEa2F2qZ6TXPu4PYzzWkDqsTQsMcW9rQsG2Hof4eD2Vex6he2fVVy3UNhgi631CY8E9StAH"]}"#.to_string(),
            metadata: None,
        };

        rpc_validation
            .verify(
                auth_contract_id.to_string(),
                HOT_VERIFY_METHOD_NAME.to_string(),
                args,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn near_single_verifier_get_wallet() {
        let client = Arc::new(reqwest::Client::new());
        let rpc_caller = NearVerifier::new(client, near_rpc());

        let wallet_id = "A8NpkSkn1HZPYjxJRCpD4iPhDHzP81bbduZTqPpHmEgn";
        let expected = WalletAuthMethods {
            access_list: vec![AuthMethod {
                account_id: "keys.auth.hot.tg".to_string(),
                metadata: None,
                chain_id: ChainId::Near,
            }],
            key_gen: 1,
            block_height: 0,
        };

        let actual = rpc_caller.get_wallet(wallet_id.to_string()).await.unwrap();
        assert_eq!(actual, expected);
    }

    #[tokio::test]
    async fn near_single_verifier_get_wallet_with_meta() {
        let client = Arc::new(reqwest::Client::new());
        let rpc_caller = NearVerifier::new(client, near_rpc());

        let wallet_id =
            uid_to_wallet_id("fe62128e531a7f7c15e9f919db9ff1d112e5d23c3ef9e23723224c2358c0b496")
                .unwrap();
        let expected = WalletAuthMethods {
            access_list: vec![AuthMethod {
                account_id: "drops.nfts.tg".to_string(),
                metadata: Some("{\"method\": \"hot_verify_deposit\"}".to_string()),
                chain_id: ChainId::Near,
            }],
            key_gen: 1,
            block_height: 0,
        };

        let actual = rpc_caller.get_wallet(wallet_id.to_string()).await.unwrap();
        assert_eq!(actual, expected);
    }

    #[tokio::test]
    async fn threshold_verifier_get_wallet() {
        let rpc_validation = ThresholdVerifier::new_near(
            ChainValidationConfig {
                threshold: 2,
                servers: vec![
                    "https://rpc.mainnet.near.org".to_string(),
                    "https://rpc.near.org".to_string(),
                    "https://nearrpc.aurora.dev".to_string(),
                    near_rpc(),
                ],
            },
            &Arc::new(reqwest::Client::new()),
        );

        let wallet_id = "A8NpkSkn1HZPYjxJRCpD4iPhDHzP81bbduZTqPpHmEgn";
        let expected = WalletAuthMethods {
            access_list: vec![AuthMethod {
                account_id: "keys.auth.hot.tg".to_string(),
                metadata: None,
                chain_id: ChainId::Near,
            }],
            key_gen: 1,
            block_height: 0,
        };

        let actual = Arc::new(rpc_validation)
            .get_wallet_auth_methods(wallet_id)
            .await
            .unwrap();

        assert_eq!(actual, expected);
    }

    #[tokio::test]
    async fn threshold_verifier_get_wallet_bad_rpcs() {
        let rpc_validation = ThresholdVerifier::new_near(
            ChainValidationConfig {
                threshold: 2,
                servers: vec![
                    "https://google.com".to_string(),
                    "https://bim-bim-bom-bom.com".to_string(),
                    "https://rpc.mainnet.near.org".to_string(),
                    "https://hello.dev".to_string(),
                    "https://rpc.near.org".to_string(),
                    "https://nearrpc.aurora.dev".to_string(),
                    near_rpc(),
                ],
            },
            &Arc::new(reqwest::Client::new()),
        );

        let expected = WalletAuthMethods {
            access_list: vec![AuthMethod {
                account_id: "keys.auth.hot.tg".to_string(),
                metadata: None,
                chain_id: ChainId::Near,
            }],
            key_gen: 1,
            block_height: 0,
        };

        let wallet_id = "A8NpkSkn1HZPYjxJRCpD4iPhDHzP81bbduZTqPpHmEgn";
        let actual = Arc::new(rpc_validation)
            .get_wallet_auth_methods(wallet_id)
            .await
            .unwrap();

        assert_eq!(actual, expected);
    }

    #[test]
    fn converter_to_base58_correct() {
        let uid = "0887d14fbe253e8b6a7b8193f3891e04f88a9ed744b91f4990d567ffc8b18e5f";
        let expected = "A8NpkSkn1HZPYjxJRCpD4iPhDHzP81bbduZTqPpHmEgn";
        let actual = uid_to_wallet_id(uid).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    #[should_panic]
    fn converter_to_base58_incorrect() {
        let uid = "sha256 expected as uid";
        uid_to_wallet_id(uid).unwrap();
    }

    #[test]
    fn get_wallet_data_model_correct() {
        let sample_json = r#"{
            "access_list": [
                {
                    "account_id": "keys.auth.hot.tg",
                    "metadata": null,
                    "chain_id": 0
                }
            ],
            "key_gen": 1,
            "block_height": 0
        }"#;

        let expected = WalletAuthMethods {
            access_list: vec![AuthMethod {
                account_id: "keys.auth.hot.tg".to_string(),
                metadata: None,
                chain_id: ChainId::Near,
            }],
            key_gen: 1,
            block_height: 0,
        };

        let actual: WalletAuthMethods = serde_json::from_str(sample_json).unwrap();

        assert_eq!(actual, expected);
    }
}
