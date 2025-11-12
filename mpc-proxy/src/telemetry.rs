use anyhow::Result;
use opentelemetry::trace::TracerProvider;
use opentelemetry::KeyValue;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::Sampler;
use opentelemetry_sdk::Resource;
use std::sync::LazyLock;
use tracing_error::ErrorLayer;
use tracing_subscriber::fmt::time::UtcTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter, Layer};

const ALLOY_ENDPOINT: &str = "http://alloy:4317";

static RESOURCE: LazyLock<Resource> = LazyLock::new(|| {
    Resource::builder()
        .with_attributes(vec![
            KeyValue::new("service.namespace", "hot-labs"),
            KeyValue::new("service.name", env!("CARGO_PKG_NAME")),
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
        ])
        .build()
});

pub struct TelemetryGuard {
    log_provider: opentelemetry_sdk::logs::SdkLoggerProvider,
    trace_provider: opentelemetry_sdk::trace::SdkTracerProvider,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        self.log_provider
            .shutdown()
            .expect("failed to shutdown log provider");
        self.trace_provider
            .shutdown()
            .expect("failed to shutdown trace provider");
    }
}

fn init_logs() -> Result<opentelemetry_sdk::logs::SdkLoggerProvider> {
    use opentelemetry_sdk::logs::SdkLoggerProvider;

    let exporter = opentelemetry_otlp::LogExporter::builder()
        .with_tonic()
        .with_endpoint(ALLOY_ENDPOINT)
        .build()?;

    let provider = SdkLoggerProvider::builder()
        .with_resource(RESOURCE.clone())
        .with_batch_exporter(exporter)
        .build();

    Ok(provider)
}

fn init_traces() -> Result<opentelemetry_sdk::trace::SdkTracerProvider> {
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(ALLOY_ENDPOINT)
        .build()?;

    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_resource(RESOURCE.clone())
        .with_batch_exporter(exporter)
        .with_sampler(Sampler::AlwaysOn)
        .build();

    Ok(provider)
}

pub fn init_telemetry() -> Result<TelemetryGuard> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("info,axum::rejection=trace,tower_http=info,hyper=warn,h2=warn") // TODO: revise
    });

    let fmt_layer = fmt::layer()
        .json()
        .with_timer(UtcTime::rfc_3339())
        .with_target(true)
        .with_current_span(true)
        .with_span_list(true);

    let log_provider = init_logs()?;
    let otel_log_layer = OpenTelemetryTracingBridge::new(&log_provider).boxed();

    let trace_provider = init_traces()?;
    let otel_trace_layer = tracing_opentelemetry::layer()
        .with_tracer(trace_provider.tracer(env!("CARGO_PKG_NAME")))
        .boxed();

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(otel_log_layer)
        .with(otel_trace_layer)
        .init();

    Ok(TelemetryGuard { log_provider, trace_provider })
}
