use hot_validation_primitives::{ChainId, ExtendedChainId};
use prometheus::{
    register_histogram, register_int_counter_vec, register_int_gauge_vec, IntCounterVec,
    IntGaugeVec,
};
use reqwest::Url;
use std::sync::LazyLock;

pub static RPC_VERIFY_TOTAL_DURATION: LazyLock<prometheus::Histogram> = LazyLock::new(|| {
    register_histogram!(
        "rpc_verify_total_duration_seconds",
        "Histogram of how long top-level verify() takes",
        vec![0.02, 0.03, 0.04, 0.05, 0.06, 0.08, 0.09, 0.10, 0.12, 0.15, 0.20,]
    )
    .expect("register rpc_verify_total_duration_seconds")
});

pub static RPC_SINGLE_VERIFY_DURATION: LazyLock<prometheus::Histogram> = LazyLock::new(|| {
    register_histogram!(
        "rpc_single_verify_duration_seconds",
        "Histogram of how long individual verify() takes",
        vec![0.01, 0.02, 0.03, 0.04, 0.05, 0.06, 0.08, 0.09, 0.10, 0.12, 0.15, 0.20,]
    )
    .expect("register rpc_single_verify_duration_seconds")
});

pub static RPC_GET_AUTH_METHODS_DURATION: LazyLock<prometheus::Histogram> = LazyLock::new(|| {
    register_histogram!(
        "rpc_get_auth_methods_duration_seconds",
        "Histogram of how long `get auth methods for wallet` takes",
        vec![0.01, 0.02, 0.03, 0.04, 0.05, 0.06, 0.08, 0.09, 0.10, 0.12, 0.15, 0.20,]
    )
    .expect("register rpc_get_auth_methods_duration_seconds")
});

#[inline]
fn chain_label(chain_id: ChainId) -> String {
    ExtendedChainId::try_from(chain_id).map_or_else(|_| chain_id.to_string(), |x| x.to_string())
}

pub static RPC_THRESHOLD_DELTA: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec!(
        "rpc_threshold_delta",
        "Static difference between threshold and total number of servers",
        &["chain_id"]
    )
    .expect("register rpc_threshold_delta")
});

pub fn set_threshold_delta(chain_id: ChainId, total: usize, threshold: usize) {
    RPC_THRESHOLD_DELTA
        .with_label_values(&[&chain_label(chain_id)])
        .set((total - threshold) as i64);
}

static RPC_CALL_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "rpc_call_total",
        "Total calls to the RPC",
        &["chain_id", "provider"]
    )
    .expect("register rpc_call_total")
});

pub fn bump_metrics_rpc_call_total(chain_id: ChainId, url: &str) {
    let chain_label = chain_label(chain_id);
    let provider = second_level_or_url(url);
    RPC_CALL_TOTAL
        .with_label_values(&[&chain_label, &provider])
        .inc();
}

static RPC_CALL_FAILS: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "rpc_call_fails",
        "Total calls to the RPC that failed due to transport error",
        &["chain_id", "provider"]
    )
    .expect("register rpc_call_fails")
});

pub fn bump_metrics_rpc_call_fail(chain_id: ChainId, url: &str) {
    let chain_label = chain_label(chain_id);
    let provider = second_level_or_url(url);
    RPC_CALL_FAILS
        .with_label_values(&[&chain_label, &provider])
        .inc();
}

fn second_level_or_url(url: &str) -> String {
    match Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(std::string::ToString::to_string))
    {
        Some(host) => {
            let parts: Vec<&str> = host.split('.').collect();
            if parts.len() >= 2 {
                parts[parts.len() - 2].to_string()
            } else {
                url.to_string()
            }
        }
        None => url.to_string(), // not a valid URL
    }
}

pub static VERIFY_TOTAL_ATTEMPTS: LazyLock<prometheus::IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "verify_total_attempts",
        "Total attempts to perform verify per chain",
        &["chain_id"]
    )
    .expect("register verify_total_attempts")
});

pub fn tick_metrics_verify_total_attempts(chain_id: ChainId) {
    let chain_label = chain_label(chain_id);
    VERIFY_TOTAL_ATTEMPTS
        .with_label_values(&[&chain_label])
        .inc();
}

pub static VERIFY_SUCCESS_ATTEMPTS: LazyLock<prometheus::IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "verify_success_attempts",
        "Success attempts to perform verify per chain",
        &["chain_id"]
    )
    .expect("register verify_success_attempts")
});

pub fn tick_metrics_verify_success_attempts(chain_id: ChainId) {
    let chain_label = chain_label(chain_id);
    VERIFY_SUCCESS_ATTEMPTS
        .with_label_values(&[&chain_label])
        .inc();
}

#[cfg(test)]
mod tests {
    use crate::metrics::second_level_or_url;

    #[test]
    fn test_second_level_or_url() {
        assert_eq!(second_level_or_url("http://bar.foo.baz"), "foo");
        assert_eq!(second_level_or_url("http://foo.baz"), "foo");
        assert_eq!(second_level_or_url("http://123.123.123.123"), "123");
    }
}
