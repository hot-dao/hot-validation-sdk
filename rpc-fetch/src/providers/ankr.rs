use crate::providers::{Provider, SlugFromChainId};
use anyhow::Result;
use async_trait::async_trait;
use hot_validation_primitives::ExtendedChainId;
use std::collections::HashMap;
use strum::IntoEnumIterator;

pub struct AnkrProvider {
    api_key: String,
}

impl AnkrProvider {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

impl SlugFromChainId for AnkrProvider {
    fn slug(chain_id: ExtendedChainId) -> Option<String> {
        match chain_id {
            ExtendedChainId::Eth => Some("eth".to_string()),
            ExtendedChainId::Optimism => Some("optimism".to_string()),
            ExtendedChainId::Bsc => Some("bsc".to_string()),
            ExtendedChainId::Polygon => Some("polygon".to_string()),
            ExtendedChainId::MonadTestnet => Some("monad_testnet".to_string()),
            ExtendedChainId::ZkSync => Some("zksync_era".to_string()),
            ExtendedChainId::Stellar => Some("stellar_soroban".to_string()),
            ExtendedChainId::Base => Some("base".to_string()),
            ExtendedChainId::Arbitrum => Some("arbitrum".to_string()),
            ExtendedChainId::Avax => Some("avalanche".to_string()),
            ExtendedChainId::Scroll => Some("scroll".to_string()),
            ExtendedChainId::Ton => Some("premium-http/ton_api_v2".to_string()),
            ExtendedChainId::Solana => Some("solana".to_string()),
            ExtendedChainId::Kava => Some("kava_rpc".to_string()),

            ExtendedChainId::Near | ExtendedChainId::BeraChain | ExtendedChainId::Aurora => None,
        }
    }
}

#[async_trait]
impl Provider for AnkrProvider {
    async fn fetch_endpoints(&self) -> Result<HashMap<ExtendedChainId, String>> {
        let mut endpoints = HashMap::new();
        for chain_id in ExtendedChainId::iter() {
            if let Some(slug) = Self::slug(chain_id) {
                let url = if matches!(chain_id, ExtendedChainId::Ton) {
                    format!("https://rpc.ankr.com/{}/{}/jsonRPC", slug, &self.api_key)
                } else {
                    format!("https://rpc.ankr.com/{}/{}", slug, &self.api_key)
                };
                endpoints.insert(chain_id, url);
            }
        }
        Ok(endpoints)
    }
}
