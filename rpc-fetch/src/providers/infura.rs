use crate::providers::Provider;
use crate::supported_chains::{ChainId, SlugFromChainId};
use async_trait::async_trait;
use std::collections::HashMap;
use strum::IntoEnumIterator;

pub struct InfuraProvider {
    api_key: String,
}

impl InfuraProvider {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

impl SlugFromChainId for InfuraProvider {
    fn slug(chain_id: ChainId) -> Option<String> {
        match chain_id {
            ChainId::Eth => Some("mainnet".to_string()),
            ChainId::Optimism => Some("optimism-mainnet".to_string()),
            ChainId::Bsc => Some("bsc-mainnet".to_string()),
            ChainId::Polygon => Some("polygon-mainnet".to_string()),
            ChainId::ZkSync => Some("zksync-mainnet".to_string()),
            ChainId::Base => Some("base-mainnet".to_string()),
            ChainId::Arbitrum => Some("arbitrum-mainnet".to_string()),
            ChainId::Avax => Some("avalanche-mainnet".to_string()),
            ChainId::Scroll => Some("scroll-mainnet".to_string()),

            ChainId::Near
            | ChainId::MonadTestnet
            | ChainId::Stellar
            | ChainId::Kava
            | ChainId::BeraChain
            | ChainId::Aurora
            | ChainId::Ton => None,
        }
    }
}

#[async_trait]
impl Provider for InfuraProvider {
    async fn fetch_endpoints(&self) -> anyhow::Result<HashMap<ChainId, String>> {
        let mut endpoints = HashMap::new();
        for chain_id in ChainId::iter() {
            if let Some(slug) = Self::slug(chain_id) {
                let url = format!("https://{}.infura.io/v3/{}", slug, &self.api_key);
                endpoints.insert(chain_id, url);
            }
        }
        Ok(endpoints)
    }
}
