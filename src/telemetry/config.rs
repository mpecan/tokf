use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Protocol {
    #[default]
    Http,
    Grpc,
}

impl Protocol {
    /// Parse a protocol string (case-insensitive). Returns `Http` for any unrecognised value.
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "grpc" => Self::Grpc,
            _ => Self::Http,
        }
    }
}

/// Runtime telemetry configuration merged from the optional config file and environment variables.
///
/// Environment variables take precedence over the config file:
/// - `TOKF_TELEMETRY_ENABLED` — `true`, `1`, or `yes` to enable
/// - `OTEL_EXPORTER_OTLP_ENDPOINT` — OTLP collector endpoint
/// - `OTEL_EXPORTER_OTLP_PROTOCOL` — `http` (default) or `grpc`
/// - `OTEL_EXPORTER_OTLP_HEADERS` — comma-separated `key=value` headers
/// - `OTEL_RESOURCE_ATTRIBUTES` — OpenTelemetry resource attributes; `service.name` is extracted
#[derive(Debug, Clone)]
pub struct TelemetryConfig {
    pub enabled: bool,
    pub endpoint: String,
    pub protocol: Protocol,
    pub headers: HashMap<String, String>,
    pub service_name: String,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: "http://localhost:4318".to_string(),
            protocol: Protocol::default(),
            headers: HashMap::new(),
            service_name: "tokf".to_string(),
        }
    }
}

fn parse_headers(headers_str: &str) -> HashMap<String, String> {
    headers_str
        .split(',')
        .filter_map(|kv| {
            let mut parts = kv.splitn(2, '=');
            let key = parts.next()?.trim().to_string();
            let value = parts.next()?.trim().to_string();
            if key.is_empty() {
                None
            } else {
                Some((key, value))
            }
        })
        .collect()
}

fn apply_toml(config: &mut TelemetryConfig, table: &toml::Table) {
    let Some(telemetry) = table.get("telemetry").and_then(toml::Value::as_table) else {
        return;
    };
    if let Some(enabled) = telemetry.get("enabled").and_then(toml::Value::as_bool) {
        config.enabled = enabled;
    }
    if let Some(endpoint) = telemetry.get("endpoint").and_then(toml::Value::as_str) {
        config.endpoint = endpoint.to_string();
    }
    if let Some(protocol) = telemetry.get("protocol").and_then(toml::Value::as_str) {
        config.protocol = Protocol::parse(protocol);
    }
    if let Some(service_name) = telemetry.get("service_name").and_then(toml::Value::as_str) {
        config.service_name = service_name.to_string();
    }
}

/// Load `TelemetryConfig` by merging the optional config file with environment variables.
/// Environment variables take precedence over the file.
pub fn load() -> TelemetryConfig {
    let mut config = TelemetryConfig::default();

    // Load from file (optional)
    if let Some(cfg_dir) = dirs::config_dir() {
        let cfg_path = cfg_dir.join("tokf").join("config.toml");
        if cfg_path.exists()
            && let Ok(content) = std::fs::read_to_string(&cfg_path)
            && let Ok(table) = content.parse::<toml::Table>()
        {
            apply_toml(&mut config, &table);
        }
    }

    // Override with env vars
    if let Ok(val) = std::env::var("TOKF_TELEMETRY_ENABLED") {
        config.enabled = matches!(val.to_ascii_lowercase().as_str(), "true" | "1" | "yes");
    }
    if let Ok(val) = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        config.endpoint = val;
    }
    if let Ok(val) = std::env::var("OTEL_EXPORTER_OTLP_PROTOCOL") {
        config.protocol = Protocol::parse(&val);
    }
    if let Ok(val) = std::env::var("OTEL_EXPORTER_OTLP_HEADERS") {
        config.headers = parse_headers(&val);
    }
    if let Ok(attrs) = std::env::var("OTEL_RESOURCE_ATTRIBUTES")
        && let Some(name) = attrs.split(',').find_map(|kv| {
            let mut parts = kv.splitn(2, '=');
            let key = parts.next()?.trim();
            let value = parts.next()?.trim();
            if key == "service.name" {
                Some(value.to_string())
            } else {
                None
            }
        })
    {
        config.service_name = name;
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = TelemetryConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.endpoint, "http://localhost:4318");
        assert_eq!(config.protocol, Protocol::Http);
        assert_eq!(config.service_name, "tokf");
        assert!(config.headers.is_empty());
    }

    #[test]
    fn test_protocol_parse() {
        assert_eq!(Protocol::parse("grpc"), Protocol::Grpc);
        assert_eq!(Protocol::parse("GRPC"), Protocol::Grpc);
        assert_eq!(Protocol::parse("http"), Protocol::Http);
        assert_eq!(Protocol::parse("unknown"), Protocol::Http);
    }

    #[test]
    fn test_parse_headers() {
        let headers = parse_headers("key1=val1,key2=val2");
        assert_eq!(headers.get("key1"), Some(&"val1".to_string()));
        assert_eq!(headers.get("key2"), Some(&"val2".to_string()));
    }

    #[test]
    fn test_parse_headers_empty() {
        let headers = parse_headers("");
        assert!(headers.is_empty());
    }

    #[test]
    fn test_apply_toml_telemetry_section() {
        let toml_str = r#"
[telemetry]
enabled = true
endpoint = "http://otel.example.com:4318"
protocol = "grpc"
service_name = "my-service"
"#;
        let table: toml::Table = toml_str.parse().expect("valid toml");
        let mut config = TelemetryConfig::default();
        apply_toml(&mut config, &table);

        assert!(config.enabled);
        assert_eq!(config.endpoint, "http://otel.example.com:4318");
        assert_eq!(config.protocol, Protocol::Grpc);
        assert_eq!(config.service_name, "my-service");
    }

    #[test]
    fn test_apply_toml_missing_section() {
        let table: toml::Table = "[other]\nkey = \"val\"\n".parse().expect("valid toml");
        let mut config = TelemetryConfig::default();
        apply_toml(&mut config, &table);
        // defaults unchanged
        assert!(!config.enabled);
    }

    // -----------------------------------------------------------------------
    // env var override tests — serialised to avoid cross-test contamination
    // -----------------------------------------------------------------------

    #[test]
    #[serial_test::serial]
    fn test_load_enabled_via_env() {
        for val in ["true", "1", "yes", "TRUE", "YES"] {
            // SAFETY: single-threaded via serial_test; no other thread reads this var.
            unsafe { std::env::set_var("TOKF_TELEMETRY_ENABLED", val) };
            let cfg = load();
            assert!(
                cfg.enabled,
                "expected enabled for TOKF_TELEMETRY_ENABLED={val}"
            );
        }
        for val in ["false", "0", "no", "off", ""] {
            unsafe { std::env::set_var("TOKF_TELEMETRY_ENABLED", val) };
            let cfg = load();
            assert!(
                !cfg.enabled,
                "expected disabled for TOKF_TELEMETRY_ENABLED={val}"
            );
        }
        unsafe { std::env::remove_var("TOKF_TELEMETRY_ENABLED") };
    }

    #[test]
    #[serial_test::serial]
    fn test_load_endpoint_via_env() {
        // SAFETY: single-threaded via serial_test.
        unsafe {
            std::env::set_var(
                "OTEL_EXPORTER_OTLP_ENDPOINT",
                "http://otel.example.com:4317",
            )
        };
        let cfg = load();
        assert_eq!(cfg.endpoint, "http://otel.example.com:4317");
        unsafe { std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT") };
    }

    #[test]
    #[serial_test::serial]
    fn test_load_protocol_via_env() {
        // SAFETY: single-threaded via serial_test.
        unsafe { std::env::set_var("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc") };
        let cfg = load();
        assert_eq!(cfg.protocol, Protocol::Grpc);
        unsafe { std::env::remove_var("OTEL_EXPORTER_OTLP_PROTOCOL") };
    }

    #[test]
    #[serial_test::serial]
    fn test_load_headers_via_env() {
        // SAFETY: single-threaded via serial_test.
        unsafe { std::env::set_var("OTEL_EXPORTER_OTLP_HEADERS", "x-api-key=secret,x-team=eng") };
        let cfg = load();
        assert_eq!(cfg.headers.get("x-api-key"), Some(&"secret".to_string()));
        assert_eq!(cfg.headers.get("x-team"), Some(&"eng".to_string()));
        unsafe { std::env::remove_var("OTEL_EXPORTER_OTLP_HEADERS") };
    }

    #[test]
    #[serial_test::serial]
    fn test_load_service_name_from_resource_attributes() {
        // SAFETY: single-threaded via serial_test.
        unsafe {
            std::env::set_var(
                "OTEL_RESOURCE_ATTRIBUTES",
                "deployment.env=prod,service.name=my-tokf,team=platform",
            );
        }
        let cfg = load();
        assert_eq!(cfg.service_name, "my-tokf");
        unsafe { std::env::remove_var("OTEL_RESOURCE_ATTRIBUTES") };
    }

    #[test]
    #[serial_test::serial]
    fn test_load_resource_attributes_without_service_name() {
        // SAFETY: single-threaded via serial_test.
        unsafe { std::env::set_var("OTEL_RESOURCE_ATTRIBUTES", "deployment.env=prod") };
        let cfg = load();
        // service_name unchanged — falls back to default
        assert_eq!(cfg.service_name, "tokf");
        unsafe { std::env::remove_var("OTEL_RESOURCE_ATTRIBUTES") };
    }
}
