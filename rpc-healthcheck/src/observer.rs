use crate::healthcheck_many;
use hot_validation_primitives::{ChainId, ChainValidationConfig};
use lazy_static::lazy_static;
use prometheus::register_int_gauge_vec;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const METRICS_EVALUATION_INTERVAL: Duration = Duration::from_secs(30);

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
pub struct Observer {
    configs: HashMap<ChainId, ChainValidationConfig>,
    shutdown_token: CancellationToken,
    client: Client,
}

impl Observer {
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
            let availability = healthcheck_many(&self.client, chain_id, &config.servers).await;

            let chain_label = chain_id.to_string();

            let mut available_serveres = 0;
            for result in &availability {
                match result {
                    Ok(server) => {
                        available_serveres += 1;
                        RPC_AVAILABILITY_SERVER_UP
                            .with_label_values(&[&chain_label, &server.0])
                            .set(1);
                    }
                    Err(failed_server) => {
                        RPC_AVAILABILITY_SERVER_UP
                            .with_label_values(&[&chain_label, &failed_server.server.0])
                            .set(0);
                    }
                }
            }

            RPC_AVAILABILITY_THRESHOLD_DELTA
                .with_label_values(&[&chain_label])
                .set(available_serveres - config.threshold as i64)
        }
        Ok(())
    }
}
