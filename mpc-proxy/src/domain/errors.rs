use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use derive_more::{Display, Error};
use tracing::error;

#[derive(Debug, Error, Display)]
pub(crate) enum AppError {
    DataConversionError(anyhow::Error),
    ValidationError(anyhow::Error),
    InitializationError(anyhow::Error),
    MpcError(anyhow::Error),
    NearSigner(anyhow::Error),
    OsError(anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = StatusCode::INTERNAL_SERVER_ERROR;
        (status, format!("{:#}", self)).into_response()
    }
}
