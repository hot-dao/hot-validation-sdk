use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use serde_valid::Validate;
use crate::ChainId;

/// Collection of arguments for each auth method.
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Eq, Hash)]
pub struct ProofModel {
    pub message_body: String,
    pub user_payloads: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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
    if cfg.threshold <= cfg.servers.len() / 2 {
        return Err(serde_valid::validation::Error::Custom(
            "threshold must be greater than half of servers.len()".into(),
        ));
    }
    Ok(())
}
