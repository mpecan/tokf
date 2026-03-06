use std::collections::HashMap;
use std::time::Instant;

use tokf::telemetry::config;

/// `tokf telemetry status` — display telemetry configuration and optionally
/// test connectivity to the OTLP endpoint.
pub fn cmd_telemetry_status(check: bool, verbose: bool) -> anyhow::Result<i32> {
    let cfg = config::load();

    let status = if cfg.enabled { "enabled" } else { "disabled" };
    let pipeline = std::env::var("TOKF_OTEL_PIPELINE")
        .ok()
        .filter(|s| !s.is_empty());

    println!("telemetry: {status}");
    println!("endpoint:  {}", cfg.endpoint);
    println!("protocol:  {}", cfg.protocol);
    println!("service:   {}", cfg.service_name);
    println!("pipeline:  {}", pipeline.as_deref().unwrap_or("(not set)"));

    if verbose {
        print_verbose(&cfg);
    }

    if check {
        return check_endpoint(&cfg);
    }

    Ok(0)
}

fn print_verbose(cfg: &config::TelemetryConfig) {
    println!();

    // Config file path
    let cfg_path = tokf::paths::user_dir().map(|d| d.join("config.toml"));
    match cfg_path {
        Some(ref p) if p.exists() => println!("config:    {}", p.display()),
        Some(ref p) => println!("config:    {} (not found)", p.display()),
        None => println!("config:    (unavailable)"),
    }

    // Headers (redacted values)
    if cfg.headers.is_empty() {
        println!("headers:   (none)");
    } else {
        let redacted: Vec<String> = cfg
            .headers
            .keys()
            .map(|k| format!("{k}=<redacted>"))
            .collect();
        println!("headers:   {}", redacted.join(", "));
    }

    // Compiled feature flags
    let otel_http = cfg!(any(feature = "otel", feature = "otel-http"));
    let otel_grpc = cfg!(feature = "otel-grpc");
    println!(
        "features:  otel-http={}, otel-grpc={}",
        if otel_http { "yes" } else { "no" },
        if otel_grpc { "yes" } else { "no" },
    );
}

fn check_endpoint(cfg: &config::TelemetryConfig) -> anyhow::Result<i32> {
    eprintln!("[tokf] checking OTLP endpoint {} ...", cfg.endpoint);

    match cfg.protocol {
        config::Protocol::Http => check_http(&cfg.endpoint, &cfg.headers),
        config::Protocol::Grpc => check_grpc(&cfg.endpoint),
    }
}

fn build_header_map(
    headers: &HashMap<String, String>,
) -> anyhow::Result<reqwest::header::HeaderMap> {
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

    let mut map = HeaderMap::with_capacity(headers.len());
    for (k, v) in headers {
        let name = HeaderName::from_bytes(k.as_bytes())
            .map_err(|e| anyhow::anyhow!("invalid header name '{k}': {e}"))?;
        let value = HeaderValue::from_str(v)
            .map_err(|e| anyhow::anyhow!("invalid header value for '{k}': {e}"))?;
        map.insert(name, value);
    }
    Ok(map)
}

fn check_http(endpoint: &str, headers: &HashMap<String, String>) -> anyhow::Result<i32> {
    let url = format!("{}/v1/metrics", endpoint.trim_end_matches('/'));
    let header_map = build_header_map(headers)?;
    let start = Instant::now();
    let result = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()?
        .post(&url)
        .headers(header_map)
        .send();

    match result {
        Ok(_response) => {
            let ms = start.elapsed().as_millis();
            eprintln!("[tokf] OK ({ms} ms)");
            Ok(0)
        }
        Err(e) => {
            eprintln!("[tokf] FAILED");
            eprintln!("[tokf] error: {e}");
            Ok(1)
        }
    }
}

const CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

fn check_grpc(endpoint: &str) -> anyhow::Result<i32> {
    // Resolve + connect under a single timeout so slow DNS can't exceed the budget.
    let host_port = strip_endpoint(endpoint);
    let start = Instant::now();

    let addr = resolve_with_timeout(&host_port, CHECK_TIMEOUT)?;
    let remaining = CHECK_TIMEOUT.saturating_sub(start.elapsed());
    let result = std::net::TcpStream::connect_timeout(&addr, remaining);

    match result {
        Ok(_) => {
            let ms = start.elapsed().as_millis();
            eprintln!("[tokf] OK ({ms} ms)");
            Ok(0)
        }
        Err(e) => {
            eprintln!("[tokf] FAILED");
            eprintln!("[tokf] error: {e}");
            Ok(1)
        }
    }
}

/// Strip scheme and path from an endpoint URL, returning `host:port`.
fn strip_endpoint(endpoint: &str) -> String {
    let without_scheme = endpoint
        .strip_prefix("https://")
        .or_else(|| endpoint.strip_prefix("http://"))
        .unwrap_or(endpoint);
    let host_port = without_scheme.split('/').next().unwrap_or(without_scheme);
    host_port.to_string()
}

/// Resolve a `host:port` string to a `SocketAddr` with a bounded timeout,
/// so slow DNS cannot block indefinitely.
fn resolve_with_timeout(
    host_port: &str,
    timeout: std::time::Duration,
) -> anyhow::Result<std::net::SocketAddr> {
    use std::net::ToSocketAddrs;

    let owned = host_port.to_string();
    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let result = owned
            .to_socket_addrs()
            .map(|mut addrs| addrs.next())
            .map_err(|e| e.to_string());
        let _ = tx.send(result);
    });

    let result = rx
        .recv_timeout(timeout)
        .map_err(|_| anyhow::anyhow!("DNS resolution timed out for {host_port}"))?;

    match result {
        Ok(Some(addr)) => Ok(addr),
        Ok(None) => anyhow::bail!("could not resolve endpoint address: {host_port}"),
        Err(e) => anyhow::bail!("DNS resolution failed for {host_port}: {e}"),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn strip_endpoint_http() {
        assert_eq!(strip_endpoint("http://127.0.0.1:4317"), "127.0.0.1:4317");
    }

    #[test]
    fn strip_endpoint_https() {
        assert_eq!(strip_endpoint("https://127.0.0.1:4318"), "127.0.0.1:4318");
    }

    #[test]
    fn strip_endpoint_with_path() {
        assert_eq!(
            strip_endpoint("http://127.0.0.1:9090/v1/metrics"),
            "127.0.0.1:9090"
        );
    }

    #[test]
    fn strip_endpoint_no_scheme() {
        assert_eq!(strip_endpoint("127.0.0.1:4317"), "127.0.0.1:4317");
    }

    #[test]
    fn resolve_loopback() {
        let addr =
            resolve_with_timeout("127.0.0.1:4317", std::time::Duration::from_secs(2)).unwrap();
        assert_eq!(addr.port(), 4317);
        assert!(addr.ip().is_loopback());
    }

    #[test]
    fn resolve_missing_port() {
        assert!(resolve_with_timeout("localhost", std::time::Duration::from_secs(2)).is_err());
    }

    #[test]
    fn resolve_empty() {
        assert!(resolve_with_timeout("", std::time::Duration::from_secs(2)).is_err());
    }

    #[test]
    fn resolve_ipv6() {
        let addr = resolve_with_timeout("[::1]:4317", std::time::Duration::from_secs(2)).unwrap();
        assert_eq!(addr.port(), 4317);
        assert!(addr.ip().is_loopback());
    }

    #[test]
    fn build_header_map_empty() {
        let map = build_header_map(&HashMap::new()).unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn build_header_map_with_entries() {
        let mut headers = HashMap::new();
        headers.insert("x-api-key".to_string(), "secret".to_string());
        headers.insert("x-team".to_string(), "platform".to_string());
        let map = build_header_map(&headers).unwrap();
        assert_eq!(map.get("x-api-key").unwrap(), "secret");
        assert_eq!(map.get("x-team").unwrap(), "platform");
    }

    #[test]
    fn test_protocol_display() {
        assert_eq!(format!("{}", config::Protocol::Http), "http");
        assert_eq!(format!("{}", config::Protocol::Grpc), "grpc");
    }
}
