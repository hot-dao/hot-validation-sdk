use crate::api::bridge::ClearCompletedWithdrawalRequest;
use crate::domain::errors::AppError;
use crate::domain::mpc::cluster::ClusterManager;
use crate::domain::validate_and_sign;
use hot_validation_core::Validation;
use hot_validation_primitives::bridge::CompletedWithdrawalAction;
use hot_validation_primitives::mpc::{KeyType, OffchainSignatureResponse};
use hot_validation_primitives::uid::Uid;
use std::sync::Arc;

pub(crate) async fn sign_clear_completed_withdrawal(
    uid: Uid,
    cluster_manager: &Arc<ClusterManager>,
    validation: &Arc<Validation>,
    completed_withdrawal_action: CompletedWithdrawalAction,
    key_type: KeyType,
) -> Result<OffchainSignatureResponse, AppError> {
    let challenge = completed_withdrawal_action
        .data
        .build_challenge_for_removal_owned()
        .to_vec();
    let proof_model =
        ClearCompletedWithdrawalRequest::create_proof_model(completed_withdrawal_action)?;
    validate_and_sign(
        cluster_manager,
        validation,
        uid,
        challenge,
        proof_model,
        key_type,
    )
    .await
}
