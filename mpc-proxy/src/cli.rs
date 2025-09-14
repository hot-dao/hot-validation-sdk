use clap::Parser;
use tracing::{info};

fn default_workers() -> usize {
    // use at least 2; auto-detect otherwise
    num_cpus::get().max(2)
}

#[derive(Debug, Parser, Clone)]
#[command(name = "actix-prod", about = "Actix Web prod server")]
pub struct Cli {
    /// Bind address (e.g., 0.0.0.0:8080)
    #[arg(long, env = "BIND_ADDR", default_value = "0.0.0.0:8080")]
    pub bind_addr: String,

    /// Number of worker threads
    #[arg(long, env = "WORKERS", default_value_t = default_workers())]
    pub workers: usize,

    /// TCP keep-alive seconds
    #[arg(long, env = "KEEP_ALIVE_SECS", default_value_t = 30u64)]
    pub keep_alive_secs: u64,

    /// Client request timeout (ms)
    #[arg(long, env = "CLIENT_TIMEOUT_MS", default_value_t = 10_000u64)]
    pub client_timeout_ms: u64,

    /// Client disconnect timeout (ms)
    #[arg(long, env = "CLIENT_DISCONNECT_TIMEOUT", default_value_t = 3_000u64)]
    pub client_disconnect_timeout: u64,

    /// Max accepted connection rate
    #[arg(long, env = "MAX_CONN_RATE", default_value_t = 256usize)]
    pub max_conn_rate: usize,
}