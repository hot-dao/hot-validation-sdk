use crate::api::AppState;
use crate::domain::errors::AppError;
use crate::domain::validate_and_sign;
use axum::Json;
use axum::extract::State;
use hot_validation_primitives::Base58;
use hot_validation_primitives::Base58Array;
use hot_validation_primitives::ProofModel;
use hot_validation_primitives::mpc::{KeyType, OffchainSignatureResponse};
use hot_validation_primitives::uid::Uid;
use serde::{Deserialize, Serialize};
use serde_with::hex::Hex;
use serde_with::serde_as;
use tracing::instrument;

#[derive(Deserialize)]
struct ProofRaw {
    message_body: String,
    user_payloads: Vec<serde_json::Value>,
}

impl From<ProofRaw> for ProofModel {
    fn from(value: ProofRaw) -> Self {
        Self {
            message_body: value.message_body,
            user_payloads: value.user_payloads.iter().map(|p| p.to_string()).collect(),
        }
    }
}

#[serde_as]
#[derive(Deserialize)]
pub(crate) struct SignRawRequest {
    #[serde_as(deserialize_as = "Hex")]
    uid: Uid,
    #[serde_as(as = "Hex")]
    message: Vec<u8>,
    proof: ProofRaw,
    #[serde(default = "SignRequest::default_key_type")]
    key_type: KeyType,
}

#[serde_as]
#[derive(Deserialize)]
pub(crate) struct SignRequest {
    #[serde_as(deserialize_as = "Base58Array<32>")]
    #[serde(rename = "wallet_derive")]
    uid: Uid,
    /// Hashed message that we want to sign
    #[serde_as(as = "Base58")]
    message: Vec<u8>,
    #[serde(flatten)]
    proof: ProofModel,
    #[serde(default = "SignRequest::default_key_type")]
    key_type: KeyType,
}

impl SignRequest {
    pub(crate) fn default_key_type() -> KeyType {
        KeyType::Ecdsa
    }
}

#[instrument(skip_all)]
pub(crate) async fn sign_raw(
    State(state): State<AppState>,
    Json(SignRawRequest { uid, message, proof, key_type }): Json<SignRawRequest>,
) -> Result<Json<OffchainSignatureResponse>, AppError> {
    let proof_model = ProofModel::from(proof);
    let signature = validate_and_sign(
        &state.cluster_manager,
        &state.validation,
        uid,
        message,
        proof_model,
        key_type,
    )
        .await?;
    Ok(Json(signature))
}

#[instrument(skip_all)]
pub(crate) async fn sign(
    State(state): State<AppState>,
    Json(SignRequest { uid, message, proof, key_type }): Json<SignRequest>,
) -> Result<Json<OffchainSignatureResponse>, AppError> {
    let signature = validate_and_sign(
        &state.cluster_manager,
        &state.validation,
        uid,
        message,
        proof,
        key_type,
    )
        .await?;
    Ok(Json(signature))
}
