pub struct Config {
    pub port: u16,
    pub database_url: Option<String>,
    /// When `false`, the server starts without applying migrations.
    /// Set `RUN_MIGRATIONS=false` to manage migrations out-of-band (e.g. a
    /// dedicated migration job in Kubernetes).  Defaults to `true`.
    pub run_migrations: bool,
    pub r2_bucket: Option<String>,
    pub r2_access_key_id: Option<String>,
    pub r2_secret_access_key: Option<String>,
    pub r2_endpoint: Option<String>,
}

// R10: Custom Debug masks secrets so the struct is safe to log.
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("port", &self.port)
            .field(
                "database_url",
                &self.database_url.as_deref().map(|_| "<redacted>"),
            )
            .field("run_migrations", &self.run_migrations)
            .field("r2_bucket", &self.r2_bucket)
            .field(
                "r2_access_key_id",
                &self.r2_access_key_id.as_deref().map(|_| "<redacted>"),
            )
            .field(
                "r2_secret_access_key",
                &self.r2_secret_access_key.as_deref().map(|_| "<redacted>"),
            )
            .field("r2_endpoint", &self.r2_endpoint)
            .finish()
    }
}

impl Config {
    pub fn from_env() -> Self {
        // R12: warn when PORT is set but invalid so misconfiguration is visible.
        let port = std::env::var("PORT").ok().map_or(8080, |s| {
            match s.parse::<u16>() {
                Ok(0) => {
                    tracing::warn!(
                        "PORT env var {s:?} is not a valid port number (1-65535), defaulting to 8080"
                    );
                    8080
                }
                Ok(port) => port,
                Err(_) => {
                    tracing::warn!(
                        "PORT env var {s:?} is not a valid port number (1-65535), defaulting to 8080"
                    );
                    8080
                }
            }
        });
        let run_migrations = std::env::var("RUN_MIGRATIONS")
            .map(|v| !matches!(v.to_lowercase().as_str(), "false" | "0" | "no"))
            .unwrap_or(true);
        Self {
            port,
            database_url: std::env::var("DATABASE_URL").ok(),
            run_migrations,
            r2_bucket: std::env::var("R2_BUCKET").ok(),
            r2_access_key_id: std::env::var("R2_ACCESS_KEY_ID").ok(),
            r2_secret_access_key: std::env::var("R2_SECRET_ACCESS_KEY").ok(),
            r2_endpoint: std::env::var("R2_ENDPOINT").ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use std::sync::Mutex;

    // Serialize env-mutating tests to avoid races between parallel test threads.
    // SAFETY: The Mutex ensures exclusive env access within this process; lock
    // poisoning is recovered via into_inner() so a panicking test won't block
    // subsequent ones.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn defaults_to_port_8080() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: protected by ENV_LOCK; no concurrent env mutations
        unsafe { std::env::remove_var("PORT") };
        let cfg = Config::from_env();
        assert_eq!(cfg.port, 8080);
    }

    #[test]
    fn reads_port_from_env() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: protected by ENV_LOCK; no concurrent env mutations
        unsafe { std::env::set_var("PORT", "9090") };
        let cfg = Config::from_env();
        unsafe { std::env::remove_var("PORT") };
        assert_eq!(cfg.port, 9090);
    }

    #[test]
    fn invalid_port_falls_back_to_default() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: protected by ENV_LOCK; no concurrent env mutations
        unsafe { std::env::set_var("PORT", "not-a-number") };
        let cfg = Config::from_env();
        unsafe { std::env::remove_var("PORT") };
        assert_eq!(cfg.port, 8080);
    }

    #[test]
    fn reads_optional_fields_from_env() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: protected by ENV_LOCK; no concurrent env mutations
        unsafe {
            std::env::set_var("DATABASE_URL", "postgres://localhost/tokf");
            std::env::set_var("R2_BUCKET", "my-bucket");
        }
        let cfg = Config::from_env();
        unsafe {
            std::env::remove_var("DATABASE_URL");
            std::env::remove_var("R2_BUCKET");
        }
        assert_eq!(
            cfg.database_url.as_deref(),
            Some("postgres://localhost/tokf")
        );
        assert_eq!(cfg.r2_bucket.as_deref(), Some("my-bucket"));
    }

    #[test]
    fn run_migrations_defaults_to_true() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: protected by ENV_LOCK; no concurrent env mutations
        unsafe { std::env::remove_var("RUN_MIGRATIONS") };
        let cfg = Config::from_env();
        assert!(cfg.run_migrations, "should default to true");
    }

    #[test]
    fn run_migrations_can_be_disabled() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: protected by ENV_LOCK; no concurrent env mutations
        unsafe { std::env::set_var("RUN_MIGRATIONS", "false") };
        let cfg = Config::from_env();
        unsafe { std::env::remove_var("RUN_MIGRATIONS") };
        assert!(!cfg.run_migrations);
    }

    #[test]
    fn debug_masks_secrets() {
        let cfg = Config {
            port: 8080,
            database_url: Some("postgres://secret".to_string()),
            run_migrations: true,
            r2_bucket: Some("my-bucket".to_string()),
            r2_access_key_id: Some("key-id".to_string()),
            r2_secret_access_key: Some("super-secret".to_string()),
            r2_endpoint: Some("https://r2.example.com".to_string()),
        };
        let debug_str = format!("{cfg:?}");
        assert!(!debug_str.contains("postgres://secret"));
        assert!(!debug_str.contains("key-id"));
        assert!(!debug_str.contains("super-secret"));
        assert!(debug_str.contains("<redacted>"));
        // Non-secret fields should be visible
        assert!(debug_str.contains("8080"));
        assert!(debug_str.contains("my-bucket"));
    }
}
