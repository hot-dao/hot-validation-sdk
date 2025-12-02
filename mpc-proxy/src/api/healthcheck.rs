use axum::Json;
use tracing::instrument;

#[derive(serde::Serialize)]
pub(crate) struct Health {
    status: &'static str,
}

#[instrument(skip_all)]
pub(crate) async fn healthcheck() -> Json<Health> {
    Json(Health { status: "ok" })
}
