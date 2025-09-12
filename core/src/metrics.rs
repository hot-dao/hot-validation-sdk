use lazy_static::lazy_static;
use prometheus::{register_histogram, register_int_counter_vec};

lazy_static! {
    pub static ref RPC_VERIFY_TOTAL_DURATION: prometheus::Histogram = register_histogram!(
        "rpc_verify_total_duration_seconds",
        "Histogram of how long top-level verify() takes",
        vec![
            // head-room for any near-zero calls
            0.01, 0.02, 0.05,
            // main cluster (zoomed in 0.08 → 0.18)
            0.08, 0.10, 0.12, 0.14, 0.16, 0.18,
            // upper-cluster to catch the bulk
            0.30,
            // tail for slow outliers
            0.75,
            // extreme outliers
            2.0,
        ]
    )
    .unwrap();
}

lazy_static! {
    pub static ref RPC_SINGLE_VERIFY_DURATION: prometheus::Histogram = register_histogram!(
        "rpc_single_verify_duration_seconds",
        "Histogram of how long individual verify() takes",
        vec![0.01, 0.02, 0.03, 0.04, 0.05, 0.25, 1.0, 1.5, 2.0,]
    )
    .unwrap();
}

lazy_static! {
    pub static ref RPC_GET_AUTH_METHODS_DURATION: prometheus::Histogram = register_histogram!(
        "rpc_get_auth_methods_duration_seconds",
        "Histogram of how long `get auth methods for wallet` takes",
        vec![
            // very tight around the sub-0.05s cluster
            0.01, 0.02, 0.03, 0.05, 0.06, 0.07,
            // the bulk of calls
            0.08, 0.1, 0.15,
            // slow tail
            0.5,
            // outliers
            1.5,
        ]
    )
    .unwrap();
}

lazy_static! {
    pub static ref VERIFY_TOTAL_ATTEMPTS: prometheus::IntCounterVec = register_int_counter_vec!(
        "verify_total_attempts",
        "Total attempts to perform verify per chain",
        &["chain_id"]
    )
    .unwrap();
}

lazy_static! {
    pub static ref VERIFY_SUCCESS_ATTEMPTS: prometheus::IntCounterVec = register_int_counter_vec!(
        "verify_success_attempts",
        "Success attempts to perform verify per chain",
        &["chain_id"]
    )
    .unwrap();
}
