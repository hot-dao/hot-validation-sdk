use crate::api::AppState;
use crate::domain::errors::AppError;
use crate::domain::validate_and_sign;
use axum::Json;
use axum::extract::State;
use hot_validation_primitives::Base58;
use hot_validation_primitives::Base58Array;
use hot_validation_primitives::ProofModel;
use hot_validation_primitives::mpc::{k256, KeyType, OffchainSignatureResponse};
use hot_validation_primitives::uid::Uid;
use serde::{Deserialize, Serialize};
use serde_with::hex::Hex;
use serde_with::serde_as;
use tracing::instrument;
use hot_validation_primitives::mpc::cait_sith::frost_ed25519;
use hot_validation_primitives::mpc::cait_sith::frost_ed25519::VerifyingKey;
use hot_validation_primitives::mpc::k256::elliptic_curve::sec1::ToEncodedPoint;

#[derive(Deserialize, Debug)]
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

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ProxySignatureResponse {
    Ecdsa {
        r: String,
        s: k256::Scalar,
    },
    Eddsa {
        signature: frost_ed25519::Signature,
        public_key: VerifyingKey,
    },
}

impl From<OffchainSignatureResponse> for ProxySignatureResponse {
    fn from(value: OffchainSignatureResponse) -> Self {
        match value {
            OffchainSignatureResponse::Ecdsa { big_r, signature, .. } => {
                Self::Ecdsa {
                    r: big_r.to_encoded_point(true).to_string()[2..].to_string(),
                    s: signature,
                }
            }
            OffchainSignatureResponse::Eddsa { signature, public_key, .. } => {
                Self::Eddsa { signature, public_key }
            }
        }
    }
}

#[instrument(
    skip(state, uid),
    err(Debug)
)]
pub(crate) async fn sign_raw_endpoint(
    State(state): State<AppState>,
    Json(SignRawRequest { uid, message, proof, key_type }): Json<SignRawRequest>,
) -> Result<Json<ProxySignatureResponse>, AppError> {
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
    Ok(Json(signature.into()))
}

#[instrument(
    skip(state, uid),
    err(Debug)
)]
pub(crate) async fn sign_endpoint(
    State(state): State<AppState>,
    Json(SignRequest { uid, message, proof, key_type }): Json<SignRequest>,
) -> Result<Json<ProxySignatureResponse>, AppError> {
    let signature = validate_and_sign(
        &state.cluster_manager,
        &state.validation,
        uid,
        message,
        proof,
        key_type,
    )
        .await?;
    Ok(Json(signature.into()))
}
