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
        config::Protocol::Http => check_http(&cfg.endpoint),
        config::Protocol::Grpc => check_grpc(&cfg.endpoint),
    }
}

fn check_http(endpoint: &str) -> anyhow::Result<i32> {
    let url = format!("{}/v1/metrics", endpoint.trim_end_matches('/'));
    let start = Instant::now();
    let result = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()?
        .post(&url)
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

fn check_grpc(endpoint: &str) -> anyhow::Result<i32> {
    // Parse host:port from the endpoint URL for a raw TCP connect check.
    let addr = parse_host_port(endpoint)?;
    let start = Instant::now();
    let result = std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(3));

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

fn parse_host_port(endpoint: &str) -> anyhow::Result<std::net::SocketAddr> {
    use std::net::ToSocketAddrs;

    // Strip scheme if present (http:// or https://)
    let without_scheme = endpoint
        .strip_prefix("https://")
        .or_else(|| endpoint.strip_prefix("http://"))
        .unwrap_or(endpoint);

    // Strip trailing path
    let host_port = without_scheme.split('/').next().unwrap_or(without_scheme);

    host_port
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow::anyhow!("could not resolve endpoint address: {endpoint}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_host_port_http() {
        let addr = parse_host_port("http://127.0.0.1:4317").unwrap();
        assert_eq!(addr.port(), 4317);
    }

    #[test]
    fn parse_host_port_https() {
        let addr = parse_host_port("https://127.0.0.1:4318").unwrap();
        assert_eq!(addr.port(), 4318);
    }

    #[test]
    fn parse_host_port_with_path() {
        let addr = parse_host_port("http://127.0.0.1:9090/v1/metrics").unwrap();
        assert_eq!(addr.port(), 9090);
    }

    #[test]
    fn parse_host_port_no_scheme() {
        let addr = parse_host_port("127.0.0.1:4317").unwrap();
        assert_eq!(addr.port(), 4317);
    }

    #[test]
    fn parse_host_port_missing_port() {
        assert!(parse_host_port("http://localhost").is_err());
    }

    #[test]
    fn parse_host_port_empty() {
        assert!(parse_host_port("").is_err());
    }

    #[test]
    fn parse_host_port_ipv6() {
        let addr = parse_host_port("http://[::1]:4317").unwrap();
        assert_eq!(addr.port(), 4317);
        assert!(addr.ip().is_loopback());
    }

    #[test]
    fn test_protocol_display() {
        assert_eq!(format!("{}", config::Protocol::Http), "http");
        assert_eq!(format!("{}", config::Protocol::Grpc), "grpc");
    }
}
