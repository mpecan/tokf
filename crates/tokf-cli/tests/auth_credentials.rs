#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use tokf::auth::credentials;

/// Verify that `remove()` is idempotent — calling it twice doesn't error.
#[test]
fn remove_is_idempotent() {
    // This may or may not have real credentials on the developer's machine.
    // The function should never panic regardless.
    let _ = credentials::remove();
    let _ = credentials::remove();
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
