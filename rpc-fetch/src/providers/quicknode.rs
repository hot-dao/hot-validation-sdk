use crate::providers::{Provider, SlugFromChainId};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use hot_validation_primitives::ExtendedChainId;
use serde::Deserialize;
use std::collections::HashMap;
use strum::IntoEnumIterator;

pub struct QuicknodeProvider {
    api_key: String,
}

impl QuicknodeProvider {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

impl SlugFromChainId for QuicknodeProvider {
    fn slug(chain_id: ExtendedChainId) -> Option<String> {
        match chain_id {
            ExtendedChainId::Near => Some("near-mainnet".to_string()),
            ExtendedChainId::Optimism => Some("optimism".to_string()),
            ExtendedChainId::Bsc => Some("bsc".to_string()),
            ExtendedChainId::Polygon => Some("matic".to_string()),
            ExtendedChainId::MonadTestnet => Some("monad-testnet".to_string()),
            ExtendedChainId::ZkSync => Some("zksync-mainnet".to_string()),
            ExtendedChainId::Stellar => Some("stellar-mainnet".to_string()),
            ExtendedChainId::Base => Some("base-mainnet".to_string()),
            ExtendedChainId::Arbitrum => Some("arbitrum-mainnet".to_string()),
            ExtendedChainId::Avax => Some("avalanche-mainnet".to_string()),
            ExtendedChainId::BeraChain => Some("bera-mainnet".to_string()),
            ExtendedChainId::Scroll => Some("scroll-mainnet".to_string()),
            ExtendedChainId::Ton => Some("ton-mainnet".to_string()),
            ExtendedChainId::Solana => Some("solana-mainnet".to_string()),

            ExtendedChainId::Eth | // has to return base endpoint
            ExtendedChainId::Kava |
            ExtendedChainId::Aurora => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct QuicknodeEndpointsResp {
    data: Vec<QuicknodeEndpoint>,
}

#[derive(Debug, Deserialize)]
struct QuicknodeEndpoint {
    chain: String,
    network: String,
    http_url: String,
}

impl QuicknodeProvider {
    async fn fetch_all_endpoints(&self) -> Result<Vec<QuicknodeEndpoint>> {
        let http = reqwest::Client::new();
        let resp = http
            .get("https://api.quicknode.com/v0/endpoints")
            .header("accept", "application/json")
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .context("request to quicknode /v0/endpoints failed")?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "quicknode /v0/endpoints returned HTTP {}",
                resp.status()
            ));
        }

        let parsed: QuicknodeEndpointsResp = resp
            .json()
            .await
            .context("failed to parse quicknode endpoints response")?;
        Ok(parsed.data)
    }

    async fn get_eth_mainnet_endpoint(&self) -> Result<String> {
        let eps = self.fetch_all_endpoints().await?;
        let base = eps
            .into_iter()
            .find(|e| e.chain == "eth" && e.network == "mainnet");
        match base {
            Some(e) => Ok(e.http_url),
            None => Err(anyhow!(
                "ETH mainnet endpoint not found in QuickNode account"
            )),
        }
    }
}

#[async_trait]
impl Provider for QuicknodeProvider {
    async fn fetch_endpoints(&self) -> Result<HashMap<ExtendedChainId, String>> {
        let base_ep = self.get_eth_mainnet_endpoint().await?;
        let mut map = HashMap::new();

        for chain in ExtendedChainId::iter() {
            if matches!(chain, ExtendedChainId::Eth) {
                map.insert(chain, base_ep.clone());
            } else if let Some(slug) = Self::slug(chain) {
                let mut ep = base_ep.replace(".quiknode.pro", &format!(".{slug}.quiknode.pro"));
                if matches!(chain, ExtendedChainId::Avax) {
                    ep += "ext/bc/C/rpc";
                } else if matches!(chain, ExtendedChainId::Ton) {
                    ep += "jsonRPC";
                }
                map.insert(chain, ep.clone());
            }
        }

        Ok(map)
    }
}
