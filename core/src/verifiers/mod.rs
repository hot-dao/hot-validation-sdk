use async_trait::async_trait;
use reqwest::Url;
use hot_validation_primitives::bridge::InputData;

pub mod evm;
pub mod near;
pub mod solana;
pub mod stellar;
pub mod ton;


#[async_trait]
pub trait Verifier: Sized + Send + Sync {
    async fn verify(
        &self,
        auth_contract_id: String,
        method_name: String,
        input_data: InputData,
    ) -> anyhow::Result<bool>;
}
