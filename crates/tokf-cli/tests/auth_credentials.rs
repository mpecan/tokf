#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use tokf::auth::credentials;
use tokf::runtime::Runtime;

/// Verify that `remove()` is idempotent — calling it twice doesn't error.
#[test]
fn remove_is_idempotent() {
    credentials::use_mock_keyring();
    let rt = Runtime::isolated();

    let _ = credentials::remove(&rt);
    let _ = credentials::remove(&rt);
}

/// Verify the config path is well-formed on all platforms.
#[test]
fn config_path_is_well_formed() {
    let rt = Runtime::isolated();
    let path = credentials::auth_config_path(&rt).expect("isolated runtime resolves a config dir");
    assert_eq!(path, rt.user_dir().unwrap().join("auth.toml"));
}

/// Verify `StoredAuth` serialization produces valid TOML.
#[test]
fn stored_auth_toml_format() {
    let meta = credentials::StoredAuth {
        username: "testuser".to_string(),
        server_url: "https://api.tokf.net".to_string(),
        expires_at: 1_700_000_000,
        mit_license_accepted: None,
        tos_accepted_version: None,
    };
    let toml_str = toml::to_string_pretty(&meta).unwrap();
    assert!(toml_str.contains("username = \"testuser\""));
    assert!(toml_str.contains("server_url = \"https://api.tokf.net\""));
}

/// Verify `LoadedAuth` fields are accessible (compile-time check + runtime).
#[test]
fn loaded_auth_has_named_fields() {
    let auth = credentials::LoadedAuth {
        token: "tok_123".to_string(),
        username: "bob".to_string(),
        server_url: "https://example.com".to_string(),
        expires_at: 0,
        mit_license_accepted: None,
        tos_accepted_version: None,
    };
    assert_eq!(auth.token, "tok_123");
    assert_eq!(auth.username, "bob");
    assert_eq!(auth.server_url, "https://example.com");
    assert!(!auth.is_expired());
}

/// Verify expired token detection works.
#[test]
fn loaded_auth_expired_detection() {
    let auth = credentials::LoadedAuth {
        token: "tok_123".to_string(),
        username: "bob".to_string(),
        server_url: "https://example.com".to_string(),
        expires_at: 1, // epoch + 1 second = definitely expired
        mit_license_accepted: None,
        tos_accepted_version: None,
    };
    assert!(auth.is_expired());
}

/// Save credentials, load them back, verify all fields match.
#[test]
fn save_load_roundtrip_integration() {
    credentials::use_mock_keyring();
    let rt = Runtime::isolated();

    credentials::save(&rt, "int-token", "carol", "https://registry.tokf.net", 7200).unwrap();
    let loaded = credentials::load(&rt).expect("should load saved credentials");

    assert_eq!(loaded.token, "int-token");
    assert_eq!(loaded.username, "carol");
    assert_eq!(loaded.server_url, "https://registry.tokf.net");
    assert!(!loaded.is_expired());
}

/// Save credentials, remove them, verify they are gone.
#[test]
fn remove_after_save_returns_true() {
    credentials::use_mock_keyring();
    let rt = Runtime::isolated();

    credentials::save(&rt, "tok_rm", "dave", "https://example.com", 0).unwrap();
    let removed = credentials::remove(&rt);
    assert!(removed, "remove should return true after save");

    let loaded = credentials::load(&rt);
    assert!(loaded.is_none(), "load should return None after remove");
}
