use crate::providers::{Provider, SlugFromChainId};
use async_trait::async_trait;
use hot_validation_primitives::ExtendedChainId;
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
    fn slug(chain_id: ExtendedChainId) -> Option<String> {
        match chain_id {
            ExtendedChainId::Eth => Some("mainnet".to_string()),
            ExtendedChainId::Optimism => Some("optimism-mainnet".to_string()),
            ExtendedChainId::Bsc => Some("bsc-mainnet".to_string()),
            ExtendedChainId::Polygon => Some("polygon-mainnet".to_string()),
            ExtendedChainId::ZkSync => Some("zksync-mainnet".to_string()),
            ExtendedChainId::Base => Some("base-mainnet".to_string()),
            ExtendedChainId::Arbitrum => Some("arbitrum-mainnet".to_string()),
            ExtendedChainId::Avax => Some("avalanche-mainnet".to_string()),
            ExtendedChainId::Scroll => Some("scroll-mainnet".to_string()),

            ExtendedChainId::Near
            | ExtendedChainId::MonadTestnet
            | ExtendedChainId::Stellar
            | ExtendedChainId::Kava
            | ExtendedChainId::BeraChain
            | ExtendedChainId::Aurora
            | ExtendedChainId::Solana
            | ExtendedChainId::Ton => None,
        }
    }
}

#[async_trait]
impl Provider for InfuraProvider {
    async fn fetch_endpoints(&self) -> anyhow::Result<HashMap<ExtendedChainId, String>> {
        let mut endpoints = HashMap::new();
        for chain_id in ExtendedChainId::iter() {
            if let Some(slug) = Self::slug(chain_id) {
                let url = format!("https://{}.infura.io/v3/{}", slug, &self.api_key);
                endpoints.insert(chain_id, url);
            }
        }
        Ok(endpoints)
    }
}
