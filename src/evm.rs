use crate::internals::{SingleVerifier, ThresholdVerifier, HOT_VERIFY_METHOD_NAME, TIMEOUT};
use crate::{ChainValidationConfig, VerifyArgs};
use anyhow::{Context, Result};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use serde::Serialize;
use serde_json::json;
use std::str::FromStr;
use std::sync::Arc;
use web3::ethabi;
use web3::ethabi::{Contract, ParamType, Token};
use web3::types::{CallRequest, H160};

pub const HOT_VERIFY_EVM_ABI: &str = r#"[{"inputs":[{"internalType":"bytes32","name":"msg_hash","type":"bytes32"},{"internalType":"bytes","name":"walletId","type":"bytes"},{"internalType":"bytes","name":"userPayload","type":"bytes"},{"internalType":"bytes","name":"metadata","type":"bytes"}],"name":"hot_verify","outputs":[{"internalType":"bool","name":"","type":"bool"}],"stateMutability":"view","type":"function"}]"#;
static EVM_CONTRACT_ABI: Lazy<Arc<Contract>> = Lazy::new(|| {
    let contract =
        Contract::load(HOT_VERIFY_EVM_ABI.as_bytes()).expect("Couldn't load evm contract schema");
    Arc::new(contract)
});

#[derive(Serialize)]
struct RpcRequest {
    jsonrpc: String,
    id: String,
    method: String,
    params: serde_json::Value,
}

impl RpcRequest {
    pub fn build(call_request: CallRequest) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: "dontcare".to_string(),
            method: "eth_call".to_string(),
            params: json!([call_request, "finalized"]),
        }
    }
}

#[derive(Clone)]
pub(crate) struct EvmSingleVerifier {
    client: Arc<reqwest::Client>,
    server: String,
    contract: Arc<Contract>,
}

impl EvmSingleVerifier {
    pub fn new(client: Arc<reqwest::Client>, server: String, contract: Arc<Contract>) -> Self {
        Self {
            client,
            server,
            contract,
        }
    }

    async fn call_rpc(&self, json: serde_json::Value) -> Result<String> {
        let response = self
            .client
            .post(&self.server)
            .json(&json)
            .timeout(TIMEOUT)
            .send()
            .await?;

        if response.status().is_success() {
            let value = response.json::<serde_json::Value>().await?;
            let value = value
                .get("result")
                .context(format!("missing result: {}", value))?;
            let value = serde_json::from_value::<String>(value.clone())?;
            Ok(value)
        } else {
            Err(anyhow::anyhow!(
                "Failed to call {}: {}",
                self.server,
                response.status()
            ))
        }
    }
}

#[async_trait]
impl SingleVerifier for EvmSingleVerifier {
    fn get_endpoint(&self) -> String {
        self.server.clone()
    }

    async fn verify(&self, auth_contract_id: &str, args: VerifyArgs) -> Result<bool> {
        let msg_hash = hex::decode(args.msg_hash).context("msg_hash is not valid hex")?;
        let user_payload =
            hex::decode(args.user_payload).context("user_payload is not valid hex")?;

        let data = self
            .contract
            .function(HOT_VERIFY_METHOD_NAME)?
            .encode_input(&[
                Token::FixedBytes(msg_hash),
                Token::Bytes(vec![]),
                Token::Bytes(user_payload),
                Token::Bytes(vec![]),
            ])
            .context("Bad arguments for evm smart contract")?;

        let call_request = CallRequest::builder()
            .to(H160::from_str(auth_contract_id)?)
            .data(data.into())
            .build();

        let rpc_request = RpcRequest::build(call_request);
        let rpc_request = serde_json::to_value(&rpc_request)?;

        let raw = self
            .call_rpc(rpc_request)
            .await?
            .trim_start_matches("0x")
            .to_string();

        let data = hex::decode(raw).context("invalid hex in RPC response")?;

        let tokens = ethabi::decode(&[ParamType::Bool], &data).context("ABI decode failed")?;
        let result = match tokens[0] {
            Token::Bool(b) => b,
            _ => unreachable!(),
        };

        Ok(result)
    }
}

impl ThresholdVerifier<EvmSingleVerifier> {
    pub fn new_evm(validation_config: ChainValidationConfig, client: Arc<reqwest::Client>) -> Self {
        let threshold = validation_config.threshold;
        let servers = validation_config.servers;
        if threshold > servers.len() {
            panic!(
                "There should be at least {} servers, got {}",
                threshold,
                servers.len()
            )
        }
        let verifiers = servers
            .iter()
            .map(|s| {
                let verifier =
                    EvmSingleVerifier::new(client.clone(), s.clone(), EVM_CONTRACT_ABI.clone());
                Arc::new(verifier)
            })
            .collect();
        Self {
            threshold,
            verifiers,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn base_single_verifier() {
        let evm_hot_verify_contract =
            Arc::new(Contract::load(HOT_VERIFY_EVM_ABI.as_bytes()).unwrap());
        let args = VerifyArgs {
            msg_body: "".to_string(),
            wallet_id: None,
            msg_hash: "0000000000000000000000000000000000000000000000000000000000000000".into(),
            metadata: None,
            user_payload: "00000000000000000000000000000000000000000000005dac769be0b6d400000000000000000000000000000000000000000000000000000000000000000000".into(),
        };
        let auth_contract_id = "0xf22Ef29d5Bb80256B569f4233a76EF09Cae996eC";
        let validation = EvmSingleVerifier::new(
            Arc::new(reqwest::Client::new()),
            "https://1rpc.io/base".to_string(),
            evm_hot_verify_contract,
        );

        validation.verify(auth_contract_id, args).await.unwrap();
    }

    #[tokio::test]
    async fn base_single_verifier_non_trivial_message() {
        let evm_hot_verify_contract =
            Arc::new(Contract::load(HOT_VERIFY_EVM_ABI.as_bytes()).unwrap());
        let args = VerifyArgs {
            msg_body: "".to_string(),
            wallet_id: None,
            msg_hash: "ef32edffb454d2a3172fd0af3fdb0e43fac5060a929f1b83b6de2b73754e3f45".into(),
            metadata: None,
            user_payload: "00000000000000000000000000000000000000000000005e095d2c286c4414050000000000000000000000000000000000000000000000000000000000000000".into(),
        };
        let auth_contract_id = "0x42351e68420D16613BBE5A7d8cB337A9969980b4";
        let validation = EvmSingleVerifier::new(
            Arc::new(reqwest::Client::new()),
            "https://1rpc.io/base".to_string(),
            evm_hot_verify_contract,
        );

        validation.verify(auth_contract_id, args).await.unwrap();
    }

    #[tokio::test]
    async fn base_single_verifier_wrong_message() {
        let evm_hot_verify_contract =
            Arc::new(Contract::load(HOT_VERIFY_EVM_ABI.as_bytes()).unwrap());
        let args = VerifyArgs {
            msg_body: "".to_string(),
            wallet_id: None,
            msg_hash: "0000000000012300000000000000000000000000000000000000000000000000".into(),
            metadata: None,
            user_payload: "00000000000000000000000000000000000000000000005dac769be0b6d400000000000000000000000000000000000000000000000000000000000000000000".into(),
        };
        let auth_contract_id = "0xf22Ef29d5Bb80256B569f4233a76EF09Cae996eC";
        let validation = EvmSingleVerifier::new(
            Arc::new(reqwest::Client::new()),
            "https://1rpc.io/base".to_string(),
            evm_hot_verify_contract,
        );

        let result = validation.verify(auth_contract_id, args).await.unwrap();
        assert!(!result);
    }

    #[tokio::test]
    async fn base_threshold_verifier() {
        let args = VerifyArgs {
            msg_body: "".to_string(),
            wallet_id: None,
            msg_hash: "0000000000000000000000000000000000000000000000000000000000000000".into(),
            metadata: None,
            user_payload: "00000000000000000000000000000000000000000000005dac769be0b6d400000000000000000000000000000000000000000000000000000000000000000000".into(),
        };
        let auth_contract_id = "0xf22Ef29d5Bb80256B569f4233a76EF09Cae996eC";

        let validation = ThresholdVerifier::new_evm(
            ChainValidationConfig {
                threshold: 1,
                servers: vec![
                    "http://localhost:8545".to_string(),
                    "https://1rpc.io/base".to_string(),
                    "http://localhost:8545".to_string(),
                ],
            },
            Arc::new(reqwest::Client::new()),
        );

        validation.verify(auth_contract_id, args).await.unwrap();
    }

    #[tokio::test]
    async fn base_threshold_verifier_with_bad_rpcs() {
        let args = VerifyArgs {
            msg_body: "".to_string(),
            wallet_id: None,
            msg_hash: "0000000000000000000000000000000000000000000000000000000000000000".into(),
            metadata: None,
            user_payload: "00000000000000000000000000000000000000000000005dac769be0b6d400000000000000000000000000000000000000000000000000000000000000000000".into(),
        };
        let auth_contract_id = "0xf22Ef29d5Bb80256B569f4233a76EF09Cae996eC";

        let validation = ThresholdVerifier::new_evm(
            ChainValidationConfig {
                threshold: 1,
                servers: vec![
                    "http://localhost:1000".to_string(),
                    "http://localhost:1000".to_string(),
                    "http://localhost:1000".to_string(),
                    "http://localhost:1000".to_string(),
                    "http://localhost:8545".to_string(),
                    "http://localhost:1000".to_string(),
                    "https://1rpc.io/base".to_string(),
                    "http://localhost:1000".to_string(),
                ],
            },
            Arc::new(reqwest::Client::new()),
        );

        validation.verify(auth_contract_id, args).await.unwrap();
    }

    #[should_panic]
    #[tokio::test]
    async fn base_threshold_verifier_all_rpcs_bad() {
        let args = VerifyArgs {
            msg_body: "".to_string(),
            wallet_id: None,
            msg_hash: "0000000000000000000000000000000000000000000000000000000000000000".into(),
            metadata: None,
            user_payload: "00000000000000000000000000000000000000000000005dac769be0b6d400000000000000000000000000000000000000000000000000000000000000000000".into(),
        };
        let auth_contract_id = "0xf22Ef29d5Bb80256B569f4233a76EF09Cae996eC";

        let validation = ThresholdVerifier::new_evm(
            ChainValidationConfig {
                threshold: 1,
                servers: vec![
                    "http://localhost:1000".to_string(),
                    "http://localhost:1000".to_string(),
                    "http://localhost:1000".to_string(),
                    "http://localhost:1000".to_string(),
                    "http://localhost:1000".to_string(),
                    "http://localhost:1000".to_string(),
                ],
            },
            Arc::new(reqwest::Client::new()),
        );

        validation.verify(auth_contract_id, args).await.unwrap();
    }
}
