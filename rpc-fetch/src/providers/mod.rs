use crate::supported_chains::ChainId;
use async_trait::async_trait;
use std::collections::HashMap;

pub mod alchemy;
pub mod ankr;
pub mod infura;
pub mod quicknode;

#[async_trait]
pub trait Provider {
    async fn fetch_endpoints(&self) -> anyhow::Result<HashMap<ChainId, String>>;
}
