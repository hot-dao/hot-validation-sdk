mod types;

use std::fmt::Debug;
use std::hash::Hash;
use crate::http_client::post_json_receive_json;
use crate::threshold_verifier::{Identifiable, ThresholdVerifier};
use crate::verifiers::near::types::{GetWalletArgs, RpcRequest, RpcResponse, VerifyArgs};
use crate::{
    metrics, AuthMethod, ChainValidationConfig, WalletAuthMethods, HOT_VERIFY_METHOD_NAME,
    MPC_GET_WALLET_METHOD, MPC_HOT_WALLET_CONTRACT,
};
use anyhow::Result;
use hot_validation_primitives::bridge::HotVerifyResult;
use hot_validation_primitives::ChainId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use serde::de::DeserializeOwned;
use hot_validation_primitives::uid::WalletId;

#[derive(Clone)]
pub struct NearVerifier {
    client: Arc<reqwest::Client>,
    server: String,
}

impl Identifiable for NearVerifier {
    fn id(&self) -> String {
        self.server.clone()
    }
}

impl NearVerifier {
    fn new(client: Arc<reqwest::Client>, server: String) -> Self {
        Self { client, server }
    }

    async fn get_wallet(&self, wallet_id: WalletId) -> Result<WalletAuthMethods> {
        let wallet_id = GetWalletArgs { wallet_id };
        self.call_view_method(
            MPC_HOT_WALLET_CONTRACT.to_string(),
            MPC_GET_WALLET_METHOD.to_string(),
            &wallet_id
        ).await
    }

    async fn verify(
        &self,
        wallet_id: WalletId,
        auth_method: AuthMethod,
        message_hex: String,
        message_body: String,
        user_payload: String,
    ) -> Result<HotVerifyResult> {
        #[derive(Debug, Deserialize)]
        struct MethodName {
            method: String,
        }
        // Used in omni bridge: there are different methods for deposit/withdraw flows.
        let method_name = if let Some(metadata) = &auth_method.metadata {
            let method_name = serde_json::from_str::<MethodName>(metadata)?;
            method_name.method
        } else {
            HOT_VERIFY_METHOD_NAME.to_string()
        };

        // TODO: maybe the message should be plain bytes in the first place, and base58 conversion
        //  put into serialization logic.
        let message_bs58 = hex::decode(&message_hex)
            .map(|message_bytes| bs58::encode(message_bytes).into_string())?;

        let args = VerifyArgs {
            wallet_id: Some(wallet_id),
            msg_hash: message_bs58,
            metadata: auth_method.metadata.clone(),
            user_payload: user_payload.clone(),
            msg_body: message_body.clone(),
        };

        self.call_view_method(
            auth_method.account_id,
            method_name,
            &args
        ).await
    }

    async fn call_view_method<R, T>(
        &self,
        account_id: String,
        method_name: String,
        args: T,
    ) -> Result<R>
    where
        T: Serialize + Sized,
        R: DeserializeOwned,
    {
        let rpc_args = RpcRequest::build(&account_id, &method_name, &args);
        let result: RpcResponse<R> =
            post_json_receive_json(&self.client, &self.server, &rpc_args, ChainId::Near).await?;
        Ok(result.unpack())
    }
}

impl ThresholdVerifier<NearVerifier> {
    pub(crate) fn new_near(
        near_validation_config: ChainValidationConfig,
        client: &Arc<reqwest::Client>,
    ) -> Self {
        let threshold = near_validation_config.threshold;
        let servers = near_validation_config.servers;
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
        self: &Arc<Self>,
        wallet_id: WalletId,
    ) -> Result<WalletAuthMethods> {
        let _timer = metrics::RPC_GET_AUTH_METHODS_DURATION.start_timer();
        self.threshold_call(move |verifier| {
            let wallet_id = wallet_id.clone();
            async move { verifier.get_wallet(wallet_id).await }
        })
        .await
    }

    pub async fn verify(
        &self,
        wallet_id: WalletId,
        auth_method: AuthMethod,
        message_hex: String,
        message_body: String,
        user_payload: String,
    ) -> Result<HotVerifyResult> {
        self.threshold_call(move |verifier| {
            let wallet_id = wallet_id.clone();
            let auth_method = auth_method.clone();
            let message_hex = message_hex.clone();
            let message_body = message_body.clone();
            let user_payload = user_payload.clone();
            async move {
                verifier
                    .verify(
                        wallet_id,
                        auth_method,
                        message_hex,
                        message_body,
                        user_payload,
                    )
                    .await
            }
        })
        .await
    }

    pub async fn call_view_method<T, R>(
        &self,
        account_id: String,
        method_name: String,
        args: T,
    ) -> Result<R>
    where
        T: Serialize + Sized + Sync + Clone + Send + 'static,
        R: DeserializeOwned + Eq + Hash + Clone + Debug,
    {
        self.threshold_call(move |verifier| {
            let account_id = account_id.clone();
            let method_name = method_name.clone();
            let args = args.clone();
            async move {
                verifier
                    .call_view_method(
                        account_id,
                        method_name,
                        args,
                    )
                    .await
            }
        })
            .await
    }
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(clippy::should_panic_without_expect)]

    use crate::threshold_verifier::ThresholdVerifier;
    use crate::verifiers::near::NearVerifier;
    use crate::{AuthMethod, WalletAuthMethods};
    use anyhow::Result;
    use hot_validation_primitives::ChainValidationConfig;
    use std::sync::Arc;
    use hot_validation_primitives::uid::WalletId;
    use crate::test_data::near_rpc;

    fn sample_wallet_id() -> WalletId {
        "A8NpkSkn1HZPYjxJRCpD4iPhDHzP81bbduZTqPpHmEgn".to_string().into()
    }

    #[tokio::test]
    async fn near_single_verifier() {
        let client = Arc::new(reqwest::Client::new());
        let rpc_caller = NearVerifier::new(client, near_rpc());

        let wallet_id = sample_wallet_id();

        let auth_method = AuthMethod {
            account_id: "keys.auth.hot.tg".to_string(),
            metadata: None,
        };

        let message_body = String::new();
        let message_hex = {
            let bs58 = "6vLRVXiHvroXw1LEU1BNhz7QSaG73U41WM45m87X55H3";
            let bytes = bs58::decode(bs58).into_vec().unwrap();
            hex::encode(bytes)
        };
        let user_payload = r#"{"auth_method":0,"signatures":["HZUhhJamfp8GJLL8gEa2F2qZ6TXPu4PYzzWkDqsTQsMcW9rQsG2Hof4eD2Vex6he2fVVy3UNhgi631CY8E9StAH"]}"#.to_string();

        rpc_caller
            .verify(
                wallet_id,
                auth_method,
                message_hex,
                message_body,
                user_payload,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    #[should_panic]
    async fn near_single_verifier_bad_wallet() {
        let client = Arc::new(reqwest::Client::new());
        let rpc_caller = NearVerifier::new(client, near_rpc());

        let wallet_id = "B8NpkSkn1HZPYjxJRCpD4iPhDHzP81bbduZTqPpHmEgn".to_string().into();

        let auth_method = AuthMethod {
            account_id: "keys.auth.hot.tg".to_string(),
            metadata: None,
        };

        let message_body = String::new();
        let message_hex = {
            let bs58 = "6vLRVXiHvroXw1LEU1BNhz7QSaG73U41WM45m87X55H3";
            let bytes = bs58::decode(bs58).into_vec().unwrap();
            hex::encode(bytes)
        };
        let user_payload = r#"{"auth_method":0,"signatures":["HZUhhJamfp8GJLL8gEa2F2qZ6TXPu4PYzzWkDqsTQsMcW9rQsG2Hof4eD2Vex6he2fVVy3UNhgi631CY8E9StAH"]}"#.to_string();

        rpc_caller
            .verify(
                wallet_id,
                auth_method,
                message_hex,
                message_body,
                user_payload,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    #[should_panic]
    async fn near_single_verifier_bad_auth_contract() {
        let client = Arc::new(reqwest::Client::new());
        let rpc_caller = NearVerifier::new(client, near_rpc());

        let wallet_id = sample_wallet_id();

        let auth_method = AuthMethod {
            account_id: "kek.auth.hot.tg".to_string(),
            metadata: None,
        };

        let message_body = String::new();
        let message_hex = {
            let bs58 = "6vLRVXiHvroXw1LEU1BNhz7QSaG73U41WM45m87X55H3";
            let bytes = bs58::decode(bs58).into_vec().unwrap();
            hex::encode(bytes)
        };
        let user_payload = r#"{"auth_method":0,"signatures":["HZUhhJamfp8GJLL8gEa2F2qZ6TXPu4PYzzWkDqsTQsMcW9rQsG2Hof4eD2Vex6he2fVVy3UNhgi631CY8E9StAH"]}"#.to_string();

        rpc_caller
            .verify(
                wallet_id,
                auth_method,
                message_hex,
                message_body,
                user_payload,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn near_single_verifier_bad_msg_hash() -> Result<()> {
        let client = Arc::new(reqwest::Client::new());
        let rpc_caller = NearVerifier::new(client, near_rpc());

        let wallet_id = sample_wallet_id();

        let auth_method = AuthMethod {
            account_id: "keys.auth.hot.tg".to_string(),
            metadata: None,
        };

        let message_body = String::new();
        let message_hex = {
            let bs58 = "7vLRVXiHvroXw1LEU1BNhz7QSaG73U41WM45m87X55H3";
            let bytes = bs58::decode(bs58).into_vec().unwrap();
            hex::encode(bytes)
        };
        let user_payload = r#"{"auth_method":0,"signatures":["HZUhhJamfp8GJLL8gEa2F2qZ6TXPu4PYzzWkDqsTQsMcW9rQsG2Hof4eD2Vex6he2fVVy3UNhgi631CY8E9StAH"]}"#.to_string();

        let result = rpc_caller
            .verify(
                wallet_id,
                auth_method,
                message_hex,
                message_body,
                user_payload,
            )
            .await;
        assert!(result.is_err());
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

        let wallet_id = sample_wallet_id();

        let auth_method = AuthMethod {
            account_id: "keys.auth.hot.tg".to_string(),
            metadata: None,
        };

        let message_body = String::new();
        let message_hex = {
            let bs58 = "6vLRVXiHvroXw1LEU1BNhz7QSaG73U41WM45m87X55H3";
            let bytes = bs58::decode(bs58).into_vec().unwrap();
            hex::encode(bytes)
        };
        let user_payload = r#"{"auth_method":0,"signatures":["HZUhhJamfp8GJLL8gEa2F2qZ6TXPu4PYzzWkDqsTQsMcW9rQsG2Hof4eD2Vex6he2fVVy3UNhgi631CY8E9StAH"]}"#.to_string();

        rpc_validation
            .verify(
                wallet_id,
                auth_method,
                message_hex,
                message_body,
                user_payload,
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
                    near_rpc(),
                ],
            },
            &Arc::new(reqwest::Client::new()),
        );

        let wallet_id = sample_wallet_id();

        let auth_method = AuthMethod {
            account_id: "keys.auth.hot.tg".to_string(),
            metadata: None,
        };

        let message_body = String::new();
        let message_hex = {
            let bs58 = "6vLRVXiHvroXw1LEU1BNhz7QSaG73U41WM45m87X55H3";
            let bytes = bs58::decode(bs58).into_vec().unwrap();
            hex::encode(bytes)
        };
        let user_payload = r#"{"auth_method":0,"signatures":["HZUhhJamfp8GJLL8gEa2F2qZ6TXPu4PYzzWkDqsTQsMcW9rQsG2Hof4eD2Vex6he2fVVy3UNhgi631CY8E9StAH"]}"#.to_string();

        rpc_validation
            .verify(
                wallet_id,
                auth_method,
                message_hex,
                message_body,
                user_payload,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn near_single_verifier_get_wallet() {
        let client = Arc::new(reqwest::Client::new());
        let rpc_caller = NearVerifier::new(client, near_rpc());

        let wallet_id = sample_wallet_id();

        let expected = WalletAuthMethods {
            access_list: vec![AuthMethod {
                account_id: "keys.auth.hot.tg".to_string(),
                metadata: None,
            }],
        };

        let actual = rpc_caller.get_wallet(wallet_id).await.unwrap();
        assert_eq!(actual.access_list, expected.access_list);
    }

    #[tokio::test]
    async fn near_single_verifier_get_wallet_with_meta() {
        let client = Arc::new(reqwest::Client::new());
        let rpc_caller = NearVerifier::new(client, near_rpc());

        let wallet_id = "Puvk3GR7bvBmJqg2Sdzs4D2AFGAW3rXq9iwpJraBkGJ".to_string().into();
        let expected = WalletAuthMethods {
            access_list: vec![AuthMethod {
                account_id: "drops.nfts.tg".to_string(),
                metadata: Some("{\"method\": \"hot_verify_deposit\"}".to_string()),
            }],
        };

        let actual = rpc_caller.get_wallet(wallet_id).await.unwrap();
        assert_eq!(actual.access_list, expected.access_list);
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

        let wallet_id = sample_wallet_id();

        let expected = WalletAuthMethods {
            access_list: vec![AuthMethod {
                account_id: "keys.auth.hot.tg".to_string(),
                metadata: None,
            }],
        };

        let actual = Arc::new(rpc_validation)
            .get_wallet_auth_methods(wallet_id)
            .await
            .unwrap();

        assert_eq!(actual.access_list, expected.access_list);
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
            }],
        };

        let wallet_id = sample_wallet_id();

        let actual = Arc::new(rpc_validation)
            .get_wallet_auth_methods(wallet_id)
            .await
            .unwrap();

        assert_eq!(actual.access_list, expected.access_list);
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
            }],
        };

        let actual: WalletAuthMethods = serde_json::from_str(sample_json).unwrap();

        assert_eq!(actual, expected);
    }
}
