use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser, Clone)]
#[command()]
pub struct Cli {
    #[arg(long, env)]
    pub port: u16,

    #[arg(long, env)]
    pub encrypted_config_path: PathBuf,

    #[arg(long, env)]
    pub validation_config_path: PathBuf,

    #[arg(long, env)]
    pub cluster_config_path: PathBuf,

    #[arg(long, env)]
    pub alloy_endpoint: String,

    #[cfg(feature = "debug")]
    #[arg(long, env)]
    pub encryption_key_path: PathBuf,
}
