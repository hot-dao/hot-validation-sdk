use crate::domain::errors::AppError;
use crate::domain::mpc::cluster::ClusterManager;
use crate::domain::{DepositRequest, validate_and_sign};
use crate::secrets::UidRegistry;
use hot_validation_core::Validation;
use hot_validation_primitives::bridge::DepositAction;
use hot_validation_primitives::mpc::OffchainSignatureResponse;
use std::sync::Arc;

pub(crate) async fn sign_deposit(
    uid_registry: &Arc<UidRegistry>,
    cluster_manager: &Arc<ClusterManager>,
    validation: &Arc<Validation>,
    deposit_action: DepositAction,
) -> Result<OffchainSignatureResponse, AppError> {
    let uid = uid_registry.get_bridge_deposit();
    let challenge = deposit_action
        .build_challenge_for_deposit()
        .map_err(AppError::DataConversionError)?
        .to_vec();
    let proof_model = DepositRequest::create_proof_model(deposit_action)?;
    validate_and_sign(cluster_manager, validation, uid, challenge, proof_model).await
}
