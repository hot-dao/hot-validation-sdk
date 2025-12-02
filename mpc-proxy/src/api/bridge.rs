use serde_with::DisplayFromStr;
use axum::extract::State;
use axum::Json;
use hot_validation_primitives::ExtendedChainId;
use hot_validation_primitives::bridge::{CompletedWithdrawal, DepositData};
use serde_with::serde_as;
use tracing::instrument;
use hot_validation_primitives::mpc::KeyType;
use hot_validation_primitives::uid::Uid;
use crate::api::AppState;
use crate::api::sign::ProxySignatureResponse;
use crate::domain::bridge::withdrawal::sign_withdrawal;
use crate::domain::errors::AppError;

#[serde_as]
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct WithdrawRequest {
    #[serde_as(as = "DisplayFromStr")]
    pub nonce: u128,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct DepositRequest {
    #[serde(alias = "chain_from")]
    pub chain_id: ExtendedChainId,
    #[serde(flatten)]
    pub deposit_data: DepositData,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct ClearCompletedWithdrawalRequest {
    #[serde(alias = "chain_from")]
    pub chain_id: ExtendedChainId,
    #[serde(flatten)]
    pub completed_withdrawal: CompletedWithdrawal,
}

#[instrument(skip_all)]
pub(crate) async fn sign_withdraw(
    State(state): State<AppState>,
    Json(withdraw_request): Json<WithdrawRequest>,
) -> Result<Json<ProxySignatureResponse>, AppError> {
    let uid: Uid = state.secrets_config.uid_registry.get_bridge_withdrawal();
    let signature = sign_withdrawal(
        uid,
        &state.cluster_manager,
        &state.validation,
        withdraw_request,
        KeyType::Ecdsa,
    ).await?;
    Ok(Json(signature.into()))
}

#[instrument(skip_all)]
pub(crate) async fn sign_deposit(deposit_request: Json<DepositRequest>) -> Json<String> {
    Json(String::from("Ok"))
}

#[instrument(skip_all)]
pub(crate) async fn clear_completed_withdrawal(
    clear_completed_withdrawal_request: Json<ClearCompletedWithdrawalRequest>,
) -> Json<String> {
    Json(String::from("Ok"))
}
