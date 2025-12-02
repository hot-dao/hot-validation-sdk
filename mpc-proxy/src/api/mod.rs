use crate::api::bridge::{clear_completed_withdrawal_endpoint, sign_deposit_endpoint, sign_withdraw_endpoint};
use crate::api::healthcheck::healthcheck;
use crate::api::public_key::public_key;
use crate::api::sign::{sign, sign_raw};
use crate::api::telemetry::prometheus_metrics;
use crate::domain::mpc::cluster::ClusterManager;
use crate::secrets::SecretsConfig;
use axum::Router;
use axum::routing::{get, post};
use hot_validation_core::Validation;
use std::sync::Arc;

pub(crate) mod bridge;
mod healthcheck;
mod public_key;
mod sign;
mod telemetry;

#[derive(Clone)]
pub(crate) struct AppState {
    pub secrets_config: Arc<SecretsConfig>,
    pub cluster_manager: Arc<ClusterManager>,
    pub validation: Arc<Validation>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/healthcheck", get(healthcheck))
        .route("/prometheus-metrics", get(prometheus_metrics))
        .route("/deposit/sign", post(sign_deposit_endpoint))
        .route("/withdraw/sign", post(sign_withdraw_endpoint))
        .route("/clear/sign", post(clear_completed_withdrawal_endpoint))
        .route("/sign_raw", post(sign_raw))
        .route("/sign", post(sign))
        .route("/public_key", post(public_key))
}
