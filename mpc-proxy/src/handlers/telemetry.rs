use actix_web::get;
use prometheus::{Encoder, TextEncoder, default_registry};

#[utoipa::path(description = "Prometheus metrics", tag = "Telemetry")]
#[get("/prometheus-metrics")]
pub(crate) async fn prometheus_metrics() -> String {
    let metric_families = default_registry().gather();
    let mut buffer = vec![];
    let encoder = TextEncoder::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}
