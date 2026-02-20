# ADR-0001: OTel Reporter Shutdown Strategy

## Status

Accepted

## Context

tokf is a CLI tool used by both humans and AI agents (e.g. Claude Code). Exit
latency is a first-class concern: every millisecond the process blocks after a
command finishes is time the user or agent is frozen, waiting for a prompt.

When the `otel` feature is compiled in and telemetry is enabled, the
`OtelReporter` holds an `SdkMeterProvider` backed by a `PeriodicReader` that
runs an export in a background thread. Before `std::process::exit()` is called,
`meter_provider.shutdown()` must be invoked — otherwise the final invocation's
metrics are silently dropped. That `shutdown()` call blocks until the in-flight
HTTP export completes or the exporter timeout fires.

The exporter is configured with a 5-second timeout. In the worst case — an
unreachable or slow OTLP endpoint — every `tokf run` invocation hangs the
terminal for 5 seconds after the command finishes. This is unacceptable.

Three alternatives were evaluated:

### Option A — Blocking shutdown (original)
Call `meter_provider.shutdown()` on the main thread before `process::exit()`.
- **Blocking time:** up to 5 s (exporter timeout)
- **Export reliability:** best — flush always completes if endpoint is reachable
- **Rejected:** 5 s hang is unacceptable for a CLI

### Option B — spawn + join with 200 ms timeout (chosen)
Spawn a thread to call `meter_provider.shutdown()`, then wait at most 200 ms
for it to finish via an `mpsc` channel. If the endpoint responds within 200 ms
the flush succeeds; otherwise the thread is abandoned and killed by
`process::exit()`.
- **Blocking time:** ≤ 200 ms
- **Export reliability:** good — succeeds for any endpoint that responds in < 200 ms
- **Data safety:** all events are written to SQLite *before* `reporter.report()`
  is called, so no event data is ever lost; only the real-time OTel export of
  the last invocation may be skipped under a slow endpoint
- **Accepted**

### Option C — fork + exec
Fork a fresh child process (`std::process::Command::spawn`) that re-execs tokf
with a `telemetry flush --payload <data>` subcommand, then exit the parent
immediately. The child runs to completion independently (adopted by init).
- **Blocking time:** ~0 ms
- **Export reliability:** best for slow endpoints (child can block up to 5 s)
- **Rejected for now:** requires a new subcommand, event serialization, and
  non-trivial inter-process plumbing. The marginal gain over Option B (covering
  the 200 ms–5 s latency band) does not justify the complexity at this stage.
  This option should be revisited if slow-endpoint export reliability becomes a
  real operational complaint.

### Why 200 ms?

200 ms is imperceptible to a human at a terminal and well within the latency
budget of an AI agent waiting for tool output. Any OTLP endpoint that cannot
respond in 200 ms is effectively unreachable from the user's perspective. The
SQLite store (see below) is the safety net for that case.

## Decision

Use **Option B**: spawn a thread for `meter_provider.shutdown()` and join it
with a 200 ms receive timeout. If the timeout fires, abandon the thread; it
will be killed by `process::exit()`.

```rust
fn shutdown(&self) {
    let provider = self.meter_provider.clone();
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        let _ = provider.shutdown();
        let _ = tx.send(());
    });
    // Best-effort: wait up to 200 ms. All data is already in SQLite.
    let _ = rx.recv_timeout(Duration::from_millis(200));
}
```

## Delta Temporality

All instruments are configured with **Delta temporality** (`Temporality::Delta`).

Delta means each export batch contains only the measurements from the current interval
(i.e., since the last export), not a cumulative total since process start. This is the
correct choice for tokf for two reasons:

1. **Per-invocation semantics.** tokf is a CLI tool that exits after each command. Every
   invocation produces a single data point. With Delta, that data point is the complete,
   self-contained measurement for that invocation — no aggregation across the process
   lifetime is needed.

2. **Historical replay compatibility.** The planned `tokf telemetry sync` command (see
   ADR-referenced `synced_to_otel_at` column) will replay events from SQLite to the OTLP
   backend. Delta temporality allows each historical row to be replayed as an independent
   delta at its original timestamp, which is semantically correct and accepted by Datadog,
   Grafana Mimir, New Relic, and Honeycomb. Cumulative temporality would require tracking
   running totals across replayed rows, making replay significantly more complex.

Prometheus Pushgateway is **not** a supported target for historical sync because it is
pull-based and rejects historical timestamps.

## Consequences

- **Blocking time is capped at 200 ms** regardless of endpoint health.
- **No event data is lost.** Every invocation is recorded to SQLite before the
  OTel path is touched. A future `tokf telemetry sync` command can replay any
  events that were not exported in real time (the `synced_to_otel_at` column
  already exists for this purpose).
- **The last invocation's real-time metric may not reach the OTel backend**
  if the endpoint takes longer than 200 ms to respond. This is an acceptable
  trade-off given the SQLite safety net.
- **Option C remains open.** If users running against slow or remote OTLP
  endpoints report missing metrics, the fork+exec approach should be
  implemented as a follow-up.
