//! We want to ensure for each node that its RPC servers are intact.
//! Possible scenarios what might go wrong:
//!     1. No API credits left for a specific provider
//!     2. Some specific endpoints became unavailable
//!
//! It is expected for the end-user to have two panes:
//!     1. A heatmap Node x Chain, with each value be `available - threshold`.
//!         Green – the diff is positive, Yellow – neutral, Red – negative
//!     2. If something's not green, then we just output the servers that are not available.
use crate::{ChainId, ChainValidationConfig};
use futures_util::stream;
use futures_util::StreamExt;
use lazy_static::lazy_static;
use prometheus::register_int_gauge_vec;
use reqwest::{Client, Url};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub(crate) mod performance;

const METRICS_EVALUATION_INTERVAL: Duration = Duration::from_secs(60);
const MAX_CONCURRENT_REQUESTS: usize = 5;
const TIMEOUT_DURATION: Duration = Duration::from_secs(2);

lazy_static! {
    pub static ref RPC_AVAILABILITY_SERVER_UP: prometheus::IntGaugeVec = register_int_gauge_vec!(
        "rpc_availability_server_up",
        "1 if server is available, 0 if not",
        &["chain_id", "domain"]
    )
    .unwrap();
}

lazy_static! {
    pub static ref RPC_AVAILABILITY_THRESHOLD_DELTA: prometheus::IntGaugeVec =
        register_int_gauge_vec!(
            "rpc_availability_threshold_delta",
            "Difference between available and threshold number of servers",
            &["chain_id"]
        )
        .unwrap();
}

#[derive(Clone)]
pub struct Metrics {
    configs: HashMap<ChainId, ChainValidationConfig>,
    shutdown_token: CancellationToken,
    client: Client,
}

impl Metrics {
    pub fn new(configs: HashMap<ChainId, ChainValidationConfig>) -> Self {
        Self {
            configs,
            shutdown_token: Default::default(),
            client: Default::default(),
        }
    }

    pub fn start_checker(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let token = self.shutdown_token.child_token();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(METRICS_EVALUATION_INTERVAL);
            loop {
                tokio::select! {
                    _ = token.cancelled() => {
                        tracing::info!("Availability checker shutting down");
                        break;
                    }
                    _ = interval.tick() => {
                        if let Err(e) = self.check_all_servers().await {
                            tracing::warn!("Availability check failed: {:?}", e);
                        }
                    }
                }
            }
        })
    }

    async fn check_all_servers(&self) -> anyhow::Result<()> {
        for (&chain_id, config) in &self.configs {
            if chain_id == ChainId::Ton {
                // TODO: Add TON availability check
                continue;
            }
            let availability =
                check_availability_for_chain(&self.client, chain_id, &config.servers).await;

            let chain_label = chain_id.to_string();

            for (server, &is_available) in &availability {
                let domain_label = get_two_part_domain(server);
                RPC_AVAILABILITY_SERVER_UP
                    .with_label_values(&[&chain_label, &domain_label])
                    .set(if is_available { 1 } else { 0 });
            }

            RPC_AVAILABILITY_THRESHOLD_DELTA
                .with_label_values(&[&chain_label])
                .set({
                    let available = availability.values().filter(|&&v| v).count() as i64;
                    available - config.threshold as i64
                })
        }
        Ok(())
    }
}

async fn check_availability_for_chain(
    client: &reqwest::Client,
    chain_id: ChainId,
    servers: &[String],
) -> HashMap<String, bool> {
    let payload = build_payload(chain_id);

    let results = stream::iter(servers.iter().cloned())
        .map(|server| {
            let client = client.clone();
            let payload = payload.clone();
            async move {
                let is_up = client
                    .post(&server)
                    .json(&payload)
                    .timeout(TIMEOUT_DURATION)
                    .send()
                    .await
                    .map(|r| r.status().is_success())
                    .unwrap_or(false);
                (server, is_up)
            }
        })
        .buffer_unordered(MAX_CONCURRENT_REQUESTS)
        .collect::<Vec<(String, bool)>>()
        .await;

    results.into_iter().collect()
}

/// We have an RPC server URL like "https://foo-bar.near-mainnet.quiknode.pro/123123".
/// We want to extract the domain part so that we can use it as a label.
/// It will fail on something like "foo.co.uk", but that's good enough for now.
pub(crate) fn get_two_part_domain(url: &str) -> String {
    let host = Url::parse(url)
        .map(|url| url.domain().unwrap_or("None").to_string())
        .unwrap_or("None".to_string());

    let mut iter = host.split('.');
    let domain2 = iter.next_back().unwrap_or("None");
    let domain1 = iter.next_back().unwrap_or("None");
    format!("{domain1}.{domain2}")
}

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
        ChainId::Ton => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_two_part_domain() {
        let url = "https://foo-bar.near-mainnet.quiknode.pro/123123";
        let domain = get_two_part_domain(url);
        assert_eq!(domain, "quiknode.pro");
    }
}
