// Only compiled when the `otel` feature (and by extension `otel-http` or `otel-grpc`) is active.
use std::time::Duration;

use anyhow::Context as _;
use opentelemetry::{
    KeyValue,
    metrics::{Counter, Gauge, Histogram, MeterProvider as _},
};
use opentelemetry_sdk::metrics::{SdkMeterProvider, Temporality};

#[cfg(feature = "otel-grpc")]
use crate::telemetry::config::Protocol;
use crate::telemetry::config::TelemetryConfig;
use crate::telemetry::{TelemetryEvent, TelemetryReporter};

/// OpenTelemetry OTLP metrics reporter.
///
/// Wraps an [`SdkMeterProvider`] and exposes one instrument per tokf metric.
/// Created via [`OtelReporter::new`] (production) or [`OtelReporter::from_provider`] (tests).
pub struct OtelReporter {
    meter_provider: SdkMeterProvider,
    /// Human-readable endpoint description shown in `--verbose` output.
    endpoint: Option<String>,
    input_lines: Counter<u64>,
    output_lines: Counter<u64>,
    lines_removed: Counter<u64>,
    compression_ratio: Gauge<f64>,
    tokens_saved: Counter<u64>,
    filter_duration: Histogram<f64>,
    invocations: Counter<u64>,
    token_usage: Histogram<u64>,
}

impl OtelReporter {
    /// Create a new `OtelReporter` from the given config, building the OTLP exporter and
    /// all metric instruments.
    ///
    /// # Errors
    /// Returns an error if the OTLP exporter cannot be built (e.g. invalid endpoint).
    pub fn new(config: &TelemetryConfig) -> anyhow::Result<Self> {
        let provider = build_provider(config)?;
        let mut reporter = Self::from_provider(provider);
        reporter.endpoint = Some(config.endpoint.clone());
        Ok(reporter)
    }

    /// Create a reporter from an already-built `SdkMeterProvider`. Used in tests.
    // Instrument registration is verbose — approved to exceed 60-line limit.
    #[allow(clippy::too_many_lines)]
    pub fn from_provider(provider: SdkMeterProvider) -> Self {
        let meter = provider.meter("tokf");

        let input_lines = meter
            .u64_counter("tokf.filter.input_lines")
            .with_unit("{line}")
            .with_description("Number of input lines before filtering")
            .build();

        let output_lines = meter
            .u64_counter("tokf.filter.output_lines")
            .with_unit("{line}")
            .with_description("Number of output lines after filtering")
            .build();

        let lines_removed = meter
            .u64_counter("tokf.filter.lines_removed")
            .with_unit("{line}")
            .with_description("Number of lines removed by filtering")
            .build();

        let compression_ratio = meter
            .f64_gauge("tokf.compression.ratio")
            .with_unit("1")
            .with_description("Ratio of output to input lines (lower means more compression)")
            .build();

        let tokens_saved = meter
            .u64_counter("tokf.tokens.saved")
            .with_unit("{token}")
            .with_description("Estimated tokens saved by filtering")
            .build();

        let filter_duration = meter
            .f64_histogram("tokf.filter.duration")
            .with_unit("s")
            .with_description("Time spent applying the filter")
            .build();

        let invocations = meter
            .u64_counter("tokf.filter.invocations")
            .with_unit("{invocation}")
            .with_description("Number of filter invocations")
            .build();

        let token_usage = meter
            .u64_histogram("gen_ai.client.token.usage")
            .with_unit("{token}")
            .with_description("Token usage per invocation")
            .build();

        Self {
            meter_provider: provider,
            endpoint: None,
            input_lines,
            output_lines,
            lines_removed,
            compression_ratio,
            tokens_saved,
            filter_duration,
            invocations,
            token_usage,
        }
    }
}

fn build_attrs(event: &TelemetryEvent) -> Vec<KeyValue> {
    // Note: tokf.version is set as a resource attribute (see build_provider), not per-metric,
    // to avoid inflating metric cardinality.
    //
    // event.command is cloned because opentelemetry's Value type requires an owned String
    // (internally Cow<'static, str>); there is no way to pass a borrowed &str here.
    let mut attrs = vec![
        KeyValue::new("tokf.command", event.command.clone()),
        KeyValue::new("tokf.exit_code", i64::from(event.exit_code)),
    ];
    if let Some(ref name) = event.filter_name {
        attrs.push(KeyValue::new("tokf.filter.name", name.clone()));
    }
    if let Some(ref pipeline) = event.pipeline {
        attrs.push(KeyValue::new("tokf.pipeline", pipeline.clone()));
    }
    attrs
}

impl TelemetryReporter for OtelReporter {
    /// Record all metrics for a single tokf invocation.
    ///
    /// `gen_ai.client.token.usage` is emitted twice — once with
    /// `gen_ai.token.type = "input"` and once with `"output"` — matching the
    /// [OTel GenAI semantic conventions](https://opentelemetry.io/docs/specs/semconv/gen-ai/).
    fn report(&self, event: &TelemetryEvent) {
        let attrs = build_attrs(event);

        self.invocations.add(1, &attrs);
        self.input_lines.add(event.input_lines, &attrs);
        self.output_lines.add(event.output_lines, &attrs);

        let removed = event.input_lines.saturating_sub(event.output_lines);
        self.lines_removed.add(removed, &attrs);

        if event.input_lines > 0 {
            #[allow(clippy::cast_precision_loss)]
            let ratio = event.output_lines as f64 / event.input_lines as f64;
            self.compression_ratio.record(ratio, &attrs);
        }

        let saved = event.input_tokens.saturating_sub(event.output_tokens);
        self.tokens_saved.add(saved, &attrs);
        self.filter_duration
            .record(event.filter_duration_secs, &attrs);

        // Record token usage split by type per OTel GenAI semantic conventions.
        let mut input_attrs = attrs.clone();
        input_attrs.push(KeyValue::new("gen_ai.token.type", "input"));
        self.token_usage.record(event.input_tokens, &input_attrs);

        let mut output_attrs = attrs;
        output_attrs.push(KeyValue::new("gen_ai.token.type", "output"));
        self.token_usage.record(event.output_tokens, &output_attrs);
    }

    fn shutdown(&self) -> bool {
        // Best-effort flush: spawn a thread and wait at most 200 ms.
        // All event data is already persisted to SQLite before this point, so
        // no data is lost if the endpoint is slow. See docs/adr/0001-otel-shutdown-strategy.md.
        let provider = self.meter_provider.clone();
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            let _ = provider.shutdown();
            let _ = tx.send(());
        });
        rx.recv_timeout(Duration::from_millis(200)).is_ok()
    }

    fn endpoint_description(&self) -> Option<String> {
        self.endpoint.clone()
    }
}

// ---------------------------------------------------------------------------
// Exporter construction — selected at compile time by active transport feature
// ---------------------------------------------------------------------------

#[cfg(feature = "otel-http")]
fn build_http_exporter(
    config: &TelemetryConfig,
) -> anyhow::Result<opentelemetry_otlp::MetricExporter> {
    use opentelemetry_otlp::{WithExportConfig, WithHttpConfig};

    let builder = opentelemetry_otlp::MetricExporter::builder()
        .with_temporality(Temporality::Delta)
        .with_http()
        .with_endpoint(&config.endpoint)
        .with_timeout(Duration::from_secs(5));

    let builder = if config.headers.is_empty() {
        builder
    } else {
        builder.with_headers(config.headers.clone())
    };

    builder.build().context("build OTLP HTTP metrics exporter")
}

#[cfg(feature = "otel-grpc")]
fn build_grpc_exporter(
    config: &TelemetryConfig,
) -> anyhow::Result<opentelemetry_otlp::MetricExporter> {
    use opentelemetry_otlp::WithExportConfig;

    opentelemetry_otlp::MetricExporter::builder()
        .with_temporality(Temporality::Delta)
        .with_tonic()
        .with_endpoint(&config.endpoint)
        .with_timeout(Duration::from_secs(5))
        .build()
        .context("build OTLP gRPC metrics exporter")
}

fn build_exporter(config: &TelemetryConfig) -> anyhow::Result<opentelemetry_otlp::MetricExporter> {
    #[cfg(feature = "otel-grpc")]
    if matches!(config.protocol, Protocol::Grpc) {
        return build_grpc_exporter(config);
    }

    #[cfg(feature = "otel-http")]
    {
        return build_http_exporter(config);
    }

    // Safety net: `otel` feature always enables `otel-http`, so this is unreachable.
    #[allow(unreachable_code)]
    {
        let _ = config;
        anyhow::bail!("no OTel transport feature compiled in (need otel-http or otel-grpc)")
    }
}

fn build_provider(config: &TelemetryConfig) -> anyhow::Result<SdkMeterProvider> {
    let exporter = build_exporter(config)?;

    let reader = opentelemetry_sdk::metrics::PeriodicReader::builder(exporter).build();

    // service.version set here (resource attribute) rather than per-metric to avoid
    // inflating cardinality — it is constant for the lifetime of the process.
    let resource = opentelemetry_sdk::Resource::builder_empty()
        .with_attribute(KeyValue::new("service.name", config.service_name.clone()))
        .with_attribute(KeyValue::new("service.version", env!("CARGO_PKG_VERSION")))
        .build();

    let provider = SdkMeterProvider::builder()
        .with_reader(reader)
        .with_resource(resource)
        .build();

    Ok(provider)
}

#[cfg(all(test, any(feature = "otel", feature = "otel-grpc")))]
mod tests {
    use opentelemetry_sdk::metrics::{ManualReader, SdkMeterProvider};

    use super::*;
    use crate::telemetry::{TelemetryEvent, TelemetryReporter};

    fn make_reporter() -> OtelReporter {
        let reader = ManualReader::builder()
            .with_temporality(Temporality::Delta)
            .build();
        let provider = SdkMeterProvider::builder().with_reader(reader).build();
        OtelReporter::from_provider(provider)
    }

    fn sample_event() -> TelemetryEvent {
        TelemetryEvent {
            filter_name: Some("cargo/build".to_string()),
            command: "cargo build".to_string(),
            input_lines: 100,
            output_lines: 40,
            input_tokens: 300,
            output_tokens: 120,
            filter_duration_secs: 0.005,
            exit_code: 0,
            pipeline: None,
        }
    }

    #[test]
    fn test_report_does_not_panic() {
        let reporter = make_reporter();
        reporter.report(&sample_event());
        let _ = reporter.shutdown();
    }

    #[test]
    fn test_report_passthrough_no_filter() {
        let reporter = make_reporter();
        let event = TelemetryEvent {
            filter_name: None,
            command: "ls".to_string(),
            input_lines: 50,
            output_lines: 50,
            input_tokens: 150,
            output_tokens: 150,
            filter_duration_secs: 0.0,
            exit_code: 0,
            pipeline: None,
        };
        reporter.report(&event);
        let _ = reporter.shutdown();
    }

    #[test]
    fn test_report_with_pipeline() {
        let reporter = make_reporter();
        let mut event = sample_event();
        event.pipeline = Some("my-pipeline".to_string());
        reporter.report(&event);
    }

    #[test]
    fn test_zero_input_lines_no_panic() {
        let reporter = make_reporter();
        let event = TelemetryEvent {
            filter_name: Some("test".to_string()),
            command: "test".to_string(),
            input_lines: 0,
            output_lines: 0,
            input_tokens: 0,
            output_tokens: 0,
            filter_duration_secs: 0.001,
            exit_code: 0,
            pipeline: None,
        };
        reporter.report(&event);
    }

    #[test]
    fn test_token_usage_split_by_type_does_not_panic() {
        // Verifies the gen_ai.token.type split path (both input and output recorded separately).
        let reporter = make_reporter();
        let event = TelemetryEvent {
            filter_name: Some("cargo/build".to_string()),
            command: "cargo build".to_string(),
            input_lines: 100,
            output_lines: 40,
            input_tokens: 300,
            output_tokens: 120,
            filter_duration_secs: 0.005,
            exit_code: 0,
            pipeline: Some("ci".to_string()),
        };
        // report() should record token_usage twice (input=300, output=120) without panicking.
        reporter.report(&event);
    }

    #[test]
    fn test_from_provider_endpoint_description_is_none() {
        // from_provider() is used in tests — endpoint should be None since no config is given.
        let reporter = make_reporter();
        assert!(reporter.endpoint_description().is_none());
    }
}
