pub(crate) mod bridge;
pub(crate) mod errors;
pub(crate) mod mpc;

use crate::api::bridge::{ClearCompletedWithdrawalRequest, DepositRequest, WithdrawRequest};
use crate::domain::errors::AppError;
use crate::domain::mpc::cluster::ClusterManager;
use anyhow::Context;
use hot_validation_core::Validation;
use hot_validation_primitives::ProofModel;
use hot_validation_primitives::bridge::{
    CompletedWithdrawalAction, DepositAction, HotVerifyBridge,
};
use hot_validation_primitives::mpc::{KeyType, OffchainSignatureResponse};
use hot_validation_primitives::uid::{Uid, WalletId};
use std::sync::Arc;
use tracing::instrument;

pub(crate) async fn validate_and_sign(
    cluster_manager: &Arc<ClusterManager>,
    validation: &Arc<Validation>,
    uid: Uid,
    message: Vec<u8>,
    proof_model: ProofModel,
    key_type: KeyType
) -> Result<OffchainSignatureResponse, AppError> {
    let wallet_id = WalletId::from(&uid);
    validation
        .verify(wallet_id, message.clone(), proof_model.clone())
        .await
        .map_err(AppError::ValidationError)?;

    let signature = cluster_manager
        .sign(uid, message, proof_model, key_type)
        .await?;
    Ok(signature)
}

impl ClearCompletedWithdrawalRequest {
    pub fn create_proof_model(
        completed_withdrawal_action: CompletedWithdrawalAction,
    ) -> Result<ProofModel, AppError> {
        let payload = HotVerifyBridge::ClearCompletedWithdrawal(completed_withdrawal_action);
        let json = serde_json::to_string(&payload)
            .map_err(anyhow::Error::from)
            .map_err(AppError::DataConversionError)?;
        let proof_model = ProofModel {
            message_body: String::new(),
            user_payloads: vec![json.to_string()],
        };
        Ok(proof_model)
    }
}

impl From<ClearCompletedWithdrawalRequest> for CompletedWithdrawalAction {
    fn from(value: ClearCompletedWithdrawalRequest) -> Self {
        Self {
            chain_id: value.chain_id.into(),
            data: value.completed_withdrawal,
        }
    }
}

impl DepositRequest {
    pub fn create_proof_model(deposit_action: DepositAction) -> Result<ProofModel, AppError> {
        let payload = HotVerifyBridge::Deposit(deposit_action);
        let json = serde_json::to_value(&payload)
            .map_err(anyhow::Error::from)
            .map_err(AppError::DataConversionError)?;
        let proof_model = ProofModel {
            message_body: String::new(),
            user_payloads: vec![json.to_string()],
        };
        Ok(proof_model)
    }
}

impl WithdrawRequest {
    pub fn create_proof_model(&self) -> ProofModel {
        let payload = self.nonce.to_string();
        ProofModel {
            message_body: String::new(),
            user_payloads: vec![payload],
        }
    }
}
