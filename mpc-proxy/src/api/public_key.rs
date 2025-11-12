use axum::Json;
use hot_validation_primitives::mpc::{PublicKeyRequest, PublicKeyResponse};

pub(crate) async fn public_key(
    public_key_request: Json<PublicKeyRequest>,
) -> Json<PublicKeyResponse> {
    todo!()
}
