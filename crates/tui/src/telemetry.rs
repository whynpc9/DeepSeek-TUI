//! OpenTelemetry / OTLP wiring for LLM tracing.
//!
//! This module owns the **process-wide telemetry pipeline** for the TUI:
//!
//! 1. Reads `[telemetry]` from `config.toml` and standard `OTEL_*` env vars.
//! 2. When enabled, builds an OTLP `SpanExporter` (HTTP/protobuf by default,
//!    gRPC opt-in) and a [`SdkTracerProvider`].
//! 3. Layers the OTel tracer into the global `tracing` subscriber via
//!    [`tracing_opentelemetry::OpenTelemetryLayer`]; without this layer the
//!    `#[tracing::instrument]` spans on the LLM client would never reach the
//!    collector.
//! 4. Returns a [`TelemetryGuard`] whose `Drop` flushes pending spans, so
//!    short-lived `deepseek exec`/`deepseek doctor` invocations don't lose
//!    their last span batch.
//!
//! The recommended sink is a self-hosted **SigNoz** stack — see
//! `docs/OPENTELEMETRY.md` for the Docker compose recipe. Any OTLP-compatible
//! backend (Jaeger, Tempo, Honeycomb, Datadog OTLP, Phoenix, Langfuse OTel
//! ingest, ...) works without code changes.
//!
//! ## GenAI semantic conventions
//!
//! Spans use the experimental [GenAI semantic conventions][semconv] so the
//! traces light up SigNoz's "LLM" view and any downstream APM that
//! understands `gen_ai.*` attributes:
//!
//! - `gen_ai.system` — provider id (`deepseek`, `nvidia-nim`, ...)
//! - `gen_ai.operation.name` — `chat` / `chat.stream`
//! - `gen_ai.request.model` / `gen_ai.response.model`
//! - `gen_ai.usage.input_tokens` / `gen_ai.usage.output_tokens`
//! - `gen_ai.response.finish_reasons`
//!
//! Prompt / completion text is **redacted by default**
//! (`telemetry.redact_content = true`). Flip it off only on trusted local
//! collectors; SigNoz stores spans verbatim and prompts often contain
//! workspace contents.
//!
//! [semconv]: https://opentelemetry.io/docs/specs/semconv/gen-ai/

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context as _, Result};
use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{Protocol, SpanExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::config::TelemetryConfig;

/// Default OTLP/HTTP endpoint exposed by a stock SigNoz Docker deployment.
pub const DEFAULT_OTLP_HTTP_ENDPOINT: &str = "http://localhost:4318";
/// Default OTLP/gRPC endpoint exposed by a stock SigNoz Docker deployment.
pub const DEFAULT_OTLP_GRPC_ENDPOINT: &str = "http://localhost:4317";
/// Default `service.name` resource attribute.
pub const DEFAULT_SERVICE_NAME: &str = "deepseek-tui";
/// Default tracing filter when `RUST_LOG` is unset.
pub const DEFAULT_LOG_FILTER: &str = "info,deepseek_tui=info,deepseek_core=info";

/// Whether telemetry was initialized on this process.
static TELEMETRY_ENABLED: OnceLock<bool> = OnceLock::new();
/// Whether prompt / completion text should be omitted from spans.
static REDACT_CONTENT: OnceLock<bool> = OnceLock::new();

/// Public check used by the LLM client to decide whether to populate
/// `gen_ai.prompt.*` / `gen_ai.completion.*` span events. Defaults to `true`
/// (redact) when telemetry is not initialized — callers can short-circuit
/// expensive serialization in the hot path.
#[allow(dead_code)]
#[must_use]
pub fn redact_content() -> bool {
    *REDACT_CONTENT.get().unwrap_or(&true)
}

/// True iff `init` succeeded with an enabled config on this process.
#[allow(dead_code)]
#[must_use]
pub fn is_enabled() -> bool {
    *TELEMETRY_ENABLED.get().unwrap_or(&false)
}

/// Resolved runtime settings derived from `TelemetryConfig` + env vars.
#[derive(Debug, Clone)]
pub struct TelemetrySettings {
    pub enabled: bool,
    pub endpoint: String,
    pub protocol: OtlpProtocol,
    pub service_name: String,
    pub environment: Option<String>,
    pub headers: HashMap<String, String>,
    pub redact_content: bool,
    pub log_filter: String,
}

/// Selected OTLP transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtlpProtocol {
    /// OTLP/HTTP with binary protobuf body (default; SigNoz HTTP port 4318).
    HttpProtobuf,
    /// OTLP/gRPC over h2 (SigNoz gRPC port 4317). Requires the `grpc-tonic`
    /// feature, which is **not** compiled in by default — falls back to HTTP
    /// with a warning when unavailable.
    Grpc,
}

impl OtlpProtocol {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "http" | "http/protobuf" | "http_protobuf" | "httpproto" | "http-protobuf" => {
                Some(Self::HttpProtobuf)
            }
            "grpc" | "otlp-grpc" | "tonic" => Some(Self::Grpc),
            _ => None,
        }
    }

    fn as_label(self) -> &'static str {
        match self {
            Self::HttpProtobuf => "http/protobuf",
            Self::Grpc => "grpc",
        }
    }

    fn default_endpoint(self) -> &'static str {
        match self {
            Self::HttpProtobuf => DEFAULT_OTLP_HTTP_ENDPOINT,
            Self::Grpc => DEFAULT_OTLP_GRPC_ENDPOINT,
        }
    }
}

impl TelemetrySettings {
    /// Merge file config with `OTEL_*` / `DEEPSEEK_OTEL_*` env vars.
    /// Env always wins. Unknown protocol strings fall back to HTTP.
    #[must_use]
    pub fn resolve(config: Option<&TelemetryConfig>) -> Self {
        let cfg_enabled = config.and_then(|c| c.enabled).unwrap_or(false);
        let env_enabled = std::env::var("DEEPSEEK_OTEL_ENABLED")
            .ok()
            .map(|v| parse_bool(&v))
            .unwrap_or(None);
        let enabled = env_enabled.unwrap_or(cfg_enabled);

        let protocol = std::env::var("OTEL_EXPORTER_OTLP_PROTOCOL")
            .ok()
            .as_deref()
            .and_then(OtlpProtocol::parse)
            .or_else(|| {
                config
                    .and_then(|c| c.protocol.as_deref())
                    .and_then(OtlpProtocol::parse)
            })
            .unwrap_or(OtlpProtocol::HttpProtobuf);

        let endpoint = std::env::var("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT")
            .ok()
            .or_else(|| std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok())
            .or_else(|| config.and_then(|c| c.endpoint.clone()))
            .unwrap_or_else(|| protocol.default_endpoint().to_string());

        let service_name = std::env::var("OTEL_SERVICE_NAME")
            .ok()
            .or_else(|| config.and_then(|c| c.service_name.clone()))
            .unwrap_or_else(|| DEFAULT_SERVICE_NAME.to_string());

        let environment = std::env::var("DEEPSEEK_OTEL_ENVIRONMENT")
            .ok()
            .or_else(|| config.and_then(|c| c.environment.clone()));

        let mut headers = config.and_then(|c| c.headers.clone()).unwrap_or_default();
        if let Ok(raw) = std::env::var("OTEL_EXPORTER_OTLP_HEADERS") {
            for entry in raw.split(',') {
                let entry = entry.trim();
                if entry.is_empty() {
                    continue;
                }
                if let Some((k, v)) = entry.split_once('=') {
                    headers.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
        }

        let redact_content = std::env::var("DEEPSEEK_OTEL_REDACT_CONTENT")
            .ok()
            .and_then(|v| parse_bool(&v))
            .or_else(|| config.and_then(|c| c.redact_content))
            .unwrap_or(true);

        let log_filter = std::env::var("RUST_LOG")
            .ok()
            .or_else(|| config.and_then(|c| c.log_filter.clone()))
            .unwrap_or_else(|| DEFAULT_LOG_FILTER.to_string());

        Self {
            enabled,
            endpoint,
            protocol,
            service_name,
            environment,
            headers,
            redact_content,
            log_filter,
        }
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" | "enabled" | "y" => Some(true),
        "0" | "false" | "no" | "off" | "disabled" | "n" => Some(false),
        _ => None,
    }
}

/// RAII handle that flushes pending spans on drop. Hold this for the
/// lifetime of `main()`; dropping it triggers a synchronous
/// `SdkTracerProvider::shutdown` so short-lived CLI runs don't lose data.
pub struct TelemetryGuard {
    provider: Option<SdkTracerProvider>,
    #[allow(dead_code)]
    enabled: bool,
}

impl TelemetryGuard {
    /// Lightweight guard used when telemetry is disabled — `Drop` is a no-op.
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            provider: None,
            enabled: false,
        }
    }

    /// True if a real exporter is attached.
    #[allow(dead_code)]
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Some(provider) = self.provider.take() {
            // Best-effort flush; we'd rather log a warning than abort cleanup.
            if let Err(err) = provider.shutdown() {
                crate::logging::warn(format!("OpenTelemetry shutdown error: {err}"));
            }
        }
    }
}

/// Initialize telemetry from resolved settings.
///
/// Returns `Ok(TelemetryGuard::disabled())` when `settings.enabled` is false,
/// so callers can unconditionally `let _guard = telemetry::init(...)?;`.
///
/// Failures during exporter construction are **not fatal** — we log a warning
/// and return a disabled guard so the agent keeps working without observability.
pub fn init(settings: TelemetrySettings) -> Result<TelemetryGuard> {
    if !settings.enabled {
        // Still install a tracing subscriber so `tracing::info!` etc. flow to
        // stderr at the configured filter level; otherwise the project's
        // existing `tracing::warn!` calls would be black-holed.
        install_fmt_only_subscriber(&settings.log_filter);
        let _ = TELEMETRY_ENABLED.set(false);
        let _ = REDACT_CONTENT.set(true);
        return Ok(TelemetryGuard::disabled());
    }

    let provider = match build_tracer_provider(&settings) {
        Ok(p) => p,
        Err(err) => {
            crate::logging::warn(format!(
                "OpenTelemetry exporter init failed ({err}); continuing without tracing. \
                 endpoint={} protocol={}",
                settings.endpoint,
                settings.protocol.as_label(),
            ));
            install_fmt_only_subscriber(&settings.log_filter);
            let _ = TELEMETRY_ENABLED.set(false);
            let _ = REDACT_CONTENT.set(true);
            return Ok(TelemetryGuard::disabled());
        }
    };

    // Install both an OTLP layer (forwards `tracing` spans to the OTLP exporter)
    // and a fmt layer (keeps existing stderr behavior). Wrapping in `try_init`
    // makes this idempotent for tests that may load the subscriber twice.
    global::set_tracer_provider(provider.clone());
    let tracer = provider.tracer(settings.service_name.clone());
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    let filter = tracing_subscriber::EnvFilter::try_new(&settings.log_filter)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(DEFAULT_LOG_FILTER));
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(false)
        .compact();

    if tracing_subscriber::registry()
        .with(filter)
        .with(otel_layer)
        .with(fmt_layer)
        .try_init()
        .is_err()
    {
        crate::logging::info("tracing subscriber already initialized; OTel layer will not attach.");
    }

    let _ = TELEMETRY_ENABLED.set(true);
    let _ = REDACT_CONTENT.set(settings.redact_content);
    crate::logging::info(format!(
        "OpenTelemetry initialized: endpoint={} protocol={} service={} redact={}",
        settings.endpoint,
        settings.protocol.as_label(),
        settings.service_name,
        settings.redact_content,
    ));

    Ok(TelemetryGuard {
        provider: Some(provider),
        enabled: true,
    })
}

/// Build an OTLP `SpanExporter` + batch-backed `SdkTracerProvider`.
fn build_tracer_provider(settings: &TelemetrySettings) -> Result<SdkTracerProvider> {
    let exporter = build_span_exporter(settings)?;

    let mut resource_attrs = vec![
        KeyValue::new(
            opentelemetry_semantic_conventions::resource::SERVICE_NAME,
            settings.service_name.clone(),
        ),
        KeyValue::new(
            opentelemetry_semantic_conventions::resource::SERVICE_VERSION,
            env!("CARGO_PKG_VERSION"),
        ),
        KeyValue::new(
            opentelemetry_semantic_conventions::resource::TELEMETRY_SDK_LANGUAGE,
            "rust",
        ),
    ];
    if let Some(env) = settings.environment.as_ref() {
        resource_attrs.push(KeyValue::new(
            opentelemetry_semantic_conventions::resource::DEPLOYMENT_ENVIRONMENT_NAME,
            env.clone(),
        ));
    }
    let resource = Resource::builder().with_attributes(resource_attrs).build();

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();
    Ok(provider)
}

fn build_span_exporter(settings: &TelemetrySettings) -> Result<SpanExporter> {
    // We only compile the HTTP/protobuf transport by default to keep the
    // dependency tree slim (no `protoc` build step). Users who set
    // `protocol = "grpc"` get a warning and HTTP fallback.
    if settings.protocol == OtlpProtocol::Grpc {
        crate::logging::warn(
            "OTLP gRPC transport is not compiled in this build; falling back to HTTP/protobuf. \
             Rebuild with the `grpc-tonic` feature on opentelemetry-otlp to enable.",
        );
    }
    let endpoint = http_traces_endpoint(&settings.endpoint);
    let mut builder = SpanExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .with_endpoint(endpoint)
        .with_timeout(Duration::from_secs(10));
    if !settings.headers.is_empty() {
        builder = builder.with_headers(settings.headers.clone());
    }
    builder
        .build()
        .context("failed to build OTLP HTTP span exporter")
}

/// Compose the full `/v1/traces` URL from a user-provided base endpoint. The
/// OTel spec allows users to pass either the root (`http://host:4318`) or the
/// signal-specific URL (`http://host:4318/v1/traces`); we accept both.
fn http_traces_endpoint(base: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    if trimmed.ends_with("/v1/traces") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1/traces")
    }
}

/// Install a plain fmt subscriber when OTel is disabled / fails to start.
fn install_fmt_only_subscriber(filter: &str) {
    let filter = tracing_subscriber::EnvFilter::try_new(filter)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(DEFAULT_LOG_FILTER));
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(false)
        .compact();
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_parser_accepts_common_spellings() {
        assert_eq!(
            OtlpProtocol::parse("http"),
            Some(OtlpProtocol::HttpProtobuf)
        );
        assert_eq!(
            OtlpProtocol::parse("HTTP/PROTOBUF"),
            Some(OtlpProtocol::HttpProtobuf)
        );
        assert_eq!(OtlpProtocol::parse("grpc"), Some(OtlpProtocol::Grpc));
        assert_eq!(OtlpProtocol::parse("nope"), None);
    }

    #[test]
    fn http_endpoint_appends_traces_path() {
        assert_eq!(
            http_traces_endpoint("http://signoz:4318"),
            "http://signoz:4318/v1/traces"
        );
        assert_eq!(
            http_traces_endpoint("http://signoz:4318/"),
            "http://signoz:4318/v1/traces"
        );
        assert_eq!(
            http_traces_endpoint("http://signoz:4318/v1/traces"),
            "http://signoz:4318/v1/traces"
        );
    }

    #[test]
    fn resolve_falls_back_to_defaults_when_unconfigured() {
        // Note: env vars are process-global. We don't mutate them here to
        // avoid racing with other parallel tests — this only verifies the
        // None-config branch picks the documented defaults.
        let settings = TelemetrySettings::resolve(None);
        // `enabled` could be true if a developer has DEEPSEEK_OTEL_ENABLED set
        // locally; only assert the structurally stable defaults.
        assert!(!settings.service_name.is_empty());
        assert!(
            settings.endpoint.starts_with("http://")
                || settings.endpoint.starts_with("https://")
                || std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_ok()
        );
        assert!(settings.redact_content || std::env::var("DEEPSEEK_OTEL_REDACT_CONTENT").is_ok());
    }
}
