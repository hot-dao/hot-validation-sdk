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
            ADI, Abstract, Arbitrum, Aurora, Avax, Base, BeraChain, Bsc, Eth, Flare, Gonka,
            HyperEVM, Ink, Juno, Kaia, Kava, Linea, Mantle, MegaEthMainnet, MegaEthTestnet,
            MonadMainnet, MonadTestnet, Near, Optimism, Plasma, Polygon, Scroll, Solana, Stellar,
            Ton, XLayer, ZkSync,
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
            Plasma => Some("plasma-mainnet".to_string()),
            MegaEthMainnet => Some("megaeth-mainnet".to_string()),

            Near | // it's "near-mainnet", but the load is too high, so we don't add it automatically, 
                   // rather we supply custom endpoints for it
            Eth | // has to return base endpoint
            ADI |
            Kava |
            Juno |
            MegaEthTestnet |
            Gonka |
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

    fn base_endpoint(endpoint: &str) -> String {
        let Some((scheme, rest)) = endpoint.split_once("://") else {
            return endpoint.to_string();
        };
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        let Some(prefix) = host.strip_suffix(".quiknode.pro") else {
            return endpoint.to_string();
        };
        let normalized_prefix = Self::strip_network_label(prefix);

        if path.is_empty() {
            format!("{scheme}://{normalized_prefix}.quiknode.pro")
        } else {
            format!("{scheme}://{normalized_prefix}.quiknode.pro/{path}")
        }
    }

    fn endpoint_with_slug(base_endpoint: &str, slug: &str) -> String {
        let Some((scheme, rest)) = base_endpoint.split_once("://") else {
            return base_endpoint.replace(".quiknode.pro", &format!(".{slug}.quiknode.pro"));
        };
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        let Some(prefix) = host.strip_suffix(".quiknode.pro") else {
            return base_endpoint.replace(".quiknode.pro", &format!(".{slug}.quiknode.pro"));
        };
        let normalized_prefix = Self::strip_network_label(prefix);

        if path.is_empty() {
            format!("{scheme}://{normalized_prefix}.{slug}.quiknode.pro")
        } else {
            format!("{scheme}://{normalized_prefix}.{slug}.quiknode.pro/{path}")
        }
    }

    fn strip_network_label(prefix: &str) -> &str {
        let Some((account, label)) = prefix.rsplit_once('.') else {
            return prefix;
        };
        if Self::is_network_label(label) {
            account
        } else {
            prefix
        }
    }

    fn is_network_label(label: &str) -> bool {
        matches!(label, "eth-mainnet" | "ethereum-mainnet" | "mainnet")
            || ExtendedChainId::iter().any(|chain| Self::slug(chain).as_deref() == Some(label))
    }

    fn append_path(mut endpoint: String, path: &str) -> String {
        if !endpoint.ends_with('/') {
            endpoint.push('/');
        }
        endpoint.push_str(path.trim_start_matches('/'));
        endpoint
    }
}

#[async_trait]
impl Provider for QuicknodeProvider {
    async fn fetch_endpoints(&self) -> Result<HashMap<ExtendedChainId, String>> {
        let base_ep = Self::base_endpoint(&self.get_eth_mainnet_endpoint().await?);
        let mut map = HashMap::new();

        for chain in ExtendedChainId::iter() {
            if matches!(chain, ExtendedChainId::Eth) {
                map.insert(chain, base_ep.clone());
            } else if let Some(slug) = Self::slug(chain) {
                let mut ep = Self::endpoint_with_slug(&base_ep, &slug);
                if matches!(chain, ExtendedChainId::Avax) {
                    ep = Self::append_path(ep, "ext/bc/C/rpc/");
                } else if matches!(chain, ExtendedChainId::Ton) {
                    ep = Self::append_path(ep, "jsonRPC");
                } else if matches!(chain, ExtendedChainId::Flare) {
                    ep = Self::append_path(ep, "ext/bc/C/rpc/");
                }
                map.insert(chain, ep.clone());
            }
        }

        Ok(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_with_slug_supports_quicknode_base_without_network_label() {
        let base =
            "https://crimson-broken-scion.quiknode.pro/c3a95e7de56e1b0e0d47b1def0358601491774b6/";
        let endpoint = QuicknodeProvider::append_path(
            QuicknodeProvider::endpoint_with_slug(base, "avalanche-mainnet"),
            "ext/bc/C/rpc/",
        );

        assert_eq!(
            endpoint,
            "https://crimson-broken-scion.avalanche-mainnet.quiknode.pro/c3a95e7de56e1b0e0d47b1def0358601491774b6/ext/bc/C/rpc/"
        );
    }

    #[test]
    fn endpoint_with_slug_supports_quicknode_bsc_url() {
        let base =
            "https://crimson-broken-scion.quiknode.pro/c3a95e7de56e1b0e0d47b1def0358601491774b6/";
        let endpoint = QuicknodeProvider::endpoint_with_slug(base, "bsc");

        assert_eq!(
            endpoint,
            "https://crimson-broken-scion.bsc.quiknode.pro/c3a95e7de56e1b0e0d47b1def0358601491774b6/"
        );
    }

    #[test]
    fn endpoint_with_slug_strips_old_quicknode_network_label() {
        let base = "https://crimson-broken-scion.ethereum-mainnet.quiknode.pro/c3a95e7de56e1b0e0d47b1def0358601491774b6/";
        let endpoint = QuicknodeProvider::append_path(
            QuicknodeProvider::endpoint_with_slug(base, "avalanche-mainnet"),
            "ext/bc/C/rpc/",
        );

        assert_eq!(
            endpoint,
            "https://crimson-broken-scion.avalanche-mainnet.quiknode.pro/c3a95e7de56e1b0e0d47b1def0358601491774b6/ext/bc/C/rpc/"
        );
    }

    #[test]
    fn base_endpoint_strips_old_quicknode_network_label() {
        let base = QuicknodeProvider::base_endpoint(
            "https://crimson-broken-scion.ethereum-mainnet.quiknode.pro/c3a95e7de56e1b0e0d47b1def0358601491774b6/",
        );

        assert_eq!(
            base,
            "https://crimson-broken-scion.quiknode.pro/c3a95e7de56e1b0e0d47b1def0358601491774b6/"
        );
    }
}
