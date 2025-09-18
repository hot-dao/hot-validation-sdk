use crate::providers::{Provider, SlugFromChainId};
use async_trait::async_trait;
use hot_validation_primitives::ExtendedChainId;
use std::collections::HashMap;
use strum::IntoEnumIterator;

pub struct AlchemyProvider {
    api_key: String,
}

impl AlchemyProvider {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

impl SlugFromChainId for AlchemyProvider {
    fn slug(chain_id: ExtendedChainId) -> Option<String> {
        match chain_id {
            ExtendedChainId::Eth => Some("eth-mainnet".to_string()),
            ExtendedChainId::Optimism => Some("opt-mainnet".to_string()),
            ExtendedChainId::Bsc => Some("bnb-mainnet".to_string()),
            ExtendedChainId::Polygon => Some("polygon-mainnet".to_string()),
            ExtendedChainId::MonadTestnet => Some("monad-testnet".to_string()),
            ExtendedChainId::ZkSync => Some("zksync-mainnet".to_string()),
            ExtendedChainId::Base => Some("base-mainnet".to_string()),
            ExtendedChainId::Arbitrum => Some("arb-mainnet".to_string()),
            ExtendedChainId::Avax => Some("avax-mainnet".to_string()),
            ExtendedChainId::Scroll => Some("scroll-mainnet".to_string()),
            ExtendedChainId::BeraChain => Some("berachain-mainnet".to_string()),
            ExtendedChainId::Solana => Some("solana-mainnet".to_string()),

            ExtendedChainId::Ton
            | ExtendedChainId::Near
            | ExtendedChainId::Stellar
            | ExtendedChainId::Kava
            | ExtendedChainId::Aurora => None,
        }
    }
}

#[async_trait]
impl Provider for AlchemyProvider {
    async fn fetch_endpoints(&self) -> anyhow::Result<HashMap<ExtendedChainId, String>> {
        let mut endpoints = HashMap::new();
        for chain_id in ExtendedChainId::iter() {
            if let Some(slug) = Self::slug(chain_id) {
                let url = format!("https://{}.g.alchemy.com/v2/{}", slug, &self.api_key);
                endpoints.insert(chain_id, url);
            }
        }
        Ok(endpoints)
    }
}
