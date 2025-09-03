pub mod observer;

use anyhow::Result;
use futures_util::{StreamExt, stream};
use hot_validation_primitives::ChainId;
use reqwest::{Client, Url};
use serde_json::json;
use std::time::Duration;

const MAX_CONCURRENT_REQUESTS: usize = 5;
const TIMEOUT_DURATION: Duration = Duration::from_secs(5);

fn build_payload(chain_id: ChainId) -> serde_json::Value {
    match chain_id {
        ChainId::Near => {
            json!({
                "jsonrpc": "2.0",
                "method": "block",
                "params": { "finality": "final" },
                "id": 1,
            })
        }
        ChainId::Stellar => {
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "getLatestLedger"
            })
        }
        ChainId::Evm(_) => {
            json!({
                "jsonrpc": "2.0",
                "method": "eth_blockNumber",
                "params": [],
                "id": 1
            })
        }
        _ => {
            unreachable!()
        }
    }
}

/// An RPC url with removed secret
#[derive(Debug, Clone)]
pub struct SafeUrl(String);

/// We have an RPC server URL like "https://foo-bar.near-mainnet.quiknode.pro/123123".
/// We want to extract the domain part so that we can use it as a label.
/// It will fail on something like "foo.co.uk", but that's good enough for now.
pub(crate) fn get_two_part_domain(url: &str) -> SafeUrl {
    let host = Url::parse(url)
        .map(|url| url.domain().unwrap_or("None").to_string())
        .unwrap_or("None".to_string());

    let mut iter = host.split('.');
    let domain2 = iter.next_back().unwrap_or("None");
    let domain1 = iter.next_back().unwrap_or("None");
    SafeUrl(format!("{domain1}.{domain2}"))
}

#[derive(Debug, Clone)]
pub struct FailedServer {
    pub server: SafeUrl,
    pub error: String,
}

impl FailedServer {
    pub fn new(server: String, error: String) -> Self {
        let extract = get_two_part_domain(&server);
        Self {
            server: extract,
            error,
        }
    }
}

pub async fn healthcheck_many(
    client: &Client,
    chain_id: ChainId,
    servers: &[String],
) -> Vec<Result<SafeUrl, FailedServer>> {
    let payload = build_payload(chain_id);

    let results = stream::iter(servers.iter().cloned())
        .map(|server| {
            let client = client.clone();
            let payload = payload.clone();
            async move {
                client
                    .post(&server)
                    .json(&payload)
                    .timeout(TIMEOUT_DURATION)
                    .send()
                    .await
                    .map_err(|e| FailedServer::new(server.clone(), e.to_string()))?
                    .error_for_status()
                    .map_err(|e| FailedServer::new(server.clone(), e.to_string()))?;
                Ok(get_two_part_domain(&server))
            }
        })
        .buffer_unordered(MAX_CONCURRENT_REQUESTS)
        .collect::<Vec<_>>()
        .await;

    results.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::{FailedServer, SafeUrl, healthcheck_many};
    use anyhow::{Result, bail};
    use hot_validation_primitives::ChainId;
    use reqwest::Client;

    /// Helper: pass if any endpoint succeeds; fail with aggregated errors otherwise.
    fn assert_any_ok(results: Vec<Result<SafeUrl, FailedServer>>) -> Result<()> {
        let (oks, errs): (Vec<_>, Vec<_>) = results.into_iter().partition(Result::is_ok);
        if !oks.is_empty() {
            Ok(())
        } else {
            bail!("all endpoints failed: {}, {}", oks.len(), errs.len())
        }
    }

    #[tokio::test]
    async fn near_healthcheck_many() -> Result<()> {
        let client = Client::new();
        let servers = vec![
            "https://rpc.mainnet.near.org".to_string(),
            "https://nearrpc.aurora.dev".to_string(),
        ];
        let results = healthcheck_many(&client, ChainId::Near, &servers).await;
        assert_any_ok(results)
    }

    #[tokio::test]
    async fn stellar_healthcheck_many() -> Result<()> {
        let client = Client::new();
        let servers = vec![
            "https://mainnet.sorobanrpc.com".to_string(),
            "https://stellar-soroban-public.nodies.app".to_string(),
        ];
        let results = healthcheck_many(&client, ChainId::Stellar, &servers).await;
        assert_any_ok(results)
    }

    #[tokio::test]
    async fn eth_healthcheck_many() -> Result<()> {
        let client = Client::new();
        let servers = vec![
            "https://ethereum.publicnode.com".to_string(),
            "https://cloudflare-eth.com".to_string(),
        ];
        let results = healthcheck_many(&client, ChainId::Evm(1), &servers).await;
        assert_any_ok(results)
    }

    #[tokio::test]
    async fn optimism_healthcheck_many() -> Result<()> {
        let client = Client::new();
        let servers = vec![
            "https://optimism.publicnode.com".to_string(),
            "https://mainnet.optimism.io".to_string(),
        ];
        let results = healthcheck_many(&client, ChainId::Evm(10), &servers).await;
        assert_any_ok(results)
    }
}
