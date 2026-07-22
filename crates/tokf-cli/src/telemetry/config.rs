use std::collections::HashMap;

use crate::runtime::Runtime;

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

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http => f.write_str("http"),
            Self::Grpc => f.write_str("grpc"),
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
    if let Some(headers) = telemetry.get("headers").and_then(toml::Value::as_table) {
        for (key, value) in headers {
            if let Some(v) = value.as_str() {
                config.headers.insert(key.clone(), v.to_string());
            }
        }
    }
}

/// Returns the conventional default OTLP endpoint for the given protocol.
const fn default_endpoint(protocol: &Protocol) -> &'static str {
    match protocol {
        Protocol::Http => "http://localhost:4318",
        Protocol::Grpc => "http://localhost:4317",
    }
}

/// Load `TelemetryConfig` by merging the optional config file with the
/// environment captured in `rt`. The environment takes precedence over the file.
pub fn load(rt: &Runtime) -> TelemetryConfig {
    let mut config = TelemetryConfig::default();
    let mut endpoint_explicitly_set = false;

    // Load from file (optional) — the runtime resolves TOKF_HOME, so this is
    // consistent with every other config path in tokf.
    if let Some(cfg_dir) = rt.user_dir() {
        let cfg_path = cfg_dir.join("config.toml");
        if cfg_path.exists()
            && let Ok(content) = std::fs::read_to_string(&cfg_path)
            && let Ok(table) = content.parse::<toml::Table>()
        {
            if let Some(telemetry) = table.get("telemetry").and_then(toml::Value::as_table) {
                endpoint_explicitly_set = telemetry.contains_key("endpoint");
            }
            apply_toml(&mut config, &table);
        }
    }

    // Override with the captured environment
    let otel = rt.otel();
    if let Some(val) = otel.telemetry_enabled.as_ref() {
        config.enabled = matches!(val.to_ascii_lowercase().as_str(), "true" | "1" | "yes");
    }
    if let Some(val) = otel.endpoint.clone() {
        config.endpoint = val;
        endpoint_explicitly_set = true;
    }
    if let Some(val) = otel.protocol.as_ref() {
        config.protocol = Protocol::parse(val);
    }
    if let Some(val) = otel.headers.as_ref() {
        config.headers = parse_headers(val);
    }
    if let Some(attrs) = otel.resource_attributes.as_ref()
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

    // When the protocol was changed but the endpoint was not explicitly set,
    // adjust the endpoint to match the protocol's conventional default port.
    if !endpoint_explicitly_set {
        config.endpoint = default_endpoint(&config.protocol).to_string();
    }

    config
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::runtime::OtelEnv;

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
    fn test_default_endpoint_per_protocol() {
        assert_eq!(default_endpoint(&Protocol::Http), "http://localhost:4318");
        assert_eq!(default_endpoint(&Protocol::Grpc), "http://localhost:4317");
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
    fn test_apply_toml_headers() {
        let toml_str = r#"
[telemetry]
enabled = true

[telemetry.headers]
x-api-key = "secret"
x-team = "platform"
"#;
        let table: toml::Table = toml_str.parse().expect("valid toml");
        let mut config = TelemetryConfig::default();
        apply_toml(&mut config, &table);

        assert_eq!(config.headers.get("x-api-key"), Some(&"secret".to_string()));
        assert_eq!(config.headers.get("x-team"), Some(&"platform".to_string()));
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
    // Environment-override tests.
    //
    // These build the captured environment directly rather than mutating the
    // process, so they need no `#[serial]` and cannot contaminate each other.
    // -----------------------------------------------------------------------

    fn rt_with(otel: OtelEnv) -> crate::runtime::Runtime {
        crate::runtime::Runtime::builder().otel(otel).build()
    }

    #[test]
    fn load_enabled_via_env() {
        for val in ["true", "1", "yes", "TRUE", "YES"] {
            let rt = rt_with(OtelEnv {
                telemetry_enabled: Some(val.to_string()),
                ..OtelEnv::default()
            });
            assert!(load(&rt).enabled, "expected enabled for {val}");
        }
        for val in ["false", "0", "no", "off", ""] {
            let rt = rt_with(OtelEnv {
                telemetry_enabled: Some(val.to_string()),
                ..OtelEnv::default()
            });
            assert!(!load(&rt).enabled, "expected disabled for {val}");
        }
    }

    #[test]
    fn load_endpoint_via_env() {
        let rt = rt_with(OtelEnv {
            endpoint: Some("http://collector:4318".to_string()),
            ..OtelEnv::default()
        });
        assert_eq!(load(&rt).endpoint, "http://collector:4318");
    }

    #[test]
    fn load_protocol_via_env() {
        let rt = rt_with(OtelEnv {
            protocol: Some("grpc".to_string()),
            ..OtelEnv::default()
        });
        let cfg = load(&rt);
        assert_eq!(cfg.protocol, Protocol::Grpc);
        // No explicit endpoint, so it follows the protocol's default port.
        assert_eq!(cfg.endpoint, "http://localhost:4317");
    }

    #[test]
    fn load_grpc_protocol_keeps_an_explicit_endpoint() {
        let rt = rt_with(OtelEnv {
            protocol: Some("grpc".to_string()),
            endpoint: Some("http://custom:1234".to_string()),
            ..OtelEnv::default()
        });
        let cfg = load(&rt);
        assert_eq!(cfg.protocol, Protocol::Grpc);
        assert_eq!(cfg.endpoint, "http://custom:1234");
    }

    #[test]
    fn load_headers_via_env() {
        let rt = rt_with(OtelEnv {
            headers: Some("api-key=secret,x-scope=team".to_string()),
            ..OtelEnv::default()
        });
        let cfg = load(&rt);
        assert_eq!(cfg.headers.get("api-key"), Some(&"secret".to_string()));
        assert_eq!(cfg.headers.get("x-scope"), Some(&"team".to_string()));
    }

    #[test]
    fn load_service_name_from_resource_attributes() {
        let rt = rt_with(OtelEnv {
            resource_attributes: Some("deployment=prod,service.name=tokf-ci".to_string()),
            ..OtelEnv::default()
        });
        assert_eq!(load(&rt).service_name, "tokf-ci");
    }

    #[test]
    fn load_resource_attributes_without_service_name_keeps_the_default() {
        let rt = rt_with(OtelEnv {
            resource_attributes: Some("deployment=prod,region=eu".to_string()),
            ..OtelEnv::default()
        });
        assert_eq!(load(&rt).service_name, "tokf");
    }

    #[test]
    fn load_reads_config_from_the_runtime_user_dir() {
        let rt = crate::runtime::Runtime::isolated();
        let dir = rt.user_dir().expect("isolated runtime has a user dir");
        std::fs::create_dir_all(&dir).expect("create config dir");
        std::fs::write(
            dir.join("config.toml"),
            "[telemetry]\nenabled = true\nendpoint = \"http://from-file:4318\"\n",
        )
        .expect("write config");

        let cfg = load(&rt);
        assert!(cfg.enabled, "config file should enable telemetry");
        assert_eq!(cfg.endpoint, "http://from-file:4318");
    }

    #[test]
    fn the_environment_overrides_the_config_file() {
        let rt = crate::runtime::Runtime::isolated();
        let dir = rt.user_dir().expect("isolated runtime has a user dir");
        std::fs::create_dir_all(&dir).expect("create config dir");
        std::fs::write(
            dir.join("config.toml"),
            "[telemetry]\nenabled = true\nendpoint = \"http://from-file:4318\"\n",
        )
        .expect("write config");

        let rt = crate::runtime::Runtime::builder()
            .home(dir)
            .otel(OtelEnv {
                endpoint: Some("http://from-env:4318".to_string()),
                ..OtelEnv::default()
            })
            .build();

        assert_eq!(load(&rt).endpoint, "http://from-env:4318");
    }
}
