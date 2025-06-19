//! We want to ensure for each node that its RPC servers are intact.
//! Possible scenarios what might go wrong:
//!     1. No API credits left for a specific provider
//!     2. Some specific endpoints became unavailable
//!
//! It is expected for the end-user to have two panes:
//!     1. A heatmap Node x Chain, with each value be `available - threshold`.
//!         Green – the diff is positive, Yellow – neutral, Red – negative
//!     2. If something's not green, then we just output the servers that are not available.

use crate::{ChainId, ChainValidationConfig, Validation};
use futures_util::{stream, StreamExt};
use lazy_static::lazy_static;
use prometheus::{register_gauge, register_int_gauge_vec};
use reqwest::Url;
use serde_json::json;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::sleep;

const METRICS_EVALUATION_INTERVAL: Duration = Duration::from_secs(2 * 60);
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

lazy_static! {
    pub static ref RPC_AVAILABILITY_LAST_CHECKED: prometheus::Gauge = register_gauge!(
        "rpc_availability_last_checked_seconds",
        "Unix timestamp of last RPC availability check"
    )
    .unwrap();
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
fn get_two_part_domain(url: &str) -> String {
    let host = Url::parse(url)
        .map(|url| url.domain().unwrap_or("None").to_string())
        .unwrap_or("None".to_string());

    let mut iter = host.split('.');
    let domain2 = iter.next_back().unwrap_or("None");
    let domain1 = iter.next_back().unwrap_or("None");
    format!("{domain1}.{domain2}")
}

impl Validation {
    /// Create a future that will run a metric-emitting loop.
    /// It's expected to pass this future in the executor as a background task.
    pub async fn availability_checker(configs: HashMap<ChainId, ChainValidationConfig>) {
        // Create the new client so we don't mess up the other TCP pool for the actual validation
        let client = reqwest::Client::new();

        loop {
            for (&chain_id, config) in &configs {
                let availability =
                    check_availability_for_chain(&client, chain_id, &config.servers).await;

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

            RPC_AVAILABILITY_LAST_CHECKED.set(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as f64,
            );
            sleep(METRICS_EVALUATION_INTERVAL).await;
        }
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
