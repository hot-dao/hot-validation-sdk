use crate::internals::{SingleVerifier, ThresholdVerifier, TIMEOUT};
use crate::ChainValidationConfig;
use alloy_contract::Interface;
use alloy_dyn_abi::DynSolValue;
use alloy_json_abi::JsonAbi;
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::future::BoxFuture;
use hot_validation_primitives::bridge::evm::EvmInputData;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

// JSON ABI for hot_verify
pub const HOT_VERIFY_EVM_ABI: &str = r#"
[
  {
    "inputs": [
      { "internalType": "bytes32", "name": "msg_hash",    "type": "bytes32" },
      { "internalType": "bytes",   "name": "walletId",    "type": "bytes"   },
      { "internalType": "bytes",   "name": "userPayload", "type": "bytes"   },
      { "internalType": "bytes",   "name": "metadata",    "type": "bytes"   }
    ],
    "name": "hot_verify",
    "outputs": [
      { "internalType": "bool", "name": "", "type": "bool" }
    ],
    "stateMutability": "view",
    "type": "function"
  },
  {
    "inputs": [
      { "internalType": "uint128", "name": "", "type": "uint128" }
    ],
    "name": "usedNonces",
    "outputs": [
      { "internalType": "bool", "name": "", "type": "bool" }
    ],
    "stateMutability": "view",
    "type": "function"
  }
]
"#;

// Initialize the Interface once
static INTERFACE: Lazy<Interface> = Lazy::new(|| {
    let abi: JsonAbi =
        serde_json::from_str(HOT_VERIFY_EVM_ABI).expect("Invalid JSON ABI for hot_verify");
    Interface::new(abi)
});

#[derive(Serialize, Deserialize)]
struct RpcRequest {
    jsonrpc: String,
    id: String,
    method: String,
    params: Value,
}

impl RpcRequest {
    pub fn build(call_obj: Value) -> Self {
        RpcRequest {
            jsonrpc: "2.0".into(),
            id: "dontcare".into(),
            method: "eth_call".into(),
            params: json!([call_obj, "latest"]),
        }
    }
}

#[derive(Clone)]
pub(crate) struct EvmSingleVerifier {
    client: Arc<reqwest::Client>,
    server: String,
}

impl EvmSingleVerifier {
    pub fn new(client: Arc<reqwest::Client>, server: String) -> Self {
        Self { client, server }
    }

    async fn call_rpc(&self, rpc: &RpcRequest) -> Result<String> {
        let resp = self
            .client
            .post(&self.server)
            .json(rpc)
            .timeout(TIMEOUT)
            .send()
            .await?;

        if resp.status().is_success() {
            let v: Value = resp.json().await?;
            let result = v
                .get("result")
                .context(format!("missing result: {:?}", v))?;
            serde_json::from_value(result.clone())
                .context("Failed to parse RPC result as hex string")
        } else {
            Err(anyhow::anyhow!(
                "RPC error {}: {}",
                self.server,
                resp.status()
            ))
        }
    }

    async fn verify(
        &self,
        auth_contract_id: &str,
        method_name: String,
        input: EvmInputData,
    ) -> Result<bool> {
        let args: Vec<DynSolValue> = From::from(input);

        let data = INTERFACE.encode_input(&method_name, &args)?;
        let data_hex = format!("0x{}", hex::encode(data));

        // Build and send RPC request
        let call_obj = json!({"to": auth_contract_id, "data": data_hex});
        let rpc = RpcRequest::build(call_obj);
        let raw = self.call_rpc(&rpc).await?;
        let bytes = hex::decode(raw.trim_start_matches("0x"))?;

        // Decode output
        let out = INTERFACE.decode_output("hot_verify", &bytes)?;
        if let Some(DynSolValue::Bool(b)) = out.first() {
            Ok(*b)
        } else {
            Err(anyhow::anyhow!("Unexpected output type"))
        }
    }
}

#[async_trait]
impl SingleVerifier for EvmSingleVerifier {
    fn get_endpoint(&self) -> String {
        self.server.clone()
    }
}

impl ThresholdVerifier<EvmSingleVerifier> {
    pub fn new_evm(config: ChainValidationConfig, client: Arc<reqwest::Client>) -> Self {
        let threshold = config.threshold;
        let servers = config.servers;
        if threshold > servers.len() {
            panic!("Threshold {} > servers {}", threshold, servers.len());
        }
        let verifiers = servers
            .into_iter()
            .map(|url| Arc::new(EvmSingleVerifier::new(client.clone(), url)))
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
        let functor = move |verifier: Arc<EvmSingleVerifier>| -> BoxFuture<'static, Result<bool>> {
            let auth = auth_contract_id.clone();
            let method_name = method_name.to_string();
            Box::pin(async move {
                verifier
                    .verify(&auth, method_name, input)
                    .await
                    .context(format!(
                        "Error calling evm `verify` with {}",
                        verifier.sanitized_endpoint()
                    ))
            })
        };
        self.threshold_call(functor).await
    }
}

#[cfg(test)]
mod tests {
    use crate::evm::EvmInputData;
    use crate::internals::{ThresholdVerifier, HOT_VERIFY_METHOD_NAME};
    use crate::ChainValidationConfig;
    use anyhow::Result;
    use hot_validation_primitives::bridge::HotVerifyAuthCall;
    use std::sync::Arc;

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
}
