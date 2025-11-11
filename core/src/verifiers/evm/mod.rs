mod types;

use crate::verifiers::evm::types::{BlockSpecifier, RpcRequest, RpcResponse, BLOCK_DELAY};
use crate::threshold_verifier::ThresholdVerifier;
use crate::{ChainValidationConfig, Validation, HOT_VERIFY_METHOD_NAME};
use alloy_contract::Interface;
use alloy_dyn_abi::DynSolValue;
use alloy_json_abi::JsonAbi;
use anyhow::{Context, Result};
use futures_util::future::BoxFuture;
use hot_validation_primitives::bridge::evm::EvmInputData;
use hot_validation_primitives::{ChainId, ExtendedChainId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::http_client::{post_json_receive_json, TIMEOUT};

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

    async fn get_block(&self) -> Result<BlockSpecifier> {
        let can_reorg = ExtendedChainId::try_from(self.chain_id)
            .map_err(anyhow::Error::msg)?
            .can_reorg();
        if !can_reorg {
            return Ok(BlockSpecifier::Latest)
        }
        let request = RpcRequest::build_block_number();
        let response: RpcResponse = post_json_receive_json(
            &self.client,
            &self.server,
            &request,
            self.chain_id,
        ).await?;
        let block_number = response.as_u64()?;

        // Ideally, we would want to use `safe` or `final` block here,
        // but some networks have too much finality time (i.e. 15 minutes). So we use `latest - 1`,
        // because in practice most reverts happen in the next block,
        // so taking some delta from the latest block is good enough.
        let safer_block_number = block_number - BLOCK_DELAY;
        Ok(BlockSpecifier::BlockNumber(safer_block_number))
    }

    async fn verify(
        &self,
        auth_contract_id: &str,
        method_name: &str,
        input: EvmInputData,
    ) -> Result<bool> {
        let args: Vec<DynSolValue> = From::from(input);
        let block_specifier = self.get_block().await?;
        let request = RpcRequest::build_eth_call(
            &auth_contract_id,
            &method_name,
            &args,
            &block_specifier,
        )?;
        let response: RpcResponse = post_json_receive_json(
            &self.client,
            &self.server,
            &request,
            self.chain_id
        ).await?;
        let status = response.as_bool()?;
        Ok(status)
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
                        "Error calling evm `verify` with", // TODO
                    ))
            })
        };
        self.threshold_call(functor).await
    }
}

impl Validation {
    pub(crate) async fn handle_evm(
        self: Arc<Self>,
        chain_id: ChainId,
        auth_contract_id: &str,
        method_name: &str,
        input: EvmInputData,
    ) -> Result<bool> {
        let validation = self.evm.get(&chain_id).ok_or(anyhow::anyhow!(
            "EVM validation is not configured for chain {chain_id:?}"
        ))?;
        let status = validation
            .verify(auth_contract_id, method_name, input)
            .await?;
        Ok(status)
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::base_rpc;
    use crate::threshold_verifier::ThresholdVerifier;
    use crate::{ChainValidationConfig, HOT_VERIFY_METHOD_NAME};
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

        let status = validation
            .verify(
                auth_contract_id,
                HOT_VERIFY_METHOD_NAME,
                EvmInputData::from_parts(msg_hash, user_payload)?,
            )
            .await?;
        assert!(status);
        Ok(())
    }
}
