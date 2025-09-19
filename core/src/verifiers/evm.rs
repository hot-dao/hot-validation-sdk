use crate::internals::{ThresholdVerifier, TIMEOUT};
use crate::metrics::{tick_metrics_verify_success_attempts, tick_metrics_verify_total_attempts};
use crate::verifiers::Verifier;
use crate::ChainValidationConfig;
use alloy_contract::Interface;
use alloy_dyn_abi::DynSolValue;
use alloy_json_abi::JsonAbi;
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::future::BoxFuture;
use hot_validation_primitives::bridge::evm::EvmInputData;
use hot_validation_primitives::ChainId;
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
static INTERFACE: std::sync::LazyLock<Interface> = std::sync::LazyLock::new(|| {
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
    pub fn build_block_number() -> Self {
        RpcRequest {
            jsonrpc: "2.0".into(),
            id: "dontcare".into(),
            method: "eth_blockNumber".into(),
            params: json!([]),
        }
    }

    pub fn build_eth_call(call_obj: &Value, block_number: u64) -> Self {
        RpcRequest {
            jsonrpc: "2.0".into(),
            id: "dontcare".into(),
            method: "eth_call".into(),
            params: json!([call_obj, format!("0x{:x}", block_number)]),
        }
    }
}

#[derive(Clone)]
pub(crate) struct EvmVerifier {
    client: Arc<reqwest::Client>,
    server: String,
    chain_id: ChainId,
}

impl EvmVerifier {
    pub fn new(client: Arc<reqwest::Client>, server: String, chain_id: ChainId) -> Self {
        Self {
            client,
            server,
            chain_id,
        }
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
            let result = v.get("result").context(format!("missing result: {v:?}"))?;
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

    async fn get_block_number(&self) -> Result<u64> {
        let rpc = RpcRequest::build_block_number();
        let raw = self.call_rpc(&rpc).await?;
        let block_number = u64::from_str_radix(raw.trim_start_matches("0x"), 16)?;
        Ok(block_number)
    }

    async fn verify(
        &self,
        auth_contract_id: &str,
        method_name: &str,
        input: EvmInputData,
    ) -> Result<bool> {
        tick_metrics_verify_total_attempts(self.chain_id);
        let block_number = self.get_block_number().await?;

        let args: Vec<DynSolValue> = From::from(input);

        let data = INTERFACE.encode_input(method_name, &args)?;
        let data_hex = format!("0x{}", hex::encode(data));

        // Build and send RPC request
        let call_obj = json!({"to": auth_contract_id, "data": data_hex});

        // Ideally, we would want to use `safe` or `final` block here,
        // but some networks have too much finality time (i.e. 15 minutes). So we use `latest - 2`,
        // because in practice most reverts happen in the next block,
        // so taking some delta from the latest block is good enough.
        let actual_block_number = block_number.checked_sub(2).expect("block number underflow");

        let rpc = RpcRequest::build_eth_call(&call_obj, actual_block_number);
        let raw = self.call_rpc(&rpc).await?;
        let bytes = hex::decode(raw.trim_start_matches("0x"))?;

        // Decode output
        let out = INTERFACE.decode_output("hot_verify", &bytes)?;
        if let Some(DynSolValue::Bool(b)) = out.first() {
            // TODO: replace checks with `ensure` and do return without conditions
            tick_metrics_verify_success_attempts(self.chain_id);
            Ok(*b)
        } else {
            Err(anyhow::anyhow!("Unexpected output type"))
        }
    }
}

#[async_trait]
impl Verifier for EvmVerifier {
    fn get_endpoint(&self) -> String {
        self.server.clone()
    }
}

impl ThresholdVerifier<EvmVerifier> {
    pub fn new_evm(
        config: ChainValidationConfig,
        client: &Arc<reqwest::Client>,
        chain_id: ChainId,
    ) -> Self {
        let threshold = config.threshold;
        let servers = config.servers;
        assert!(
            (threshold <= servers.len()),
            "Threshold {} > servers {}",
            threshold,
            servers.len()
        );
        let verifiers = servers
            .into_iter()
            .map(|url| Arc::new(EvmVerifier::new(client.clone(), url, chain_id)))
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
        let functor = move |verifier: Arc<EvmVerifier>| -> BoxFuture<'static, Result<bool>> {
            let auth = auth_contract_id.clone();
            let method_name = method_name.to_string();
            Box::pin(async move {
                verifier
                    .verify(&auth, &method_name, input)
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
    use crate::internals::{ThresholdVerifier, HOT_VERIFY_METHOD_NAME};
    use crate::tests::base_rpc;
    use crate::ChainValidationConfig;
    use anyhow::Result;
    use hot_validation_primitives::bridge::evm::EvmInputData;
    use hot_validation_primitives::ChainId;
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
                    base_rpc(),
                ],
            },
            &Arc::new(reqwest::Client::new()),
            ChainId::Evm(8453),
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
}
