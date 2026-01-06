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
        use ExtendedChainId::{
            ADI, Abstract, Arbitrum, Aurora, Avax, Base, BeraChain, Bsc, Eth, Flare, Gonka,
            HyperEVM, Ink, Juno, Kaia, Kava, Linea, Mantle, MonadMainnet, MonadTestnet, Near,
            Optimism, Polygon, Scroll, Solana, Stellar, Ton, XLayer, ZkSync, Plasma, MegaEthTestnet
        };
        match chain_id {
            Eth => Some("eth-mainnet".to_string()),
            Optimism => Some("opt-mainnet".to_string()),
            Bsc => Some("bnb-mainnet".to_string()),
            Polygon => Some("polygon-mainnet".to_string()),
            MonadMainnet => Some("monad-mainnet".to_string()),
            MonadTestnet => Some("monad-testnet".to_string()),
            ZkSync => Some("zksync-mainnet".to_string()),
            Base => Some("base-mainnet".to_string()),
            Arbitrum => Some("arb-mainnet".to_string()),
            Avax => Some("avax-mainnet".to_string()),
            Scroll => Some("scroll-mainnet".to_string()),
            BeraChain => Some("berachain-mainnet".to_string()),
            Solana => Some("solana-mainnet".to_string()),
            HyperEVM => Some("hyperliquid-mainnet".to_string()),
            Linea => Some("linea-mainnet".to_string()),
            Mantle => Some("mantle-mainnet".to_string()),
            Abstract => Some("abstract-mainnet".to_string()),
            Ink => Some("ink-mainnet".to_string()),
            ADI => Some("adi-mainnet".to_string()),
            Plasma => Some("plasma-mainnet".to_string()),
            MegaEthTestnet => Some("megaeth-testnet".to_string()),

            Juno | Gonka | Ton | Flare | Kaia | XLayer | Near | Stellar | Kava | Aurora => {
                None
            },
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
