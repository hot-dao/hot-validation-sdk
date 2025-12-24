use crate::http_client::get_json;
use crate::threshold_verifier::{Identifiable, ThresholdVerifier};
use crate::verifiers::Verifier;
use async_trait::async_trait;
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use hot_validation_primitives::bridge::cosmos::CosmosInputData;
use hot_validation_primitives::bridge::InputData;
use hot_validation_primitives::{ChainId, ChainValidationConfig, ExtendedChainId};
use serde::Deserialize;
use std::sync::Arc;

pub struct CosmosVerifier {
    client: Arc<reqwest::Client>,
    server: String,
    chain_id: ChainId,
}

impl CosmosVerifier {
    pub fn new(client: Arc<reqwest::Client>, server: String, chain_id: ChainId) -> Self {
        Self {
            client,
            server,
            chain_id,
        }
    }
}

impl Identifiable for CosmosVerifier {
    fn id(&self) -> String {
        self.server.clone()
    }
}

#[async_trait]
impl Verifier for CosmosVerifier {
    fn chain_id(&self) -> ExtendedChainId {
        self.chain_id
            .try_into()
            .expect("Couldn't convert ChainId to ExtendedChainId")
    }

    async fn verify(
        &self,
        auth_contract_id: String,
        _method_name: String, // method_name is being encoded in the input_data
        input_data: InputData,
    ) -> anyhow::Result<bool> {
        #[derive(Deserialize)]
        struct Response {
            data: bool,
        }
        let input: CosmosInputData = input_data.try_into()?;
        let b64 = BASE64_STANDARD.encode(&serde_json::to_vec(&input)?);
        let url = format!(
            "{}/cosmwasm/wasm/v1/contract/{}/smart/{}",
            self.server, auth_contract_id, b64
        );
        let response: Response = get_json(&self.client, &url, self.chain_id).await?;
        Ok(response.data)
    }
}

impl ThresholdVerifier<CosmosVerifier> {
    pub fn new_cosmos(
        config: ChainValidationConfig,
        client: &Arc<reqwest::Client>,
        chain_id: ChainId,
    ) -> Self {
        let threshold = config.threshold;
        let servers = config.servers;
        let verifiers = servers
            .into_iter()
            .map(|url| Arc::new(CosmosVerifier::new(client.clone(), url, chain_id)))
            .collect();
        Self {
            threshold,
            verifiers,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::verifiers::cosmos::CosmosVerifier;
    use crate::verifiers::Verifier;
    use anyhow::Result;
    use base64::prelude::BASE64_STANDARD;
    use base64::Engine;
    use hot_validation_primitives::bridge::cosmos::CosmosInputData;
    use hot_validation_primitives::bridge::InputData;
    use hot_validation_primitives::ChainId;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_hot_verify() -> Result<()> {
        let api = "https://juno-api.stakeandrelax.net";
        let verifier = CosmosVerifier::new(
            Arc::new(reqwest::Client::new()),
            api.to_string(),
            ChainId::Evm(4444_118),
        );

        {
            let address =
                "juno1va9q7gma6l62aqq988gghv4r7u4hnlgm85ssmsdf9ypw77qfwa0qaz7ea4".to_string();
            let msg_hash_b64 = "utaIqDt2xuY7c2V+b2JU1B+I5dJ10EbaFzvmLpjpx+U=";
            let nonce = 1764175051000000000008;
            let input_data = InputData::Cosmos(CosmosInputData::HotVerify {
                nonce,
                msg_hash: BASE64_STANDARD.decode(msg_hash_b64)?.try_into().unwrap(),
            });

            let x = verifier.verify(address, String::new(), input_data).await?;
            assert!(x);
        }
        {
            // wrong addr
            let address =
                "juno1va9q7gma6l62aqq988gghv4r7u4hnlgm85ssmsdf9ypw77qfwa0qaz7ea5".to_string();
            let msg_hash_b64 = "utaIqDt2xuY7c2V+b2JU1B+I5dJ10EbaFzvmLpjpx+U=";
            let nonce = 1764175051000000000008;
            let input_data = InputData::Cosmos(CosmosInputData::HotVerify {
                nonce,
                msg_hash: BASE64_STANDARD.decode(msg_hash_b64)?.try_into().unwrap(),
            });

            assert!(verifier
                .verify(address, String::new(), input_data,)
                .await
                .is_err());
        }
        {
            // wrong msg hash
            let address =
                "juno1va9q7gma6l62aqq988gghv4r7u4hnlgm85ssmsdf9ypw77qfwa0qaz7ea4".to_string();
            let msg_hash_b64 = "ftaIqDt2xuY7c2V+b2JU1B+I5dJ10EbaFzvmLpjpx+U=";
            let nonce = 1764175051000000000008;
            let input_data = InputData::Cosmos(CosmosInputData::HotVerify {
                nonce,
                msg_hash: BASE64_STANDARD.decode(msg_hash_b64)?.try_into().unwrap(),
            });

            assert!(!verifier.verify(address, String::new(), input_data,).await?);
        }
        {
            // wrong nonce
            let address =
                "juno1va9q7gma6l62aqq988gghv4r7u4hnlgm85ssmsdf9ypw77qfwa0qaz7ea4".to_string();
            let msg_hash_b64 = "utaIqDt2xuY7c2V+b2JU1B+I5dJ10EbaFzvmLpjpx+U=";
            let nonce = 1764175051000000000009;
            let input_data = InputData::Cosmos(CosmosInputData::HotVerify {
                nonce,
                msg_hash: BASE64_STANDARD.decode(msg_hash_b64)?.try_into().unwrap(),
            });

            assert!(verifier
                .verify(address, String::new(), input_data,)
                .await
                .is_err());
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_is_executed() -> Result<()> {
        let api = "https://juno-api.stakeandrelax.net";
        let verifier = CosmosVerifier::new(
            Arc::new(reqwest::Client::new()),
            api.to_string(),
            ChainId::Evm(4444_118),
        );

        {
            let address =
                "juno1va9q7gma6l62aqq988gghv4r7u4hnlgm85ssmsdf9ypw77qfwa0qaz7ea4".to_string();
            let input_data = InputData::Cosmos(CosmosInputData::IsExecuted {
                nonce: 1764027631000000481371,
            });

            let x = verifier.verify(address, String::new(), input_data).await?;
            assert!(x);
        }
        {
            let address =
                "juno1va9q7gma6l62aqq988gghv4r7u4hnlgm85ssmsdf9ypw77qfwa0qaz7ea4".to_string();
            let input_data = InputData::Cosmos(CosmosInputData::IsExecuted {
                nonce: 2764027631000000481371,
            });
            let x = verifier.verify(address, String::new(), input_data).await?;
            assert!(!x);
        }

        Ok(())
    }
}
