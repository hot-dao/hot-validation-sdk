use axum::Json;

#[derive(serde::Serialize)]
pub(crate) struct Health {
    status: &'static str,
}

pub(crate) async fn healthcheck() -> Json<Health> {
    Json(Health { status: "ok" })
}
