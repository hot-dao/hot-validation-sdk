use serde_with::DisplayFromStr;
use axum::extract::State;
use axum::Json;
use hot_validation_primitives::ExtendedChainId;
use hot_validation_primitives::bridge::{CompletedWithdrawal, CompletedWithdrawalAction, DepositAction, DepositData};
use serde_with::serde_as;
use tracing::instrument;
use hot_validation_primitives::mpc::KeyType;
use hot_validation_primitives::uid::Uid;
use crate::api::AppState;
use crate::api::sign::ProxySignatureResponse;
use crate::domain::bridge::clear_completed_withdrawal::sign_clear_completed_withdrawal;
use crate::domain::bridge::deposit::sign_deposit;
use crate::domain::bridge::withdrawal::sign_withdraw;
use crate::domain::errors::AppError;

#[serde_as]
#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub(crate) struct WithdrawRequest {
    #[serde_as(as = "DisplayFromStr")]
    pub nonce: u128,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub(crate) struct DepositRequest {
    #[serde(alias = "chain_from")]
    pub chain_id: ExtendedChainId,
    #[serde(flatten)]
    pub deposit_data: DepositData,
}

impl From<DepositRequest> for DepositAction {
    fn from(value: DepositRequest) -> Self {
        Self {
            chain_id: value.chain_id.into(),
            data: value.deposit_data,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub(crate) struct ClearCompletedWithdrawalRequest {
    #[serde(alias = "chain_from")]
    pub chain_id: ExtendedChainId,
    #[serde(flatten)]
    pub completed_withdrawal: CompletedWithdrawal,
}

#[instrument(
    skip(state),
    err(Debug),
)]
pub(crate) async fn sign_withdraw_endpoint(
    State(state): State<AppState>,
    Json(withdraw_request): Json<WithdrawRequest>,
) -> Result<Json<ProxySignatureResponse>, AppError> {
    let uid: Uid = state.secrets_config.uid_registry.bridge_withdraw.clone();
    let signature = sign_withdraw(
        uid,
        &state.cluster_manager,
        &state.validation,
        withdraw_request,
        KeyType::Ecdsa,
    ).await?;
    Ok(Json(signature.into()))
}

#[instrument(
    skip(state),
    err(Debug),
)]
pub(crate) async fn sign_deposit_endpoint(
    State(state): State<AppState>,
    Json(deposit_request): Json<DepositRequest>,
) -> Result<Json<ProxySignatureResponse>, AppError> {
    let uid: Uid = state.secrets_config.uid_registry.bridge_deposit.clone();
    let signature = sign_deposit(
        uid,
        &state.cluster_manager,
        &state.validation,
        deposit_request.into(),
        KeyType::Ecdsa,
    ).await?;
    Ok(Json(signature.into()))
}

#[instrument(
    skip(state),
    err(Debug),
)]
pub(crate) async fn clear_completed_withdrawal_endpoint(
    State(state): State<AppState>,
    Json(clear_completed_withdrawal_request): Json<ClearCompletedWithdrawalRequest>,
) -> Result<Json<ProxySignatureResponse>, AppError> {
    let clear_completed_withdrawal_request = enrich_with_receiver_address(clear_completed_withdrawal_request);
    let uid: Uid = state.secrets_config.uid_registry.bridge_deposit.clone();
    let signature = sign_clear_completed_withdrawal(
        uid,
        &state.cluster_manager,
        &state.validation,
        clear_completed_withdrawal_request.into(),
        KeyType::Ecdsa,
    ).await?;
    Ok(Json(signature.into()))
}

fn enrich_with_receiver_address(
    ClearCompletedWithdrawalRequest { chain_id, completed_withdrawal }: ClearCompletedWithdrawalRequest
) -> ClearCompletedWithdrawalRequest {
    ClearCompletedWithdrawalRequest {
        chain_id,
        completed_withdrawal: CompletedWithdrawal {
            nonce: completed_withdrawal.nonce,
            receiver_address: completed_withdrawal
                .receiver_address
                .or_else(|| Some("dontcare".to_string())),
        },
    }
}
