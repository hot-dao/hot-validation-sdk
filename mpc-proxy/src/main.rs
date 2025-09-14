mod cli;
mod handlers;
mod openapi;

use actix_web::{web, App, HttpServer, Responder};
use tracing_subscriber::EnvFilter;
use tracing_actix_web::TracingLogger;
use std::{env, time::Duration};
use actix_web::middleware::Logger;
use clap::Parser;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;
use crate::cli::Cli;
use crate::handlers::healthcheck::healthcheck;
use crate::handlers::telemetry::prometheus_metrics;
use crate::openapi::ApiDoc;

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,actix_web=info,tracing_actix_web=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .json()
        .with_current_span(true)
        .with_span_list(true)
        .init();
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    init_tracing();
    let cfg = Cli::parse();

    tracing::info!(?cfg, "starting server");

    HttpServer::new(|| {
        App::new()
            .wrap(TracingLogger::default())
            .wrap(Logger::default())
            .service(healthcheck)
            .service(prometheus_metrics)
            .service(
                SwaggerUi::new("/docs/{_:.*}")
                    .url("/api-docs/openapi.json", ApiDoc::openapi())
            )
    })
        .workers(cfg.workers)
        .keep_alive(Duration::from_secs(cfg.keep_alive_secs))
        .client_request_timeout(Duration::from_millis(cfg.client_timeout_ms))
        .client_disconnect_timeout(Duration::from_millis(cfg.client_disconnect_timeout))
        .max_connection_rate(cfg.max_conn_rate)
        .bind(&cfg.bind_addr)?
        .run()
        .await
}
