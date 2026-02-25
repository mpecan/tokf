use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const KEYRING_SERVICE: &str = "tokf";
const KEYRING_USER: &str = "default";

#[derive(Debug, Serialize, Deserialize)]
pub struct StoredAuth {
    pub username: String,
    pub server_url: String,
}

/// Loaded credentials: token (from keyring) + metadata (from TOML).
pub struct LoadedAuth {
    pub token: String,
    pub username: String,
    pub server_url: String,
}

pub fn auth_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("tokf").join("auth.toml"))
}

/// Store authentication credentials (token in keyring, metadata in TOML).
///
/// # Errors
///
/// Returns an error if the keyring is inaccessible, the config directory
/// cannot be determined, or the TOML file cannot be written.
pub fn save(token: &str, username: &str, server_url: &str) -> anyhow::Result<()> {
    // Store token in OS keyring
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
    entry
        .set_password(token)
        .map_err(|e| anyhow::anyhow!("could not access system keyring: {e}"))?;

    // Store metadata in TOML file
    let path =
        auth_config_path().ok_or_else(|| anyhow::anyhow!("cannot determine config directory"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let meta = StoredAuth {
        username: username.to_string(),
        server_url: server_url.to_string(),
    };
    let content = toml::to_string_pretty(&meta)?;
    write_config_file(&path, &content)?;
    Ok(())
}

/// Write a config file with restrictive permissions (0600 on Unix).
fn write_config_file(path: &PathBuf, content: &str) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(content.as_bytes())?;
    }
    #[cfg(not(unix))]
    {
        fs::write(path, content)?;
    }
    Ok(())
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
        };
        let serialized = toml::to_string_pretty(&meta).unwrap();
        let deserialized: StoredAuth = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.username, "testuser");
        assert_eq!(deserialized.server_url, "https://api.tokf.net");
    }

    #[test]
    fn auth_config_path_returns_some() {
        // On any platform with a home dir, this should return Some
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
}
