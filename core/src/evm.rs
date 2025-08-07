use crate::internals::{SingleVerifier, ThresholdVerifier, TIMEOUT};
use crate::ChainValidationConfig;
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::future::BoxFuture;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_hex::SerHexSeq;
use serde_hex::StrictPfx;
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

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Hash, Clone)]
#[serde(tag = "type", content = "value")]
pub enum EvmInputArg {
    #[serde(rename = "bytes32")]
    #[serde(with = "SerHexSeq::<StrictPfx>")]
    FixedBytes(Vec<u8>),
    #[serde(rename = "bytes")]
    #[serde(with = "SerHexSeq::<StrictPfx>")]
    Bytes(Vec<u8>),
}

impl From<EvmInputArg> for Token {
    fn from(value: EvmInputArg) -> Self {
        match value {
            EvmInputArg::FixedBytes(data) => Token::FixedBytes(data),
            EvmInputArg::Bytes(data) => Token::Bytes(data),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Hash, Clone)]
pub struct EvmInputData(pub Vec<EvmInputArg>);

impl EvmInputData {
    pub fn from_parts(message_hex: String, user_payload: String) -> Result<Self> {
        let result = EvmInputData(vec![
            EvmInputArg::FixedBytes(hex::decode(message_hex)?),
            EvmInputArg::Bytes(vec![]),
            EvmInputArg::Bytes(hex::decode(user_payload)?),
            EvmInputArg::Bytes(vec![]),
        ]);
        Ok(result)
    }
}

impl From<EvmInputData> for Vec<Token> {
    fn from(value: EvmInputData) -> Self {
        value.0.into_iter().map(|v| v.into()).collect()
    }
}

impl RpcRequest {
    pub fn build(call_request: CallRequest) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: "dontcare".to_string(),
            method: "eth_call".to_string(),
            params: json!([call_request, "latest"]),
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

    async fn verify(
        &self,
        auth_contract_id: &str,
        method_name: String,
        input: EvmInputData,
    ) -> Result<bool> {
        let input: Vec<Token> = From::from(input);
        let data = self
            .contract
            .function(&method_name)?
            .encode_input(&input)
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

#[async_trait]
impl SingleVerifier for EvmSingleVerifier {
    fn get_endpoint(&self) -> String {
        self.server.clone()
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

    pub async fn verify(
        &self,
        auth_contract_id: &str,
        method_name: &str,
        input: EvmInputData,
    ) -> Result<bool> {
        let auth_contract_id = Arc::new(auth_contract_id.to_string());
        let functor = move |verifier: Arc<EvmSingleVerifier>| -> BoxFuture<'static, Option<bool>> {
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
    use crate::evm::{EvmInputData, EvmSingleVerifier, HOT_VERIFY_EVM_ABI};
    use crate::internals::{ThresholdVerifier, HOT_VERIFY_METHOD_NAME};
    use crate::{ChainValidationConfig, HotVerifyAuthCall};
    use anyhow::Result;
    use std::sync::Arc;
    use web3::ethabi::Contract;

    #[tokio::test]
    async fn base_single_verifier() -> Result<()> {
        let evm_hot_verify_contract = Arc::new(Contract::load(HOT_VERIFY_EVM_ABI.as_bytes())?);
        let msg_hash =
            "0000000000000000000000000000000000000000000000000000000000000000".to_string();
        let user_payload = "00000000000000000000000000000000000000000000005dac769be0b6d400000000000000000000000000000000000000000000000000000000000000000000".to_string();
        let auth_contract_id = "0xf22Ef29d5Bb80256B569f4233a76EF09Cae996eC";

        let validation = EvmSingleVerifier::new(
            Arc::new(reqwest::Client::new()),
            "https://1rpc.io/base".to_string(),
            evm_hot_verify_contract,
        );

        validation
            .verify(
                auth_contract_id,
                HOT_VERIFY_METHOD_NAME.to_string(),
                EvmInputData::from_parts(msg_hash, user_payload)?,
            )
            .await?;

        Ok(())
    }

    #[tokio::test]
    async fn base_single_verifier_non_trivial_message() -> Result<()> {
        let evm_hot_verify_contract =
            Arc::new(Contract::load(HOT_VERIFY_EVM_ABI.as_bytes()).unwrap());

        let msg_hash =
            "ef32edffb454d2a3172fd0af3fdb0e43fac5060a929f1b83b6de2b73754e3f45".to_string();
        let user_payload = "00000000000000000000000000000000000000000000005e095d2c286c4414050000000000000000000000000000000000000000000000000000000000000000".to_string();
        let auth_contract_id = "0x42351e68420D16613BBE5A7d8cB337A9969980b4";

        let validation = EvmSingleVerifier::new(
            Arc::new(reqwest::Client::new()),
            "https://1rpc.io/base".to_string(),
            evm_hot_verify_contract,
        );

        validation
            .verify(
                auth_contract_id,
                HOT_VERIFY_METHOD_NAME.to_string(),
                EvmInputData::from_parts(msg_hash, user_payload)?,
            )
            .await?;

        Ok(())
    }

    #[tokio::test]
    async fn base_single_verifier_wrong_message() -> Result<()> {
        let evm_hot_verify_contract =
            Arc::new(Contract::load(HOT_VERIFY_EVM_ABI.as_bytes()).unwrap());

        let msg_hash =
            "cf32edffb454d2a3172fd0af3fdb0e43fac5060a929f1b83b6de2b73754e3f45".to_string();
        let user_payload = "00000000000000000000000000000000000000000000005e095d2c286c4414050000000000000000000000000000000000000000000000000000000000000000".to_string();
        let auth_contract_id = "0x42351e68420D16613BBE5A7d8cB337A9969980b4";

        let validation = EvmSingleVerifier::new(
            Arc::new(reqwest::Client::new()),
            "https://1rpc.io/base".to_string(),
            evm_hot_verify_contract,
        );

        let status = validation
            .verify(
                auth_contract_id,
                HOT_VERIFY_METHOD_NAME.to_string(),
                EvmInputData::from_parts(msg_hash, user_payload)?,
            )
            .await;

        assert!(status.is_err());

        Ok(())
    }

    #[tokio::test]
    async fn base_threshold_verifier() -> Result<()> {
        let msg_hash =
            "0000000000000000000000000000000000000000000000000000000000000000".to_string();
        let user_payload = "00000000000000000000000000000000000000000000005dac769be0b6d400000000000000000000000000000000000000000000000000000000000000000000".to_string();
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

        validation
            .verify(
                auth_contract_id,
                HOT_VERIFY_METHOD_NAME,
                EvmInputData::from_parts(msg_hash, user_payload)?,
            )
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn base_threshold_verifier_with_bad_rpcs() -> Result<()> {
        let msg_hash =
            "0000000000000000000000000000000000000000000000000000000000000000".to_string();
        let user_payload = "00000000000000000000000000000000000000000000005dac769be0b6d400000000000000000000000000000000000000000000000000000000000000000000".to_string();
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

        validation
            .verify(
                auth_contract_id,
                HOT_VERIFY_METHOD_NAME,
                EvmInputData::from_parts(msg_hash, user_payload)?,
            )
            .await?;
        Ok(())
    }

    #[should_panic]
    #[tokio::test]
    async fn base_threshold_verifier_all_rpcs_bad() {
        let msg_hash =
            "0000000000000000000000000000000000000000000000000000000000000000".to_string();
        let user_payload = "00000000000000000000000000000000000000000000005dac769be0b6d400000000000000000000000000000000000000000000000000000000000000000000".to_string();
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

        validation
            .verify(
                auth_contract_id,
                HOT_VERIFY_METHOD_NAME,
                EvmInputData::from_parts(msg_hash, user_payload).unwrap(),
            )
            .await
            .unwrap();
    }

    #[test]
    fn check_evm_bridge_validation_format() {
        let x = r#"{
        "chain_id": 56,
        "contract_id": "0x233c5370CCfb3cD7409d9A3fb98ab94dE94Cb4Cd",
        "input": [
         {
           "type": "bytes32",
           "value": "0x74657374"
         },
         {
           "type": "bytes",
           "value": "0x5075766b334752376276426d4a71673253647a73344432414647415733725871396977704a7261426b474a"
         },
         {
           "type": "bytes",
           "value": "0x000000000000000000000000000000000000000000000000000000000001d97c00"
         },
         {
           "type": "bytes",
           "value": "0x"
         }
        ],
        "method": "hot_verify"
        }"#
            .to_string();
        serde_json::from_str::<HotVerifyAuthCall>(&x).unwrap();
    }

    #[tokio::test]
    async fn test_bridge_validation() -> Result<()> {
        let evm_hot_verify_contract = Arc::new(Contract::load(HOT_VERIFY_EVM_ABI.as_bytes())?);
        let msg_hash =
            "c4ea3c95f2171df3fa5a6f8452d1bbbbd0608abe68fdcea7f25a04516c50cba6".to_string();
        let user_payload =
            "00000000000000000000000000000000000000000000005f1ba235abe1a5f37e00".to_string();
        let auth_contract_id = "0x233c5370CCfb3cD7409d9A3fb98ab94dE94Cb4Cd";

        let validation = EvmSingleVerifier::new(
            Arc::new(reqwest::Client::new()),
            "https://bsc.drpc.org".to_string(),
            evm_hot_verify_contract,
        );

        validation
            .verify(
                auth_contract_id,
                HOT_VERIFY_METHOD_NAME.to_string(),
                EvmInputData::from_parts(msg_hash, user_payload)?,
            )
            .await?;

        Ok(())
    }
}
