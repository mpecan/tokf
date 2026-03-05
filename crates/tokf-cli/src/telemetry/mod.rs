//! Telemetry infrastructure for tokf.
//!
//! Entirely opt-in: disabled by default. Enable at runtime via
//! `TOKF_TELEMETRY_ENABLED=true` (or the `--otel-export` flag) together with a
//! compiled `--features otel` (HTTP) or `--features otel-grpc` binary.
//! When disabled the [`NoopReporter`] is used, which has zero runtime cost.
//!
//! The source of truth for every invocation is the local `SQLite` database (see
//! `tracking` module). OpenTelemetry export is a best-effort real-time replica.

pub mod config;

#[cfg(any(feature = "otel", feature = "otel-grpc"))]
mod otel;

/// Data emitted per command invocation to the telemetry backend.
pub struct TelemetryEvent {
    /// Matched filter name (e.g. `"cargo/build"`), or `None` for passthrough.
    pub filter_name: Option<String>,
    /// Full command string as typed by the user.
    pub command: String,
    /// Raw line count from the command's combined output.
    pub input_lines: u64,
    /// Line count after filtering.
    pub output_lines: u64,
    /// Estimated input tokens (bytes / 4).
    pub input_tokens: u64,
    /// Estimated output tokens (bytes / 4).
    pub output_tokens: u64,
    /// Wall-clock time spent in the filter pipeline (seconds).
    pub filter_duration_secs: f64,
    /// Exit code returned by the underlying command.
    pub exit_code: i32,
    /// Optional pipeline label from `TOKF_OTEL_PIPELINE`.
    pub pipeline: Option<String>,
}

impl TelemetryEvent {
    /// Build a `TelemetryEvent` from raw execution data.
    ///
    /// Centralizes the `bytes / 4` token estimation, `.lines().count()`, and
    /// `TOKF_OTEL_PIPELINE` env-var read so callers don't duplicate these.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::too_many_arguments
    )]
    pub fn new(
        filter_name: Option<String>,
        command: String,
        input_bytes: usize,
        output_bytes: usize,
        raw_output: &str,
        filtered_output: &str,
        filter_duration: std::time::Duration,
        exit_code: i32,
    ) -> Self {
        Self {
            filter_name,
            command,
            input_lines: raw_output.lines().count() as u64,
            output_lines: filtered_output.lines().count() as u64,
            input_tokens: (input_bytes / 4) as u64,
            output_tokens: (output_bytes / 4) as u64,
            filter_duration_secs: filter_duration.as_secs_f64(),
            exit_code,
            pipeline: std::env::var("TOKF_OTEL_PIPELINE").ok(),
        }
    }
}

/// Abstraction over telemetry backends. Implementations must be `Send + Sync`
/// so the reporter can be held behind a shared reference from `main`.
pub trait TelemetryReporter: Send + Sync {
    fn report(&self, event: &TelemetryEvent);
    /// Flush pending metrics. Returns `true` if the flush completed successfully,
    /// `false` if it timed out or was a no-op.
    fn shutdown(&self) -> bool;
    /// Returns a human-readable description of the active backend endpoint,
    /// or `None` when telemetry is disabled (e.g. `NoopReporter`).
    fn endpoint_description(&self) -> Option<String> {
        None
    }
}

/// Zero-cost reporter used when telemetry is disabled or unavailable.
pub struct NoopReporter;

impl TelemetryReporter for NoopReporter {
    fn report(&self, _event: &TelemetryEvent) {}
    fn shutdown(&self) -> bool {
        true
    }
}

/// Initialise the telemetry reporter.
///
/// If `otel_export_requested` is `true` the config's `enabled` flag is forced on.
/// Returns a `NoopReporter` when:
/// - telemetry is disabled in both flags and config, or
/// - the binary was not compiled with `--features otel` or `--features otel-grpc`, or
/// - OTLP initialisation fails (with a warning printed to stderr).
pub fn init(otel_export_requested: bool) -> Box<dyn TelemetryReporter> {
    let mut cfg = config::load();
    if otel_export_requested {
        cfg.enabled = true;
    }
    if !cfg.enabled {
        return Box::new(NoopReporter);
    }
    init_enabled(otel_export_requested, &cfg)
}

#[cfg(any(feature = "otel", feature = "otel-grpc"))]
fn init_enabled(_requested: bool, cfg: &config::TelemetryConfig) -> Box<dyn TelemetryReporter> {
    match otel::OtelReporter::new(cfg) {
        Ok(reporter) => Box::new(reporter),
        Err(e) => {
            eprintln!("[tokf] warning: OTel init failed ({e:#}), metrics disabled");
            Box::new(NoopReporter)
        }
    }
}

#[cfg(not(any(feature = "otel", feature = "otel-grpc")))]
fn init_enabled(requested: bool, _cfg: &config::TelemetryConfig) -> Box<dyn TelemetryReporter> {
    if requested {
        eprintln!(
            "[tokf] warning: OTel support not compiled in (need --features otel or otel-grpc)"
        );
    }
    Box::new(NoopReporter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noop_reporter_report_and_shutdown() {
        let reporter = NoopReporter;
        let event = TelemetryEvent {
            filter_name: Some("test/filter".to_string()),
            command: "test command".to_string(),
            input_lines: 100,
            output_lines: 50,
            input_tokens: 200,
            output_tokens: 100,
            filter_duration_secs: 0.01,
            exit_code: 0,
            pipeline: None,
        };
        reporter.report(&event);
        let _ = reporter.shutdown();
    }

    #[test]
    fn test_noop_init_does_not_panic() {
        let reporter = NoopReporter;
        reporter.report(&TelemetryEvent::new(
            None,
            "ls".to_string(),
            120,
            120,
            "line1\nline2\n",
            "line1\nline2\n",
            std::time::Duration::ZERO,
            0,
        ));
        let _ = reporter.shutdown();
    }

    #[test]
    fn test_noop_reporter_endpoint_description_is_none() {
        assert!(NoopReporter.endpoint_description().is_none());
    }

    #[test]
    fn test_noop_reporter_shutdown_returns_true() {
        assert!(NoopReporter.shutdown());
    }

    #[test]
    fn test_telemetry_event_new_computes_fields() {
        let raw = "line1\nline2\nline3\n";
        let filtered = "summary\n";
        let event = TelemetryEvent::new(
            Some("cargo/build".to_string()),
            "cargo build".to_string(),
            400, // input_bytes
            100, // output_bytes
            raw,
            filtered,
            std::time::Duration::from_millis(5),
            0,
        );
        assert_eq!(event.input_lines, 3);
        assert_eq!(event.output_lines, 1);
        assert_eq!(event.input_tokens, 100); // 400 / 4
        assert_eq!(event.output_tokens, 25); // 100 / 4
        assert!((event.filter_duration_secs - 0.005).abs() < 0.001);
        assert_eq!(event.exit_code, 0);
        assert_eq!(event.filter_name, Some("cargo/build".to_string()));
        assert_eq!(event.command, "cargo build");
    }

    #[test]
    fn test_telemetry_event_new_passthrough() {
        let output = "hello\nworld\n";
        let event = TelemetryEvent::new(
            None,
            "ls".to_string(),
            48,
            48,
            output,
            output,
            std::time::Duration::ZERO,
            0,
        );
        // Passthrough: input == output
        assert_eq!(event.input_lines, event.output_lines);
        assert_eq!(event.input_tokens, event.output_tokens);
        assert!(event.filter_duration_secs.abs() < f64::EPSILON);
        assert!(event.filter_name.is_none());
    }

    /// When compiled without any otel feature, requesting `OTel` export falls back to `NoopReporter`.
    #[cfg(not(any(feature = "otel", feature = "otel-grpc")))]
    #[test]
    fn test_init_without_otel_feature_returns_noop() {
        let reporter = init(true); // otel_export_requested=true, but feature not compiled in
        // endpoint_description() returns None for NoopReporter
        assert!(reporter.endpoint_description().is_none());
    }
}
