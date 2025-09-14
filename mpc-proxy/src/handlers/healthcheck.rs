use actix_web::{get, web, Responder};
use actix_web::web::Json;
use serde_json::json;
use utoipa::ToSchema;

#[derive(serde::Serialize, ToSchema)]
struct Health { status: &'static str }

#[utoipa::path(
    description = "Health check",
    responses((status = OK, description = "Service is healthy", body = Health)),
    tag = "Health"
)]
#[get("/healthcheck")]
pub(crate) async fn healthcheck() -> Json<Health> {
    Json(Health { status: "ok" })
}
