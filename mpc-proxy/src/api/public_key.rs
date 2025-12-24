use axum::extract::State;
use axum::Json;
use tracing::instrument;
use hot_validation_primitives::mpc::{PublicKeyRequest, PublicKeyResponse};
use crate::api::AppState;
use crate::domain::errors::AppError;

#[instrument(
    skip_all,
    err(Debug)
)]
pub(crate) async fn public_key_endpoint(
    State(state): State<AppState>,
    Json(public_key_request): Json<PublicKeyRequest>,
) -> Result<Json<PublicKeyResponse>, AppError> {
    let public_key = state.cluster_manager.get_public_key(public_key_request.uid).await?;
    Ok(Json(public_key))
}
