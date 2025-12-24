mod api;
mod cli;
mod domain;
mod secrets;
mod telemetry;

use crate::api::AppState;
use crate::cli::Cli;
use crate::domain::mpc::api::Server;
use crate::domain::mpc::cluster::ClusterManager;
use crate::secrets::SecretsConfig;
use crate::telemetry::init_telemetry;
use anyhow::{Context, Result};
use axum::Router;
use axum::extract::MatchedPath;
use clap::Parser;
use hot_validation_core::Validation;
use hot_validation_primitives::ValidationConfig;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use tracing::{Level, info};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let _telemetry_guard = init_telemetry(&cli.alloy_endpoint)?;

    info!(?cli, "starting server");

    let secrets_config = {
        #[cfg(feature = "debug")]
        let result =
            SecretsConfig::decrypt_with_key(&cli.encryption_key_path, &cli.encrypted_config_path)?;

        #[cfg(not(feature = "debug"))]
        let result = SecretsConfig::decrypt_with_prompt(&cli.encrypted_config_path)?;

        result
    };
    let validation = {
        let validation_config: ValidationConfig = {
            let file = std::fs::read_to_string(&cli.validation_config_path)
                .context("failed to read validation config")?;
            serde_yaml::from_str(&file)?
        };
        Validation::new(&validation_config)?
    };
    let cluster_manager = {
        let cluster_config: Vec<Vec<Server>> = {
            let file = std::fs::read_to_string(&cli.cluster_config_path)
                .context("failed to read cluster config")?;
            serde_yaml::from_str(&file)?
        };
        ClusterManager::new(cluster_config).await?
    };

    let state = AppState {
        secrets_config: Arc::new(secrets_config),
        cluster_manager,
        validation: Arc::new(validation),
    };

    // ----- routes -----
    let app = Router::new()
        .merge(api::router())
        .with_state(state)
        // ----- middleware stack (applies to all routes) -----
        .layer(
            ServiceBuilder::new()
                // set and propagate X-Request-Id
                .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
                .layer(PropagateRequestIdLayer::x_request_id())
                // structured HTTP tracing
                .layer(
                    TraceLayer::new_for_http().make_span_with(|req: &axum::http::Request<_>| {
                        let route = req
                            .extensions()
                            .get::<MatchedPath>()
                            .map_or("-", axum::extract::MatchedPath::as_str);
                        let ua = req
                            .headers()
                            .get(axum::http::header::USER_AGENT)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("-");
                        let id = req
                            .headers()
                            .get(axum::http::header::HeaderName::from_static("x-request-id"))
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("-");
                        tracing::span!(
                            Level::INFO, "mpc-proxy",
                            method=%req.method(),
                            uri=%req.uri(),
                            route,
                            user_agent=%ua,
                            version=?req.version(),
                            x_request_id=id,
                            status=tracing::field::Empty,
                            elapsed_ms=tracing::field::Empty,
                        )
                    }),
                )
                .layer(CompressionLayer::new())
                .layer(TimeoutLayer::new(Duration::from_secs(15))),
        );

    // ----- serve with graceful shutdown -----
    let addr: SocketAddr = ([0, 0, 0, 0], cli.port).into();
    let listener = TcpListener::bind(addr).await?;
    info!("listening on http://{addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        signal(SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
