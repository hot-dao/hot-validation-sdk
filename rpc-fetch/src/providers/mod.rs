use async_trait::async_trait;
use hot_validation_primitives::ExtendedChainId;
use std::collections::HashMap;

pub mod alchemy;
pub mod ankr;
pub mod infura;
pub mod quicknode;

#[async_trait]
pub trait Provider {
    async fn fetch_endpoints(&self) -> anyhow::Result<HashMap<ExtendedChainId, String>>;
}

pub trait SlugFromChainId {
    fn slug(chain_id: ExtendedChainId) -> Option<String>;
}
