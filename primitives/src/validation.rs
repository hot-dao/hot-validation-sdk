use crate::ChainId;
use derive_more::{Deref, DerefMut, Into};
use serde::{Deserialize, Serialize};
use serde_valid::Validate;
use std::collections::HashMap;

/// Collection of arguments for each auth method.
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Eq, Hash)]
pub struct ProofModel {
    pub message_body: String,
    pub user_payloads: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Into, Deref, DerefMut)]
pub struct ValidationConfig(pub HashMap<ChainId, ChainValidationConfig>);

/// For a specific chain:
/// * `threshold` is the number of servers that need to give the same response to be able to accept it
/// * `servers` is the available RPCs
#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
#[validate(custom = validate_chain_config)]
pub struct ChainValidationConfig {
    pub threshold: usize,
    #[validate(unique_items)]
    pub servers: Vec<String>,
}

fn validate_chain_config(
    cfg: &ChainValidationConfig,
) -> anyhow::Result<(), serde_valid::validation::Error> {
    if cfg.threshold == 0 {
        return Err(serde_valid::validation::Error::Custom(
            "threshold must be >= 1".to_string(),
        ));
    }
    if cfg.servers.len() < cfg.threshold {
        return Err(serde_valid::validation::Error::Custom(format!(
            "Number of servers must be greater than or equal to threshold. Got {} servers and {} threshold.",
            cfg.servers.len(),
            cfg.threshold
        )));
    }
    // Require a strict majority: any value with fewer than `threshold` matching
    // votes cannot determine the result. `2 * threshold > servers.len()`
    // guarantees that at most one variant can ever reach threshold.
    if cfg.threshold * 2 <= cfg.servers.len() {
        return Err(serde_valid::validation::Error::Custom(format!(
            "threshold {} must be strict majority of {} servers (require 2*threshold > servers)",
            cfg.threshold,
            cfg.servers.len()
        )));
    }
    for server in &cfg.servers {
        if !server.starts_with("https://") {
            return Err(serde_valid::validation::Error::Custom(format!(
                "plaintext RPC endpoint is not allowed: {server}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(threshold: usize, servers: &[&str]) -> ChainValidationConfig {
        ChainValidationConfig {
            threshold,
            servers: servers.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    #[test]
    fn rejects_zero_threshold() {
        assert!(validate_chain_config(&cfg(0, &["https://a"])).is_err());
    }

    #[test]
    fn rejects_threshold_above_server_count() {
        assert!(validate_chain_config(&cfg(2, &["https://a"])).is_err());
    }

    #[test]
    fn rejects_non_majority_threshold() {
        // 3-of-7 is a minority — must be rejected
        let servers: Vec<&str> = (0..7).map(|_| "https://a").collect();
        assert!(validate_chain_config(&cfg(3, &servers)).is_err());
        // 3-of-6 is exactly half — also rejected (not *strict* majority)
        let servers: Vec<&str> = (0..6).map(|_| "https://a").collect();
        assert!(validate_chain_config(&cfg(3, &servers)).is_err());
    }

    #[test]
    fn accepts_strict_majority() {
        assert!(validate_chain_config(&cfg(1, &["https://a"])).is_ok());
        assert!(validate_chain_config(&cfg(2, &["https://a", "https://b"])).is_ok());
        assert!(validate_chain_config(&cfg(2, &["https://a", "https://b", "https://c"])).is_ok());
        assert!(validate_chain_config(&cfg(3, &["https://a", "https://b", "https://c", "https://d"])).is_ok());
    }

    #[test]
    fn rejects_plaintext_endpoint() {
        assert!(validate_chain_config(&cfg(1, &["http://a"])).is_err());
        assert!(validate_chain_config(&cfg(2, &["https://a", "http://b"])).is_err());
    }
}
