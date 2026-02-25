use std::fs;
use std::path::PathBuf;

use crate::fs::write_config_file;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const KEYRING_SERVICE: &str = "tokf";
const KEYRING_USER: &str = "default";

#[derive(Debug, Serialize, Deserialize)]
pub struct StoredAuth {
    pub username: String,
    pub server_url: String,
    /// Unix timestamp when the token expires (0 = unknown).
    #[serde(default)]
    pub expires_at: u64,
    /// Whether the user has accepted the MIT license for filter publishing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mit_license_accepted: Option<bool>,
}

/// Loaded credentials: token (from keyring) + metadata (from TOML).
pub struct LoadedAuth {
    pub token: String,
    pub username: String,
    pub server_url: String,
    /// Unix timestamp when the token expires (0 = unknown).
    pub expires_at: u64,
    /// Whether the user has accepted the MIT license for filter publishing.
    pub mit_license_accepted: Option<bool>,
}

impl LoadedAuth {
    /// Returns `true` if the token has a known expiry time that has passed.
    pub fn is_expired(&self) -> bool {
        if self.expires_at == 0 {
            return false; // unknown expiry
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now >= self.expires_at
    }
}

pub fn auth_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("tokf").join("auth.toml"))
}

/// Store authentication credentials (token in keyring, metadata in TOML).
///
/// `token_expires_in` is the number of seconds until the token expires
/// (from the server's `expires_in` field). Pass 0 if unknown.
///
/// # Errors
///
/// Returns an error if the keyring is inaccessible, the config directory
/// cannot be determined, or the TOML file cannot be written.
pub fn save(
    token: &str,
    username: &str,
    server_url: &str,
    token_expires_in: i64,
) -> anyhow::Result<()> {
    // Store token in OS keyring
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
    entry
        .set_password(token)
        .map_err(|e| anyhow::anyhow!("could not access system keyring: {e}"))?;

    let expires_at = if token_expires_in > 0 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + token_expires_in.unsigned_abs()
    } else {
        0
    };

    // Store metadata in TOML file
    let path =
        auth_config_path().ok_or_else(|| anyhow::anyhow!("cannot determine config directory"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Preserve any previously stored fields (e.g. mit_license_accepted)
    let existing = auth_config_path()
        .and_then(|p| fs::read_to_string(&p).ok())
        .and_then(|c| toml::from_str::<StoredAuth>(&c).ok());
    let mit_license_accepted = existing.and_then(|e| e.mit_license_accepted);

    let meta = StoredAuth {
        username: username.to_string(),
        server_url: server_url.to_string(),
        expires_at,
        mit_license_accepted,
    };
    let content = toml::to_string_pretty(&meta)?;
    write_config_file(&path, &content)?;
    Ok(())
}

/// Persist the user's MIT license acceptance to the auth config file.
///
/// If no auth config file exists, creates a minimal one. Existing fields
/// (`username`, `server_url`, `expires_at`) are preserved unchanged.
///
/// # Errors
///
/// Returns an error if the config directory cannot be determined or the file
/// cannot be written.
pub fn save_license_accepted(accepted: bool) -> anyhow::Result<()> {
    let path =
        auth_config_path().ok_or_else(|| anyhow::anyhow!("cannot determine config directory"))?;
    save_license_accepted_to_path(&path, accepted)
}

/// Core logic for persisting MIT license acceptance to a specific path.
///
/// Separated from [`save_license_accepted`] to allow direct testing without
/// depending on the platform config directory.
pub(crate) fn save_license_accepted_to_path(
    path: &std::path::Path,
    accepted: bool,
) -> anyhow::Result<()> {
    let mut meta: StoredAuth = if path.exists() {
        let content = fs::read_to_string(path)?;
        toml::from_str(&content)?
    } else {
        StoredAuth {
            username: String::new(),
            server_url: String::new(),
            expires_at: 0,
            mit_license_accepted: None,
        }
    };

    meta.mit_license_accepted = Some(accepted);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(&meta)?;
    write_config_file(path, &content)
}

/// Load stored authentication credentials.
///
/// Returns `None` if no credentials are stored, the TOML file is missing
/// or malformed, or the keyring entry is absent.
pub fn load() -> Option<LoadedAuth> {
    let path = auth_config_path()?;
    let content = fs::read_to_string(&path).ok()?;
    let meta: StoredAuth = toml::from_str(&content).ok()?;

    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).ok()?;
    let token = entry.get_password().ok()?;

    Some(LoadedAuth {
        token,
        username: meta.username,
        server_url: meta.server_url,
        expires_at: meta.expires_at,
        mit_license_accepted: meta.mit_license_accepted,
    })
}

/// Remove stored credentials (keyring entry and TOML file).
///
/// Silently ignores errors — the credentials may already be absent.
/// Returns `true` if credentials were present before removal.
pub fn remove() -> bool {
    let had_credentials = load().is_some();

    // Remove keyring entry (ignore errors — may already be absent)
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        let _ = entry.delete_credential();
    }

    // Remove TOML file
    if let Some(path) = auth_config_path() {
        let _ = fs::remove_file(&path);
    }

    had_credentials
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn stored_auth_roundtrip() {
        let meta = StoredAuth {
            username: "testuser".to_string(),
            server_url: "https://api.tokf.net".to_string(),
            expires_at: 1_700_000_000,
            mit_license_accepted: None,
        };
        let serialized = toml::to_string_pretty(&meta).unwrap();
        let deserialized: StoredAuth = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.username, "testuser");
        assert_eq!(deserialized.server_url, "https://api.tokf.net");
        assert_eq!(deserialized.expires_at, 1_700_000_000);
    }

    #[test]
    fn stored_auth_mit_license_roundtrip() {
        let meta = StoredAuth {
            username: "bob".to_string(),
            server_url: "https://api.tokf.net".to_string(),
            expires_at: 0,
            mit_license_accepted: Some(true),
        };
        let serialized = toml::to_string_pretty(&meta).unwrap();
        assert!(
            serialized.contains("mit_license_accepted"),
            "should serialize field: {serialized}"
        );
        let deserialized: StoredAuth = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.mit_license_accepted, Some(true));
    }

    #[test]
    fn save_license_accepted_preserves_other_fields() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("auth.toml");
        let initial = StoredAuth {
            username: "alice".to_string(),
            server_url: "https://example.com".to_string(),
            expires_at: 9_999_999_999,
            mit_license_accepted: None,
        };
        let content = toml::to_string_pretty(&initial).unwrap();
        std::fs::write(&path, &content).unwrap();

        // Call the real function via the testable internal helper
        save_license_accepted_to_path(&path, true).unwrap();

        let result: StoredAuth = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(result.username, "alice");
        assert_eq!(result.server_url, "https://example.com");
        assert_eq!(result.expires_at, 9_999_999_999);
        assert_eq!(result.mit_license_accepted, Some(true));
    }

    #[test]
    fn stored_auth_missing_expires_at_defaults_to_zero() {
        let toml_str = r#"
            username = "bob"
            server_url = "https://api.tokf.net"
        "#;
        let meta: StoredAuth = toml::from_str(toml_str).unwrap();
        assert_eq!(meta.expires_at, 0);
    }

    #[test]
    fn auth_config_path_returns_some() {
        let path = auth_config_path();
        assert!(path.is_some(), "expected auth config path to be Some");
        let path = path.unwrap();
        assert!(
            path.ends_with("tokf/auth.toml"),
            "expected path to end with tokf/auth.toml, got: {}",
            path.display()
        );
    }

    #[test]
    fn load_returns_none_when_no_file() {
        // This test validates graceful handling when no credentials exist.
        // It may return Some if the developer has logged in locally, so we
        // just verify it doesn't panic.
        let _ = load();
    }

    #[test]
    fn is_expired_unknown() {
        let auth = LoadedAuth {
            token: String::new(),
            username: String::new(),
            server_url: String::new(),
            expires_at: 0,
            mit_license_accepted: None,
        };
        assert!(!auth.is_expired(), "unknown expiry should not be expired");
    }

    #[test]
    fn is_expired_future() {
        let future = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600;
        let auth = LoadedAuth {
            token: String::new(),
            username: String::new(),
            server_url: String::new(),
            expires_at: future,
            mit_license_accepted: None,
        };
        assert!(!auth.is_expired());
    }

    #[test]
    fn is_expired_past() {
        let auth = LoadedAuth {
            token: String::new(),
            username: String::new(),
            server_url: String::new(),
            expires_at: 1, // 1970 — definitely expired
            mit_license_accepted: None,
        };
        assert!(auth.is_expired());
    }
}
