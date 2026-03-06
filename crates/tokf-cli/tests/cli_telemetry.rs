#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::TcpListener;
use std::process::Command;

use tempfile::TempDir;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

#[test]
fn telemetry_status_shows_disabled_by_default() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path())
        .args(["telemetry", "status"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("telemetry: disabled"),
        "expected 'telemetry: disabled' in output:\n{stdout}"
    );
}

#[test]
fn telemetry_status_shows_enabled_when_env_set() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path())
        .env("TOKF_TELEMETRY_ENABLED", "true")
        .args(["telemetry", "status"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("telemetry: enabled"),
        "expected 'telemetry: enabled' in output:\n{stdout}"
    );
}

#[test]
fn telemetry_status_shows_expected_fields() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path())
        .args(["telemetry", "status"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("endpoint:"),
        "missing endpoint field:\n{stdout}"
    );
    assert!(
        stdout.contains("protocol:"),
        "missing protocol field:\n{stdout}"
    );
    assert!(
        stdout.contains("service:"),
        "missing service field:\n{stdout}"
    );
    assert!(
        stdout.contains("pipeline:"),
        "missing pipeline field:\n{stdout}"
    );
}

#[test]
fn telemetry_status_shows_custom_endpoint() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path())
        .env(
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            "http://otel.example.com:4318",
        )
        .args(["telemetry", "status"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("http://otel.example.com:4318"),
        "expected custom endpoint in output:\n{stdout}"
    );
}

#[test]
fn telemetry_status_verbose_shows_config_and_features() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path())
        .args(["--verbose", "telemetry", "status"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("config:"),
        "expected config path in verbose output:\n{stdout}"
    );
    assert!(
        stdout.contains("features:"),
        "expected features in verbose output:\n{stdout}"
    );
    assert!(
        stdout.contains("headers:"),
        "expected headers in verbose output:\n{stdout}"
    );
}

#[test]
fn telemetry_status_check_http_unreachable_exits_1() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path())
        // Port 1 on loopback is always closed — deterministic failure.
        .env("OTEL_EXPORTER_OTLP_ENDPOINT", "http://127.0.0.1:1")
        .args(["telemetry", "status", "--check"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "expected exit code 1 for unreachable endpoint, stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("FAILED"),
        "expected FAILED in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("[tokf] error:"),
        "expected error detail in stderr:\n{stderr}"
    );
}

#[test]
fn telemetry_status_check_http_reachable_exits_0() {
    // Bind a TCP listener and serve a minimal HTTP response so that
    // reqwest's POST completes successfully.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            use std::io::{Read, Write};
            let mut buf = [0u8; 512];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        }
    });

    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path())
        .env(
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            format!("http://127.0.0.1:{port}"),
        )
        .args(["telemetry", "status", "--check"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let _ = handle.join();
    assert!(
        output.status.success(),
        "expected exit code 0 for reachable endpoint, stderr:\n{stderr}"
    );
    assert!(stderr.contains("OK"), "expected OK in stderr:\n{stderr}");
    assert!(
        stderr.contains("ms)"),
        "expected latency in stderr:\n{stderr}"
    );
}

#[test]
fn telemetry_status_check_grpc_unreachable_exits_1() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path())
        .env("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc")
        // Port 1 on loopback is always closed — deterministic failure.
        .env("OTEL_EXPORTER_OTLP_ENDPOINT", "http://127.0.0.1:1")
        .args(["telemetry", "status", "--check"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "expected exit code 1 for unreachable gRPC endpoint, stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("FAILED"),
        "expected FAILED in stderr:\n{stderr}"
    );
}

#[test]
fn telemetry_status_check_grpc_reachable_exits_0() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path())
        .env("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc")
        .env(
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            format!("http://127.0.0.1:{port}"),
        )
        .args(["telemetry", "status", "--check"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    drop(listener);
    assert!(
        output.status.success(),
        "expected exit code 0 for reachable gRPC endpoint, stderr:\n{stderr}"
    );
    assert!(stderr.contains("OK"), "expected OK in stderr:\n{stderr}");
}

#[test]
fn telemetry_status_pipeline_from_env() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path())
        .env("TOKF_OTEL_PIPELINE", "ci-main")
        .args(["telemetry", "status"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("ci-main"),
        "expected pipeline value in output:\n{stdout}"
    );
}

#[test]
fn telemetry_status_reads_config_file() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("config.toml"),
        "[telemetry]\nenabled = true\nendpoint = \"http://custom:4318\"\nservice_name = \"my-svc\"\n",
    )
    .unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path())
        // Clear env vars that would override
        .env_remove("TOKF_TELEMETRY_ENABLED")
        .env_remove("OTEL_EXPORTER_OTLP_ENDPOINT")
        .args(["telemetry", "status"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("telemetry: enabled"),
        "expected enabled from config file:\n{stdout}"
    );
    assert!(
        stdout.contains("http://custom:4318"),
        "expected custom endpoint from config file:\n{stdout}"
    );
    assert!(
        stdout.contains("my-svc"),
        "expected custom service name:\n{stdout}"
    );
}

#[test]
fn telemetry_status_verbose_redacts_headers() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path())
        .env(
            "OTEL_EXPORTER_OTLP_HEADERS",
            "x-api-key=supersecret,x-team=eng",
        )
        .args(["--verbose", "telemetry", "status"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    // Header keys should be visible but values redacted
    assert!(
        stdout.contains("<redacted>"),
        "expected <redacted> in verbose output:\n{stdout}"
    );
    // The actual secret value must NOT appear
    assert!(
        !stdout.contains("supersecret"),
        "secret value should be redacted, got:\n{stdout}"
    );
}

#[test]
fn telemetry_status_verbose_shows_no_headers_when_empty() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path())
        .env_remove("OTEL_EXPORTER_OTLP_HEADERS")
        .args(["--verbose", "telemetry", "status"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("headers:   (none)"),
        "expected '(none)' for empty headers:\n{stdout}"
    );
}

#[test]
fn telemetry_status_shows_grpc_protocol() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path())
        .env("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc")
        .args(["telemetry", "status"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("protocol:  grpc"),
        "expected 'protocol:  grpc' in output:\n{stdout}"
    );
    // gRPC default endpoint should be :4317
    assert!(
        stdout.contains(":4317"),
        "expected gRPC default port 4317:\n{stdout}"
    );
}

#[test]
fn telemetry_status_verbose_config_not_found() {
    let tmp = TempDir::new().unwrap();
    // No config.toml created — should show "not found"
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path())
        .args(["--verbose", "telemetry", "status"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("(not found)"),
        "expected '(not found)' for missing config:\n{stdout}"
    );
}

#[test]
fn telemetry_status_verbose_config_exists() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("config.toml"), "[telemetry]\n").unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path())
        .args(["--verbose", "telemetry", "status"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    // When config exists, path should be shown without "(not found)"
    assert!(
        stdout.contains("config:"),
        "expected config line:\n{stdout}"
    );
    assert!(
        stdout.contains("config.toml"),
        "expected config.toml path:\n{stdout}"
    );
    // Should NOT say "(not found)" when file exists
    let config_line = stdout
        .lines()
        .find(|l| l.starts_with("config:"))
        .unwrap_or("");
    assert!(
        !config_line.contains("(not found)"),
        "config exists but line says not found:\n{stdout}"
    );
}

#[test]
fn telemetry_status_check_stderr_shows_endpoint() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path())
        .env("OTEL_EXPORTER_OTLP_ENDPOINT", "http://127.0.0.1:1")
        .args(["telemetry", "status", "--check"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The "checking" line should include the endpoint
    assert!(
        stderr.contains("checking OTLP endpoint"),
        "expected 'checking OTLP endpoint' in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("127.0.0.1:1"),
        "expected endpoint address in stderr:\n{stderr}"
    );
}
