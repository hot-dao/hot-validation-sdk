use crate::domain::WithdrawRequest;
use crate::domain::mpc::cluster::ClusterManager;
use crate::secrets::UidRegistry;
use anyhow::Result;
use hot_validation_core::Validation;
use hot_validation_primitives::bridge::DepositAction;
use hot_validation_primitives::mpc::{KeyType, OffchainSignatureResponse};
use hot_validation_primitives::uid::Uid;
use serde_json::json;
use std::sync::Arc;
use tracing::instrument;
use crate::domain::errors::AppError;


#[instrument(
    skip(validation),
    err(Debug)
)]
async fn get_withdrawal(
    validation: &Arc<Validation>,
    nonce: u128,
) -> Result<Option<DepositAction>> {
    let contract = "v2_1.omni.hot.tg";
    let method = "get_transfer";
    let args = json!({ "nonce": nonce.to_string() });
    let result: Option<DepositAction> = validation
        .near
        .call_view_method(contract.to_string(), method.to_string(), args)
        .await?;
    Ok(result)
}

/// We don't need to do `validation.verify()` here, because it will check the state of NEAR bridge,
/// but we've formed our data by reading the state of NEAR bridge.
pub(crate) async fn sign_withdraw(
    uid: Uid,
    cluster_manager: &Arc<ClusterManager>,
    validation: &Arc<Validation>,
    withdrawal_request: WithdrawRequest,
    key_type: KeyType,
) -> Result<OffchainSignatureResponse, AppError> {
    let withdrawal = get_withdrawal(validation, withdrawal_request.nonce)
        .await
        .map_err(AppError::ValidationError)?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "withdrawal {} not found on omni bridge",
                withdrawal_request.nonce
            )
        })
        .map_err(AppError::ValidationError)?;
    let challenge = withdrawal
        .build_challenge_for_deposit()
        .map_err(AppError::ValidationError)?
        .to_vec();
    let proof_model = withdrawal_request.create_proof_model();
    let signature = cluster_manager
        .sign(uid, challenge, proof_model, key_type)
        .await?;
    Ok(signature)
}

#[cfg(test)]
mod tests {
    use crate::domain::bridge::withdrawal::get_withdrawal;
    use anyhow::Result;
    use hot_validation_core::test_data::create_validation_object;
    use hot_validation_primitives::ChainId;
    use hot_validation_primitives::bridge::{DepositAction, DepositData};

    #[tokio::test]
    async fn test_get_withdrawal() -> Result<()> {
        let validation = create_validation_object();
        let nonce = 1_749_390_032_000_000_032_243_u128;
        let expected = DepositAction {
            chain_id: ChainId::Solana,
            data: DepositData {
                sender: None,
                receiver: Some(bs58::decode("5eMysQ7ywu4D8pmN5RtDoPxbu5YbiEThQy8gaBcmMoho")
                    .into_vec()?
                    .try_into()
                    .unwrap()),
                token_id: Some(vec![206, 1, 14, 96, 175, 237, 178, 39, 23, 189, 99, 25, 47, 84, 20, 90, 63, 150, 90, 51, 187, 130, 210, 199, 2, 158, 178, 206, 30, 32, 130, 100]),
                amount: Some(998_289),
                nonce: 1_749_390_032_000_000_032_243_u128,
            },
        };
        let opt_withdrawal = get_withdrawal(&validation, nonce).await?;
        let actual = opt_withdrawal.expect("withdrawal not found");
        dbg!(&actual);
        assert_eq!(actual, expected);
        Ok(())
    }
}
