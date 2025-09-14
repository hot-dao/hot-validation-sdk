use crate::handlers::healthcheck::__path_healthcheck;
use crate::handlers::telemetry::__path_prometheus_metrics;
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(paths(
    healthcheck,
    prometheus_metrics
))]
pub struct ApiDoc;
