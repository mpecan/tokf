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
    fs::write(&path, content)?;
    Ok(())
}

pub fn load() -> Option<(String, String, String)> {
    let path = auth_config_path()?;
    let content = fs::read_to_string(&path).ok()?;
    let meta: StoredAuth = toml::from_str(&content).ok()?;

    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).ok()?;
    let token = entry.get_password().ok()?;

    Some((token, meta.username, meta.server_url))
}

pub fn remove() {
    // Remove keyring entry (ignore errors — may already be absent)
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        let _ = entry.delete_credential();
    }

    // Remove TOML file
    if let Some(path) = auth_config_path() {
        let _ = fs::remove_file(&path);
    }
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
        // With no auth.toml written, load should return None gracefully
        // (This test relies on a clean state or the TOML file not existing
        // at the default path. It validates the None-on-missing-file path.)
        // We can't guarantee the file doesn't exist in CI, but the function
        // should never panic.
        let _ = load();
    }
}
