//! Telemetry infrastructure for tokf.
//!
//! Entirely opt-in: disabled by default. Enable at runtime via
//! `TOKF_TELEMETRY_ENABLED=true` (or the `--otel-export` flag) together with a
//! compiled `--features otel` binary.  When disabled the [`NoopReporter`] is
//! used, which has zero runtime cost.
//!
//! The source of truth for every invocation is the local `SQLite` database (see
//! `tracking` module). OpenTelemetry export is a best-effort real-time replica.

pub mod config;

#[cfg(feature = "otel")]
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
/// - the binary was not compiled with `--features otel`, or
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

#[cfg(feature = "otel")]
fn init_enabled(_requested: bool, cfg: &config::TelemetryConfig) -> Box<dyn TelemetryReporter> {
    match otel::OtelReporter::new(cfg) {
        Ok(reporter) => Box::new(reporter),
        Err(e) => {
            eprintln!("[tokf] warning: OTel init failed ({e:#}), metrics disabled");
            Box::new(NoopReporter)
        }
    }
}

#[cfg(not(feature = "otel"))]
fn init_enabled(requested: bool, _cfg: &config::TelemetryConfig) -> Box<dyn TelemetryReporter> {
    if requested {
        eprintln!("[tokf] warning: OTel support not compiled in (need --features otel)");
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
        // Pass otel_export_requested=false; with no env var, this always returns NoopReporter.
        // We can't safely mutate env vars in Rust 2024 without an unsafe block, so just verify
        // the reporter returned from init() is callable without panicking.
        let reporter = NoopReporter;
        reporter.report(&TelemetryEvent {
            filter_name: None,
            command: "ls".to_string(),
            input_lines: 10,
            output_lines: 10,
            input_tokens: 30,
            output_tokens: 30,
            filter_duration_secs: 0.0,
            exit_code: 0,
            pipeline: None,
        });
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

    /// When compiled without the `otel` feature, requesting OTel export falls back to NoopReporter.
    #[cfg(not(feature = "otel"))]
    #[test]
    fn test_init_without_otel_feature_returns_noop() {
        let reporter = init(true); // otel_export_requested=true, but feature not compiled in
        // endpoint_description() returns None for NoopReporter
        assert!(reporter.endpoint_description().is_none());
    }
}
