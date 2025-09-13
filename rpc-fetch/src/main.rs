mod providers;
mod supported_chains;

use crate::providers::Provider;
use crate::providers::ankr::AnkrProvider;
use crate::supported_chains::ChainId;
use anyhow::Result;
use clap::{Parser, arg};
use hot_validation_primitives::ChainValidationConfig;
use hot_validation_rpc_healthcheck::healthcheck_many;
use tracing::{error, info, warn};
use providers::quicknode::QuicknodeProvider;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(short, long, default_value = "/data/config.yaml")]
    config: String,
    #[arg(long, default_value = "/app/rpc_config.yaml")]
    output: String,
    #[arg(long, env = "QUICKNODE_API_KEY")]
    quicknode_api_key: Option<String>,
    #[arg(long, env = "ANKR_API_KEY")]
    ankr_api_key: Option<String>,
    #[arg(long, env = "ALCHEMY_API_KEY")]
    alchemy_api_key: Option<String>,
    #[arg(long, env = "INFURA_API_KEY")]
    infura_api_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RpcConfig(HashMap<ChainId, Vec<String>>);

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();

    let mut providers = vec![];

    if let Some(quicknode_api_key) = args.quicknode_api_key {
        providers.push(Box::new(QuicknodeProvider::new(quicknode_api_key)) as Box<dyn Provider>)
    } else {
        warn!("No quicknode api key provided");
    }

    if let Some(ankr_api_key) = args.ankr_api_key {
        providers.push(Box::new(AnkrProvider::new(ankr_api_key)) as Box<dyn Provider>)
    } else {
        warn!("No ankr api key provided");
    }

    if let Some(alchemy_api_key) = args.alchemy_api_key {
        providers.push(
            Box::new(providers::alchemy::AlchemyProvider::new(alchemy_api_key))
                as Box<dyn Provider>,
        )
    } else {
        warn!("No alchemy api key provided");
    }

    if let Some(infura_api_key) = args.infura_api_key {
        providers.push(
            Box::new(providers::infura::InfuraProvider::new(infura_api_key)) as Box<dyn Provider>,
        )
    } else {
        warn!("No infura api key provided");
    }

    let mut config: HashMap<ChainId, Vec<String>> = HashMap::new();

    if let Ok(config_file) = fs::read_to_string(args.config) {
        let data: RpcConfig = serde_yaml::from_str(&config_file)?;
        config.extend(data.0);
    } else {
        warn!("No config file provided");
    }

    for provider in providers {
        let endpoints = provider.fetch_endpoints().await?;
        for (chain_id, ep) in endpoints {
            config.entry(chain_id).or_default().push(ep);
        }
    }

    let client = reqwest::Client::new();
    for (chain_id, endpoints) in config.iter() {
        let statuses = healthcheck_many(&client, (*chain_id).into(), endpoints)
            .await
            .into_iter()
            .filter_map(|r| r.err())
            .collect::<Vec<_>>();
        if !statuses.is_empty() {
            error!("Failed to healthcheck {:?}: {:?}", chain_id, statuses);
            return Err(anyhow::anyhow!("Failed to healthcheck"));
        }
    }

    let config = {
        let mut data = HashMap::new();
        for (chain_id, endpoints) in config.iter_mut() {
            let len = endpoints.len();
            let threshold = if len > 3 { 3 } else { len };
            let validation_config = ChainValidationConfig {
                threshold,
                servers: endpoints.clone(),
            };
            data.insert(chain_id, validation_config);
        }
        data
    };

    let yaml = serde_yaml::to_string(&config)?;

    if let Some(parent) = Path::new(&args.output).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(&args.output, yaml.as_bytes())?;
    info!("Wrote merged RPC config to {}", args.output);

    Ok(())
}
