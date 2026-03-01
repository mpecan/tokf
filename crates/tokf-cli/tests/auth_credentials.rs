#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serial_test::serial;
use tokf::auth::credentials;

/// Verify that `remove()` is idempotent — calling it twice doesn't error.
#[test]
#[serial]
fn remove_is_idempotent() {
    credentials::use_mock_keyring();
    let dir = tempfile::TempDir::new().unwrap();
    unsafe { std::env::set_var("TOKF_HOME", dir.path().as_os_str()) };

    let _ = credentials::remove();
    let _ = credentials::remove();

    unsafe { std::env::remove_var("TOKF_HOME") };
}

/// Verify the config path is well-formed on all platforms.
#[test]
fn config_path_is_well_formed() {
    let path = credentials::auth_config_path();
    assert!(path.is_some());
    let path = path.unwrap();
    assert!(path.to_string_lossy().contains("tokf"));
    assert!(path.to_string_lossy().ends_with("auth.toml"));
}

/// Verify `StoredAuth` serialization produces valid TOML.
#[test]
fn stored_auth_toml_format() {
    let meta = credentials::StoredAuth {
        username: "testuser".to_string(),
        server_url: "https://api.tokf.net".to_string(),
        expires_at: 1_700_000_000,
        mit_license_accepted: None,
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
    };
    assert!(auth.is_expired());
}

/// Save credentials, load them back, verify all fields match.
#[test]
#[serial]
fn save_load_roundtrip_integration() {
    credentials::use_mock_keyring();
    let dir = tempfile::TempDir::new().unwrap();
    unsafe { std::env::set_var("TOKF_HOME", dir.path().as_os_str()) };

    credentials::save("int-token", "carol", "https://registry.tokf.net", 7200).unwrap();
    let loaded = credentials::load().expect("should load saved credentials");

    unsafe { std::env::remove_var("TOKF_HOME") };

    assert_eq!(loaded.token, "int-token");
    assert_eq!(loaded.username, "carol");
    assert_eq!(loaded.server_url, "https://registry.tokf.net");
    assert!(!loaded.is_expired());
}

/// Save credentials, remove them, verify they are gone.
#[test]
#[serial]
fn remove_after_save_returns_true() {
    credentials::use_mock_keyring();
    let dir = tempfile::TempDir::new().unwrap();
    unsafe { std::env::set_var("TOKF_HOME", dir.path().as_os_str()) };

    credentials::save("tok_rm", "dave", "https://example.com", 0).unwrap();
    let removed = credentials::remove();
    assert!(removed, "remove should return true after save");

    let loaded = credentials::load();
    assert!(loaded.is_none(), "load should return None after remove");

    unsafe { std::env::remove_var("TOKF_HOME") };
}
