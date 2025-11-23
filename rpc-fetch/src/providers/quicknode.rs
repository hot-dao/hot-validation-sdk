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
        use ExtendedChainId::{
            Abstract, Arbitrum, Aurora, Avax, Base, BeraChain, Bsc, Eth, Flare, HyperEVM, Ink,
            Kaia, Kava, Linea, Mantle, MonadMainnet, MonadTestnet, Near, Optimism, Polygon, Scroll,
            Solana, Stellar, Ton, XLayer, ZkSync,
        };
        match chain_id {
            Optimism => Some("optimism".to_string()),
            Bsc => Some("bsc".to_string()),
            Polygon => Some("matic".to_string()),
            MonadMainnet => Some("monad-mainnet".to_string()),
            MonadTestnet => Some("monad-testnet".to_string()),
            ZkSync => Some("zksync-mainnet".to_string()),
            Stellar => Some("stellar-mainnet".to_string()),
            Base => Some("base-mainnet".to_string()),
            Arbitrum => Some("arbitrum-mainnet".to_string()),
            Avax => Some("avalanche-mainnet".to_string()),
            BeraChain => Some("bera-mainnet".to_string()),
            Scroll => Some("scroll-mainnet".to_string()),
            Ton => Some("ton-mainnet".to_string()),
            Solana => Some("solana-mainnet".to_string()),
            XLayer => Some("xlayer-mainnet".to_string()),
            HyperEVM => Some("hype-mainnet".to_string()),
            Linea => Some("linea-mainnet".to_string()),
            Kaia => Some("kaia-mainnet".to_string()),
            Mantle => Some("mantle-mainnet".to_string()),
            Abstract => Some("abstract-mainnet".to_string()),
            Ink => Some("ink-mainnet".to_string()),
            Flare => Some("flare-mainnet".to_string()),

            Near | // it's "near-mainnet", but the load is too high, so we don't add it automatically, 
                   // rather we supply custom endpoints for it
            Eth | // has to return base endpoint
            Kava |
            Aurora => None,
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
                } else if matches!(chain, ExtendedChainId::Flare) {
                    ep += "ext/bc/C/rpc/";
                }
                map.insert(chain, ep.clone());
            }
        }

        Ok(map)
    }
}
