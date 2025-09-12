use crate::providers::Provider;
use crate::supported_chains::{ChainId, SlugFromChainId};
use anyhow::Result;
use async_trait::async_trait;
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
    fn slug(chain_id: ChainId) -> Option<String> {
        match chain_id {
            ChainId::Eth => Some("eth".to_string()),
            ChainId::Optimism => Some("optimism".to_string()),
            ChainId::Bsc => Some("bsc".to_string()),
            ChainId::Polygon => Some("polygon".to_string()),
            ChainId::MonadTestnet => Some("monad_testnet".to_string()),
            ChainId::ZkSync => Some("zksync_era".to_string()),
            ChainId::Stellar => Some("stellar_soroban".to_string()),
            ChainId::Base => Some("base".to_string()),
            ChainId::Arbitrum => Some("arbitrum".to_string()),
            ChainId::Avax => Some("avalanche".to_string()),
            ChainId::Scroll => Some("scroll".to_string()),
            ChainId::Ton => Some("premium-http/ton_api_v2".to_string()),

            ChainId::Near | ChainId::Kava | ChainId::BeraChain | ChainId::Aurora => None,
        }
    }
}

#[async_trait]
impl Provider for AnkrProvider {
    async fn fetch_endpoints(&self) -> Result<HashMap<ChainId, String>> {
        let mut endpoints = HashMap::new();
        for chain_id in ChainId::iter() {
            if let Some(slug) = Self::slug(chain_id) {
                let url = if matches!(chain_id, ChainId::Ton) {
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
