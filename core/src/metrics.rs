use hot_validation_primitives::{ChainId, ExtendedChainId};
use prometheus::{register_histogram, register_int_counter_vec};
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
