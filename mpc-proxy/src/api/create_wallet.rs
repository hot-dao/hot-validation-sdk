use serde_with::hex::Hex;
use std::str::FromStr;
use axum::extract::State;
use axum::Json;
use near_workspaces::{AccountId, CryptoHash, InMemorySigner};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use hot_validation_primitives::ChainId;
use hot_validation_primitives::uid::WalletId;
use crate::api::AppState;
use crate::domain::errors::AppError;

#[derive(Serialize, Deserialize)]
pub(crate) struct AuthMethod {
    msg: String,
    auth_account_id: String,
    metadata: Option<String>,
    chain_id: ChainId,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum CreateWalletProof {
    Signature {
        #[serde(rename = "wallet_derive_public_key")]
        public_key: (),
        signature: (),
    },
    Proof {
        proof: String
    }
}

#[derive(Deserialize)]
pub(crate) struct CreateWalletRequest {
    #[serde(rename = "auth")]
    auth_method: AuthMethod, // created wallet id will have this auth method
    wallet_id: WalletId,
    #[serde(flatten)]
    proof: CreateWalletProof, // proof, that we are allowed to create wallet with this id
    key_gen: u64,
}

#[derive(Serialize)]
pub(crate) struct OnChainArgs {
    access: AuthMethod,
    wallet_id: WalletId,
    key_gen: u64,
}

impl From<CreateWalletRequest> for OnChainArgs {
    fn from(value: CreateWalletRequest) -> Self {
        Self {
            access: value.auth_method,
            wallet_id: value.wallet_id,
            key_gen: value.key_gen,
        }
    }
}

#[serde_as]
#[derive(Serialize)]
pub(crate) struct CreateWalletResponse {
    #[serde_as(as = "Hex")]
    hash: [u8; 32]
}

pub(crate) async fn create_wallet_endpoint(
    State(state): State<AppState>,
    Json(request): Json<CreateWalletRequest>,
) -> Result<Json<CreateWalletResponse>, AppError> {
    let worker = near_workspaces::mainnet()
        .await
        .unwrap();
    let signer = InMemorySigner::from(state.secrets_config.near_registry_account.clone());

    // TODO: Validation

    let tx = worker.call(
        &signer,
        &AccountId::from_str("mpc.hot.tg").unwrap(),
        "create_wallet",
    )
        .args_json(OnChainArgs::from(request))
        .transact()
        .await
        .map_err(anyhow::Error::from)
        .map_err(AppError::NearSigner)?
        .into_result()
        .map_err(anyhow::Error::from)
        .map_err(AppError::NearSigner)?;

    let response = CreateWalletResponse {
        hash: tx.outcome().block_hash.0,
    };

    Ok(Json(response))
}
