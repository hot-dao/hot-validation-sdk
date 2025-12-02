use axum::Json;
use hot_validation_core::integer::U128String;
use hot_validation_primitives::ExtendedChainId;
use hot_validation_primitives::bridge::{CompletedWithdrawal, DepositData};
use serde_with::serde_as;
use tracing::instrument;

#[serde_as]
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct WithdrawRequest {
    #[serde_as(as = "U128String")]
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
pub(crate) async fn sign_withdraw(withdraw_request: Json<WithdrawRequest>) -> Json<String> {
    Json(String::from("Ok"))
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

#[cfg(test)]
mod tests {
    use std::str;

    use anyhow::{Result, bail};
    use axum::body::to_bytes;
    use axum::{
        Router,
        body::Body,
        http::{self, Request},
        routing::post,
    };
    use serde_json::json;
    use tower::ServiceExt;

    use crate::api::bridge::{sign_deposit, sign_withdraw};

    fn test_app() -> Router {
        Router::new()
            .route("/deposit/sign", post(sign_deposit))
            .route("/withdraw/sign", post(sign_withdraw))
    }

    #[tokio::test]
    async fn test_sign_deposit() -> Result<()> {
        let app = test_app();

        let payload = json!({
            "chain_from": 56,
            "nonce": "1754431900000000013182",
        });

        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/deposit/sign")
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(payload.to_string()))?;

        let resp = app.clone().oneshot(req).await?;
        let status = resp.status();
        let body_bytes = to_bytes(resp.into_body(), usize::MAX).await?;

        if !status.is_success() {
            let message = str::from_utf8(&body_bytes).unwrap_or("<non-utf8 body>");
            bail!(
                "Request failed with status: {} and message `{}`",
                status,
                message
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_sign_withdraw() -> Result<()> {
        let app = test_app();

        let payload = json!({
            "nonce": "1754631474000000070075",
        });

        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/withdraw/sign")
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(payload.to_string()))?;

        let resp = app.clone().oneshot(req).await?;
        let status = resp.status();
        let body_bytes = to_bytes(resp.into_body(), usize::MAX).await?;

        if !status.is_success() {
            let message = str::from_utf8(&body_bytes).unwrap_or("<non-utf8 body>");
            bail!(
                "Request failed with status: {} and message `{}`",
                status,
                message
            );
        }

        Ok(())
    }
}
