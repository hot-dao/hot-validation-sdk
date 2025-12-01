use serde_with::hex::Hex;
use hot_validation_core::uid::HexOrBase58;
use crate::api::AppState;
use crate::domain::errors::AppError;
use crate::domain::validate_and_sign;
use axum::Json;
use axum::extract::State;
use hot_validation_primitives::ProofModel;
use hot_validation_primitives::mpc::{KeyType, OffchainSignatureResponse};
use hot_validation_primitives::uid::Uid;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use tracing::instrument;

#[serde_as]
#[derive(Serialize, Deserialize)]
pub(crate) struct SignRawRequest {
    #[serde_as(deserialize_as = "HexOrBase58", serialize_as = "Hex")]
    uid: Uid,
    message: String,
    proof: ProofModel,
    #[serde(default = "SignRequest::default_key_type")]
    key_type: KeyType,
}

#[serde_as]
#[derive(Serialize, Deserialize)]
pub(crate) struct SignRequest {
    #[serde_as(deserialize_as = "HexOrBase58", serialize_as = "Hex")]
    wallet_derive: Uid,
    /// Hashed message that we want to sign
    message: String,
    /// Image of the message
    message_body: String,
    user_payloads: Vec<String>,
    #[serde(default = "SignRequest::default_key_type")]
    key_type: KeyType,
}

impl SignRequest {
    pub(crate) fn default_key_type() -> KeyType {
        KeyType::Ecdsa
    }
}

pub(crate) async fn sign_raw(sign_raw_request: Json<SignRawRequest>) -> Json<String> {
    Json(String::from("Ok"))
}

#[instrument(skip(state, sign_request), err(Debug))]
pub(crate) async fn sign(
    State(state): State<AppState>,
    Json(sign_request): Json<SignRequest>,
) -> Result<Json<OffchainSignatureResponse>, AppError> {
    let proof_model = ProofModel {
        message_body: sign_request.message_body,
        user_payloads: sign_request.user_payloads,
    };
    let signature = validate_and_sign(
        &state.cluster_manager,
        &state.validation,
        sign_request.wallet_derive,
        sign_request.message,
        proof_model,
    )
    .await?;
    Ok(Json(signature))
}
