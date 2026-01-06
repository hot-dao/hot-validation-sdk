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
        use ExtendedChainId::{
            ADI, Abstract, Arbitrum, Aurora, Avax, Base, BeraChain, Bsc, Eth, Flare, Gonka,
            HyperEVM, Ink, Juno, Kaia, Kava, Linea, Mantle, MegaEthTestnet, MonadMainnet,
            MonadTestnet, Near, Optimism, Plasma, Polygon, Scroll, Solana, Stellar, Ton, XLayer,
            ZkSync,
        };
        match chain_id {
            Eth => Some("eth".to_string()),
            Optimism => Some("optimism".to_string()),
            Bsc => Some("bsc".to_string()),
            Polygon => Some("polygon".to_string()),
            MonadTestnet => Some("monad_testnet".to_string()),
            ZkSync => Some("zksync_era".to_string()),
            Stellar => Some("stellar_soroban".to_string()),
            Base => Some("base".to_string()),
            Arbitrum => Some("arbitrum".to_string()),
            Avax => Some("avalanche".to_string()),
            Scroll => Some("scroll".to_string()),
            Solana => Some("solana".to_string()),
            Kava => Some("kava_rpc".to_string()),
            XLayer => Some("xlayer".to_string()),
            Linea => Some("linea".to_string()),
            Kaia => Some("kaia".to_string()),
            Mantle => Some("mantle".to_string()),
            Flare => Some("flare".to_string()),
            Ton => Some("premium-http/ton_api_v2".to_string()),

            ADI | Juno | Gonka | MonadMainnet | Near | Abstract | Ink | HyperEVM | BeraChain
            | Aurora | Plasma | MegaEthTestnet => None,
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
