use std::fs;
use std::path::PathBuf;

use crate::fs::write_config_file;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::runtime::Runtime;

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
    /// Highest Terms of Service version the user has accepted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tos_accepted_version: Option<i64>,
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
    /// Highest Terms of Service version the user has accepted.
    pub tos_accepted_version: Option<i64>,
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

pub fn auth_config_path(rt: &Runtime) -> Option<PathBuf> {
    rt.user_dir().map(|d| d.join("auth.toml"))
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
    rt: &Runtime,
    token: &str,
    username: &str,
    server_url: &str,
    token_expires_in: i64,
) -> anyhow::Result<()> {
    // Store token in OS keyring
    let entry = keyring_entry(rt)?;
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
        auth_config_path(rt).ok_or_else(|| anyhow::anyhow!("cannot determine config directory"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Preserve any previously stored fields (e.g. mit_license_accepted, tos_accepted_version)
    let existing = auth_config_path(rt)
        .and_then(|p| fs::read_to_string(&p).ok())
        .and_then(|c| toml::from_str::<StoredAuth>(&c).ok());
    let mit_license_accepted = existing.as_ref().and_then(|e| e.mit_license_accepted);
    let tos_accepted_version = existing.as_ref().and_then(|e| e.tos_accepted_version);

    let meta = StoredAuth {
        username: username.to_string(),
        server_url: server_url.to_string(),
        expires_at,
        mit_license_accepted,
        tos_accepted_version,
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
pub fn save_license_accepted(rt: &Runtime, accepted: bool) -> anyhow::Result<()> {
    let path =
        auth_config_path(rt).ok_or_else(|| anyhow::anyhow!("cannot determine config directory"))?;
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
    update_stored_auth(path, |meta| meta.mit_license_accepted = Some(accepted))
}

/// Persist the user's `ToS` acceptance version to the auth config file.
///
/// If no auth config file exists, creates a minimal one. Existing fields
/// are preserved unchanged.
///
/// # Errors
///
/// Returns an error if the config directory cannot be determined or the file
/// cannot be written.
pub fn save_tos_accepted_version(rt: &Runtime, version: i64) -> anyhow::Result<()> {
    let path =
        auth_config_path(rt).ok_or_else(|| anyhow::anyhow!("cannot determine config directory"))?;
    save_tos_accepted_version_to_path(&path, version)
}

/// Core logic for persisting `ToS` acceptance version to a specific path.
///
/// Separated from [`save_tos_accepted_version`] to allow direct testing
/// without depending on the platform config directory.
pub(crate) fn save_tos_accepted_version_to_path(
    path: &std::path::Path,
    version: i64,
) -> anyhow::Result<()> {
    update_stored_auth(path, |meta| meta.tos_accepted_version = Some(version))
}

/// Load-modify-save helper for [`StoredAuth`].
///
/// Reads the existing file (or creates a default), applies `mutate`, and
/// writes back. Ensures the parent directory exists.
fn update_stored_auth(
    path: &std::path::Path,
    mutate: impl FnOnce(&mut StoredAuth),
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
            tos_accepted_version: None,
        }
    };

    mutate(&mut meta);

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
pub fn load(rt: &Runtime) -> Option<LoadedAuth> {
    let path = auth_config_path(rt)?;
    let content = fs::read_to_string(&path).ok()?;
    let meta: StoredAuth = toml::from_str(&content).ok()?;

    let entry = keyring_entry(rt).ok()?;
    let token = entry.get_password().ok()?;

    Some(LoadedAuth {
        token,
        username: meta.username,
        server_url: meta.server_url,
        expires_at: meta.expires_at,
        mit_license_accepted: meta.mit_license_accepted,
        tos_accepted_version: meta.tos_accepted_version,
    })
}

/// Guards mock-store installation so it happens exactly once per process.
///
/// `keyring_core`'s default store is process-global and first-write-wins, so
/// installing it more than once would either be ignored or swap in a fresh,
/// empty store underneath a test that had already saved a credential.
#[cfg(any(test, feature = "test-support"))]
static MOCK_STORE_INIT: std::sync::Once = std::sync::Once::new();

/// Switch the keyring to an in-memory backend that persists across entries.
///
/// Idempotent: the store is installed on the first call and every later call
/// is a no-op, so concurrent callers all observe the same store.
///
/// Uses `keyring_core`'s mock store, which reuses one credential per
/// `(service, user)` pair, so `save()` + `load()` round-trips work in tests
/// (its persistence is `ProcessOnly`).
#[cfg(any(test, feature = "test-support"))]
pub fn use_mock_keyring() {
    MOCK_STORE_INIT.call_once(|| {
        if let Ok(store) = keyring_core::mock::Store::new() {
            keyring_core::set_default_store(store);
        }
    });
}

/// The entry type used to reach the credential store.
///
/// Test builds deliberately use `keyring_core::Entry` rather than
/// `keyring::Entry`; see [`keyring_entry`] for why that distinction matters.
#[cfg(any(test, feature = "test-support"))]
type KeyringEntry = keyring_core::Entry;
#[cfg(not(any(test, feature = "test-support")))]
type KeyringEntry = keyring::Entry;

/// Construct the keyring entry holding the auth token.
///
/// Every keyring access in this module goes through here.
///
/// **Why test builds bypass `keyring::Entry`.** `keyring` 4's `v1` wrapper opens
/// `Entry::new` with `SET_CREDENTIAL_STORE.call_once(set_credential_store)`,
/// which calls `keyring_core::set_default_store(platform_store)` — and that
/// setter overwrites unconditionally. So the *first* `keyring::Entry::new`
/// anywhere in the process replaces whatever store was installed, including a
/// mock. Installing the mock earlier cannot win that race; the wrapper always
/// clobbers it, which is why tests reached the real OS keychain (prompting for
/// access, and failing with "item already exists" against leftover state).
///
/// Going through `keyring_core::Entry` in test builds skips that `call_once`
/// entirely, so the mock store installed by [`use_mock_keyring`] is the one
/// actually used. Production builds keep the `keyring::Entry` wrapper and its
/// platform-store selection, unchanged.
fn keyring_entry(rt: &Runtime) -> keyring::Result<KeyringEntry> {
    #[cfg(any(test, feature = "test-support"))]
    {
        use_mock_keyring();
        keyring_core::Entry::new(rt.keyring_service(), KEYRING_USER)
    }

    #[cfg(not(any(test, feature = "test-support")))]
    {
        keyring::Entry::new(rt.keyring_service(), KEYRING_USER)
    }
}

/// Remove stored credentials (keyring entry and TOML file).
///
/// Silently ignores errors — the credentials may already be absent.
/// Returns `true` if credentials were present before removal.
pub fn remove(rt: &Runtime) -> bool {
    let had_credentials = load(rt).is_some();

    // Remove keyring entry (ignore errors — may already be absent).
    //
    // Deliberately unconditional, not gated on `had_credentials`: the TOML file
    // and the keyring entry can go out of sync (a hand-deleted auth.toml leaves
    // the token orphaned), and `load()` reports None whenever the TOML is gone.
    // Gating here would silently strand that token forever.
    if let Ok(entry) = keyring_entry(rt) {
        let _ = entry.delete_credential();
    }

    // Remove TOML file
    if let Some(path) = auth_config_path(rt) {
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
            tos_accepted_version: None,
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
            tos_accepted_version: None,
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
            tos_accepted_version: None,
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
        let rt = Runtime::isolated();
        let path = auth_config_path(&rt);
        assert!(path.is_some(), "expected auth config path to be Some");
        let path = path.unwrap();
        assert!(
            path.ends_with("auth.toml"),
            "expected path to end with auth.toml, got: {}",
            path.display()
        );
    }

    #[test]
    fn load_returns_none_when_no_file() {
        use_mock_keyring();
        let rt = Runtime::isolated();
        let result = load(&rt);
        assert!(result.is_none(), "expected None when no auth file exists");
    }

    #[test]
    fn save_and_load_roundtrip() {
        use_mock_keyring();
        let rt = Runtime::isolated();

        save(&rt, "secret-token", "alice", "https://api.tokf.net", 3600).unwrap();
        let loaded = load(&rt).expect("credentials should be loadable after save");

        assert_eq!(loaded.token, "secret-token");
        assert_eq!(loaded.username, "alice");
        assert_eq!(loaded.server_url, "https://api.tokf.net");
        assert!(loaded.expires_at > 0);
        assert!(!loaded.is_expired());
    }

    #[test]
    fn remove_clears_credentials() {
        use_mock_keyring();
        let rt = Runtime::isolated();

        save(&rt, "tok_xyz", "bob", "https://example.com", 0).unwrap();
        assert!(load(&rt).is_some(), "credentials should exist after save");

        let removed = remove(&rt);
        assert!(
            removed,
            "remove should return true when credentials existed"
        );

        let after = load(&rt);
        assert!(after.is_none(), "credentials should be gone after remove");
    }

    /// Asserts that a keyring access lands on the mock store.
    ///
    /// Before `keyring_entry()` existed, `load()` here could construct an entry
    /// against the real OS keychain, and because `keyring_core`'s default store
    /// is process-global and first-write-wins, that pinned the real store for
    /// every test that ran afterwards — the cause of the intermittent
    /// "item already exists in the keychain" failures.
    ///
    /// The store is still process-global, but each isolated runtime carries its
    /// own service name, so concurrent tests address disjoint credentials and
    /// no longer need to be serialised.
    #[test]
    fn keyring_entry_installs_the_mock_store_without_an_explicit_call() {
        // No use_mock_keyring() call: keyring_entry() must install it.
        let rt = Runtime::isolated();
        let entry = keyring_entry(&rt).expect("keyring entry should be constructible");

        // Against the mock this round-trips; against a real keychain it would
        // prompt for access or fail outright.
        entry
            .set_password("mock-store-probe")
            .expect("mock store should accept a write");
        assert_eq!(
            entry.get_password().expect("mock store should read back"),
            "mock-store-probe"
        );
        let _ = entry.delete_credential();
    }

    #[test]
    fn use_mock_keyring_is_idempotent_and_preserves_state() {
        use_mock_keyring();
        let rt = Runtime::isolated();
        let entry = keyring_entry(&rt).unwrap();
        entry.set_password("first-write").unwrap();

        // A second call must NOT swap in a fresh, empty store underneath us.
        use_mock_keyring();
        assert_eq!(
            keyring_entry(&rt).unwrap().get_password().unwrap(),
            "first-write",
            "re-installing the mock store must not discard existing credentials"
        );
        let _ = entry.delete_credential();
    }

    #[test]
    fn stored_auth_tos_version_roundtrip() {
        let meta = StoredAuth {
            username: "carol".to_string(),
            server_url: "https://api.tokf.net".to_string(),
            expires_at: 0,
            mit_license_accepted: None,
            tos_accepted_version: Some(1),
        };
        let serialized = toml::to_string_pretty(&meta).unwrap();
        assert!(
            serialized.contains("tos_accepted_version"),
            "should serialize field: {serialized}"
        );
        let deserialized: StoredAuth = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.tos_accepted_version, Some(1));
    }

    #[test]
    fn stored_auth_missing_tos_version_defaults_to_none() {
        let toml_str = r#"
            username = "bob"
            server_url = "https://api.tokf.net"
        "#;
        let meta: StoredAuth = toml::from_str(toml_str).unwrap();
        assert_eq!(meta.tos_accepted_version, None);
    }

    #[test]
    fn save_tos_accepted_version_preserves_other_fields() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("auth.toml");
        let initial = StoredAuth {
            username: "alice".to_string(),
            server_url: "https://example.com".to_string(),
            expires_at: 9_999_999_999,
            mit_license_accepted: Some(true),
            tos_accepted_version: None,
        };
        let content = toml::to_string_pretty(&initial).unwrap();
        std::fs::write(&path, &content).unwrap();

        save_tos_accepted_version_to_path(&path, 1).unwrap();

        let result: StoredAuth = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(result.username, "alice");
        assert_eq!(result.server_url, "https://example.com");
        assert_eq!(result.expires_at, 9_999_999_999);
        assert_eq!(result.mit_license_accepted, Some(true));
        assert_eq!(result.tos_accepted_version, Some(1));
    }

    #[test]
    fn is_expired_unknown() {
        let auth = LoadedAuth {
            token: String::new(),
            username: String::new(),
            server_url: String::new(),
            expires_at: 0,
            mit_license_accepted: None,
            tos_accepted_version: None,
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
            tos_accepted_version: None,
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
            tos_accepted_version: None,
        };
        assert!(auth.is_expired());
    }
}
