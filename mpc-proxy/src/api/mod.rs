use crate::api::bridge::{clear_completed_withdrawal_endpoint, sign_deposit_endpoint, sign_withdraw_endpoint};
use crate::api::healthcheck::healthcheck_endpoint;
use crate::api::public_key::public_key_endpoint;
use crate::api::sign::{sign_endpoint, sign_raw_endpoint};
use crate::api::telemetry::prometheus_metrics;
use crate::domain::mpc::cluster::ClusterManager;
use crate::secrets::SecretsConfig;
use axum::Router;
use axum::routing::{get, post};
use hot_validation_core::Validation;
use std::sync::Arc;
use crate::api::create_wallet::create_wallet_endpoint;

pub(crate) mod bridge;
mod healthcheck;
mod public_key;
mod sign;
mod telemetry;
mod create_wallet;

#[derive(Clone)]
pub(crate) struct AppState {
    pub secrets_config: Arc<SecretsConfig>,
    pub cluster_manager: Arc<ClusterManager>,
    pub validation: Arc<Validation>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/healthcheck", get(healthcheck_endpoint))
        .route("/prometheus-metrics", get(prometheus_metrics))
        .route("/deposit/sign", post(sign_deposit_endpoint))
        .route("/withdraw/sign", post(sign_withdraw_endpoint))
        .route("/clear/sign", post(clear_completed_withdrawal_endpoint))
        .route("/sign_raw", post(sign_raw_endpoint))
        .route("/sign", post(sign_endpoint))
        .route("/public_key", post(public_key_endpoint))
        .route("/create_wallet", post(create_wallet_endpoint))
}
