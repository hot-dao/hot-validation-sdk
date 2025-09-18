use crate::providers::Provider;
use crate::supported_chains::{ChainId, SlugFromChainId};
use async_trait::async_trait;
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
    fn slug(chain_id: ChainId) -> Option<String> {
        match chain_id {
            ChainId::Eth => Some("eth-mainnet".to_string()),
            ChainId::Optimism => Some("opt-mainnet".to_string()),
            ChainId::Bsc => Some("bnb-mainnet".to_string()),
            ChainId::Polygon => Some("polygon-mainnet".to_string()),
            ChainId::MonadTestnet => Some("monad-testnet".to_string()),
            ChainId::ZkSync => Some("zksync-mainnet".to_string()),
            ChainId::Base => Some("base-mainnet".to_string()),
            ChainId::Arbitrum => Some("arb-mainnet".to_string()),
            ChainId::Avax => Some("avax-mainnet".to_string()),
            ChainId::Scroll => Some("scroll-mainnet".to_string()),
            ChainId::BeraChain => Some("berachain-mainnet".to_string()),
            ChainId::Solana => Some("solana-mainnet".to_string()),

            ChainId::Ton | ChainId::Near | ChainId::Stellar | ChainId::Kava | ChainId::Aurora => {
                None
            }
        }
    }
}

#[async_trait]
impl Provider for AlchemyProvider {
    async fn fetch_endpoints(&self) -> anyhow::Result<HashMap<ChainId, String>> {
        let mut endpoints = HashMap::new();
        for chain_id in ChainId::iter() {
            if let Some(slug) = Self::slug(chain_id) {
                let url = format!("https://{}.g.alchemy.com/v2/{}", slug, &self.api_key);
                endpoints.insert(chain_id, url);
            }
        }
        Ok(endpoints)
    }
}
