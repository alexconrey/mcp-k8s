//! The `mcp-k8s` binary's OpenTelemetry pipeline (feature `otel`, which
//! `server` enables). Binary-only: this module is declared in `main.rs`, and
//! the `mcp_k8s` library never depends on OpenTelemetry crates. The library
//! emits `tracing` (see `mcp_k8s::telemetry`); this module exports it:
//!
//! - spans through `tracing-opentelemetry`,
//! - the `mcp.server.operation.duration` metric events through its
//!   `MetricsLayer`,
//! - log events through `opentelemetry-appender-tracing`,
//!
//! all over OTLP http/protobuf, configured by the standard `OTEL_*`
//! variables, and off unless an OTLP endpoint is set.

use std::collections::HashMap;

use mcp_k8s::telemetry::{semconv, W3cTraceContext};
use opentelemetry::propagation::TextMapPropagator;
use opentelemetry::trace::{TraceContextExt, TracerProvider as _};
use opentelemetry::{InstrumentationScope, Key, KeyValue};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::{Aggregation, Instrument, SdkMeterProvider, Stream};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use tracing::Span;
use tracing_opentelemetry::{MetricsLayer, OpenTelemetrySpanExt};
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::{Layer, Registry};

/// A boxed layer for the binary's `Registry`-based subscriber.
pub type BoxedLayer = Box<dyn Layer<Registry> + Send + Sync>;

/// `EnvFilter` directives for spans and logs exported over OTLP. `RUST_LOG`
/// only drives console logs.
pub const FILTER_ENV: &str = "MCP_K8S_OTEL_FILTER";
/// The only OTLP protocol this build speaks.
const HTTP_PROTOBUF: &str = "http/protobuf";
/// `service.name` unless `OTEL_SERVICE_NAME` or `OTEL_RESOURCE_ATTRIBUTES`
/// set one.
const DEFAULT_SERVICE_NAME: &str = "mcp-k8s";

/// Which signals to export.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Signals {
    pub traces: bool,
    pub metrics: bool,
    pub logs: bool,
}

impl Signals {
    fn any(self) -> bool {
        self.traces || self.metrics || self.logs
    }
}

/// Decide which signals to export from the standard variables, read through
/// `get` (the process environment in production):
///
/// - nothing without `OTEL_EXPORTER_OTLP_ENDPOINT` or a signal's
///   `OTEL_EXPORTER_OTLP_{TRACES,METRICS,LOGS}_ENDPOINT`;
/// - nothing with `OTEL_SDK_DISABLED=true`;
/// - a signal is off with `OTEL_{TRACES,METRICS,LOGS}_EXPORTER=none`;
/// - a signal whose protocol (`OTEL_EXPORTER_OTLP_[SIGNAL_]PROTOCOL`) isn't
///   `http/protobuf`, or whose exporter isn't `otlp`, is off with a warning:
///   sending protobuf to a gRPC port would only produce errors.
pub fn signals_from_env(get: impl Fn(&str) -> Option<String>) -> (Signals, Vec<String>) {
    let mut warnings = Vec::new();
    if get("OTEL_SDK_DISABLED").is_some_and(|v| v.eq_ignore_ascii_case("true")) {
        return (Signals::default(), warnings);
    }
    let general_endpoint = get("OTEL_EXPORTER_OTLP_ENDPOINT").is_some();
    let general_protocol = get("OTEL_EXPORTER_OTLP_PROTOCOL");
    let mut signal = |name: &str, upper: &str| -> bool {
        let exporter_var = format!("OTEL_{upper}_EXPORTER");
        match get(&exporter_var).as_deref() {
            None | Some("otlp") => {}
            Some("none") => return false,
            Some(other) => {
                warnings.push(format!(
                    "{exporter_var}={other} is not supported (only `otlp` or `none`); not exporting {name}"
                ));
                return false;
            }
        }
        let endpoint = get(&format!("OTEL_EXPORTER_OTLP_{upper}_ENDPOINT")).is_some();
        if !(general_endpoint || endpoint) {
            return false;
        }
        let protocol = get(&format!("OTEL_EXPORTER_OTLP_{upper}_PROTOCOL"))
            .or_else(|| general_protocol.clone());
        if let Some(protocol) = protocol.filter(|p| p != HTTP_PROTOBUF) {
            warnings.push(format!(
                "OTLP protocol `{protocol}` is not supported (this build exports \
                 {HTTP_PROTOBUF} only, usually port 4318); not exporting {name}"
            ));
            return false;
        }
        true
    };
    let signals = Signals {
        traces: signal("traces", "TRACES"),
        metrics: signal("metrics", "METRICS"),
        logs: signal("logs", "LOGS"),
    };
    (signals, warnings)
}

/// The tracer, meter and logger providers, each present only if that signal
/// is exported. Cheap to clone (the providers are reference-counted).
#[derive(Clone, Default)]
pub struct Pipeline {
    tracer_provider: Option<SdkTracerProvider>,
    meter_provider: Option<SdkMeterProvider>,
    logger_provider: Option<SdkLoggerProvider>,
}

impl Pipeline {
    /// Build the OTLP pipeline from the environment. Returns the pipeline
    /// (empty when export is off) and warnings to log once logging is up.
    pub fn from_env() -> (Self, Vec<String>) {
        let (signals, mut warnings) = signals_from_env(env_var);
        let mut pipeline = Self::default();
        if !signals.any() {
            return (pipeline, warnings);
        }
        let resource = resource();
        if signals.traces {
            match opentelemetry_otlp::SpanExporter::builder()
                .with_http()
                .build()
            {
                Ok(exporter) => {
                    pipeline.tracer_provider = Some(
                        SdkTracerProvider::builder()
                            .with_resource(resource.clone())
                            .with_batch_exporter(exporter)
                            .build(),
                    )
                }
                Err(e) => warnings.push(format!("not exporting traces: {e}")),
            }
        }
        if signals.metrics {
            match opentelemetry_otlp::MetricExporter::builder()
                .with_http()
                .build()
            {
                Ok(exporter) => {
                    pipeline.meter_provider = Some(
                        SdkMeterProvider::builder()
                            .with_resource(resource.clone())
                            .with_periodic_exporter(exporter)
                            .with_view(duration_view)
                            .build(),
                    )
                }
                Err(e) => warnings.push(format!("not exporting metrics: {e}")),
            }
        }
        if signals.logs {
            match opentelemetry_otlp::LogExporter::builder()
                .with_http()
                .build()
            {
                Ok(exporter) => {
                    pipeline.logger_provider = Some(
                        SdkLoggerProvider::builder()
                            .with_resource(resource)
                            .with_batch_exporter(exporter)
                            .build(),
                    )
                }
                Err(e) => warnings.push(format!("not exporting logs: {e}")),
            }
        }
        (pipeline, warnings)
    }

    /// A pipeline from ready-made providers (tests use in-memory exporters).
    #[cfg(test)]
    pub fn from_providers(
        tracer_provider: Option<SdkTracerProvider>,
        meter_provider: Option<SdkMeterProvider>,
        logger_provider: Option<SdkLoggerProvider>,
    ) -> Self {
        Self {
            tracer_provider,
            meter_provider,
            logger_provider,
        }
    }

    /// Export everything buffered (tests).
    #[cfg(test)]
    pub fn force_flush(&self) {
        if let Some(p) = &self.tracer_provider {
            p.force_flush().expect("flush traces");
        }
        if let Some(p) = &self.meter_provider {
            p.force_flush().expect("flush metrics");
        }
        if let Some(p) = &self.logger_provider {
            p.force_flush().expect("flush logs");
        }
    }

    /// The signals being exported, for the startup log line.
    pub fn signals(&self) -> Signals {
        Signals {
            traces: self.tracer_provider.is_some(),
            metrics: self.meter_provider.is_some(),
            logs: self.logger_provider.is_some(),
        }
    }

    /// The `tracing` layers that feed the providers. Spans and logs pass the
    /// export filter ([`FILTER_ENV`], default `info`); the metrics layer sees
    /// only metric events.
    pub fn layers(&self) -> Vec<BoxedLayer> {
        let mut layers: Vec<BoxedLayer> = Vec::new();
        if let Some(provider) = &self.tracer_provider {
            let tracer = provider.tracer_with_scope(
                InstrumentationScope::builder(DEFAULT_SERVICE_NAME)
                    .with_version(env!("CARGO_PKG_VERSION"))
                    .build(),
            );
            layers.push(
                tracing_opentelemetry::layer()
                    .with_tracer(tracer)
                    .with_location(false)
                    .with_threads(false)
                    .with_tracked_inactivity(false)
                    .with_filter(export_filter())
                    .boxed(),
            );
        }
        if let Some(provider) = &self.meter_provider {
            layers.push(MetricsLayer::new(provider.clone()).boxed());
        }
        if let Some(provider) = &self.logger_provider {
            layers.push(
                OpenTelemetryTracingBridge::new(provider)
                    .with_filter(export_filter())
                    .boxed(),
            );
        }
        layers
    }

    /// Flush and shut down the providers. Call once, at exit.
    pub fn shutdown(&self) {
        if let Some(p) = &self.tracer_provider {
            if let Err(e) = p.shutdown() {
                eprintln!("OpenTelemetry trace shutdown: {e}");
            }
        }
        if let Some(p) = &self.meter_provider {
            if let Err(e) = p.shutdown() {
                eprintln!("OpenTelemetry metric shutdown: {e}");
            }
        }
        if let Some(p) = &self.logger_provider {
            if let Err(e) = p.shutdown() {
                eprintln!("OpenTelemetry log shutdown: {e}");
            }
        }
    }
}

fn env_var(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// The SDK's resource (`OTEL_SERVICE_NAME`, `OTEL_RESOURCE_ATTRIBUTES`) with
/// `service.name=mcp-k8s` and `service.version` unless those set them.
fn resource() -> Resource {
    let detected = Resource::builder().build();
    let mut builder = Resource::builder();
    let named = detected
        .get(&Key::from_static_str("service.name"))
        .is_some_and(|v| !v.as_str().starts_with("unknown_service"));
    if !named {
        builder = builder.with_service_name(DEFAULT_SERVICE_NAME);
    }
    if detected
        .get(&Key::from_static_str("service.version"))
        .is_none()
    {
        builder =
            builder.with_attribute(KeyValue::new("service.version", env!("CARGO_PKG_VERSION")));
    }
    builder.build()
}

/// `MetricsLayer` creates instruments without a unit or buckets, so this view
/// gives `mcp.server.operation.duration` the conventions' unit (`s`) and
/// bucket boundaries, and keeps only its low-cardinality attributes. Even if
/// someone adds a field to the metric event, it can't become an attribute.
pub fn duration_view(instrument: &Instrument) -> Option<Stream> {
    if instrument.name() != semconv::MCP_SERVER_OPERATION_DURATION {
        return None;
    }
    Stream::builder()
        .with_unit(semconv::MCP_SERVER_OPERATION_DURATION_UNIT)
        .with_description(
            "Duration of MCP requests as observed by the server, from receipt to response.",
        )
        .with_aggregation(Aggregation::ExplicitBucketHistogram {
            boundaries: semconv::MCP_SERVER_OPERATION_DURATION_BUCKETS.to_vec(),
            record_min_max: true,
        })
        .with_allowed_attribute_keys(
            semconv::METRIC_ATTRIBUTES
                .iter()
                .map(|k| Key::from_static_str(k)),
        )
        .build()
        .ok()
}

/// The filter for exported spans and logs: [`FILTER_ENV`] (default `info`),
/// minus metric events and the exporter's own HTTP stack and SDK internals,
/// which would otherwise export telemetry about exporting telemetry.
/// Dependencies' DEBUG output can echo request data (kube-client's `HTTP`
/// spans carry the request URL, label selectors included), so keep it off
/// unless you are debugging.
fn export_filter() -> EnvFilter {
    let base = env_var(FILTER_ENV).unwrap_or_else(|| "info".to_string());
    let mut filter = EnvFilter::try_new(&base).unwrap_or_else(|_| EnvFilter::new("info"));
    let quiet = [
        semconv::METRICS_TARGET,
        "opentelemetry",
        "hyper",
        "h2",
        "reqwest",
        "rustls",
        "tower",
    ];
    for target in quiet {
        if let Ok(directive) = format!("{target}=off").parse() {
            filter = filter.add_directive(directive);
        }
    }
    filter
}

/// Make the request span a child of the caller's trace. The MCP conventions
/// prefer the context in `params._meta`; the HTTP `traceparent` header is
/// the fallback, and becomes a span link when both are present (it belongs
/// to the HTTP request, which may serve more than one MCP message). Must run
/// before the span is first entered.
pub fn set_remote_parent(
    span: &Span,
    meta: Option<&W3cTraceContext>,
    http: Option<&W3cTraceContext>,
) {
    let propagator = TraceContextPropagator::new();
    let extract = |c: &W3cTraceContext| {
        let mut carrier = HashMap::new();
        carrier.insert("traceparent".to_string(), c.traceparent.clone());
        if let Some(state) = &c.tracestate {
            carrier.insert("tracestate".to_string(), state.clone());
        }
        let cx = propagator.extract(&carrier);
        cx.span().span_context().is_valid().then_some(cx)
    };
    let meta_cx = meta.and_then(extract);
    let http_cx = http.and_then(extract);
    match (meta_cx, http_cx) {
        (Some(parent), http) => {
            let _ = span.set_parent(parent);
            if let Some(http) = http {
                span.add_link(http.span().span_context().clone());
            }
        }
        (None, Some(parent)) => {
            let _ = span.set_parent(parent);
        }
        (None, None) => {}
    }
}

/// Record the W3C trace id as the span's `trace_id` field, so console logs
/// can be joined with traces. A no-op when traces aren't exported. This
/// starts the OpenTelemetry span, so call it after [`set_remote_parent`].
pub fn record_trace_id(span: &Span) {
    let cx = span.context();
    let span_context = cx.span().span_context().clone();
    if span_context.is_valid() {
        span.record(
            semconv::TRACE_ID,
            span_context.trace_id().to_string().as_str(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn off_without_an_endpoint() {
        let (s, w) = signals_from_env(env(&[("OTEL_SERVICE_NAME", "x")]));
        assert_eq!(s, Signals::default());
        assert!(w.is_empty());
    }

    #[test]
    fn general_endpoint_enables_everything() {
        let (s, w) = signals_from_env(env(&[("OTEL_EXPORTER_OTLP_ENDPOINT", "http://c:4318")]));
        assert_eq!(
            s,
            Signals {
                traces: true,
                metrics: true,
                logs: true
            }
        );
        assert!(w.is_empty());
    }

    #[test]
    fn per_signal_endpoints_and_exporters() {
        let (s, _) = signals_from_env(env(&[(
            "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
            "http://c:4318/v1/traces",
        )]));
        assert_eq!(
            s,
            Signals {
                traces: true,
                metrics: false,
                logs: false
            }
        );
        let (s, w) = signals_from_env(env(&[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://c:4318"),
            ("OTEL_LOGS_EXPORTER", "none"),
            ("OTEL_METRICS_EXPORTER", "prometheus"),
        ]));
        assert_eq!(
            s,
            Signals {
                traces: true,
                metrics: false,
                logs: false
            }
        );
        assert_eq!(w.len(), 1, "{w:?}");
    }

    #[test]
    fn unsupported_protocol_and_sdk_disabled() {
        let (s, w) = signals_from_env(env(&[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://c:4317"),
            ("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc"),
        ]));
        assert_eq!(s, Signals::default());
        assert_eq!(w.len(), 3);
        assert!(w[0].contains("grpc"));
        let (s, _) = signals_from_env(env(&[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://c:4318"),
            ("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc"),
            ("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL", "http/protobuf"),
        ]));
        assert_eq!(
            s,
            Signals {
                traces: true,
                metrics: false,
                logs: false
            }
        );
        let (s, _) = signals_from_env(env(&[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://c:4318"),
            ("OTEL_SDK_DISABLED", "TRUE"),
        ]));
        assert_eq!(s, Signals::default());
    }
}
