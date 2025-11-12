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
pub struct ChainValidationConfig {
    pub threshold: usize,
    #[validate(unique_items)]
    pub servers: Vec<String>,
}

fn validate_chain_config(
    cfg: &ChainValidationConfig,
) -> anyhow::Result<(), serde_valid::validation::Error> {
    if cfg.servers.len() < cfg.threshold {
        return Err(serde_valid::validation::Error::Custom(format!(
            "Number of servers must be greater than or equal to threshold. Got {} servers and {} threshold.",
            cfg.servers.len(),
            cfg.threshold
        )));
    }
    Ok(())
}
