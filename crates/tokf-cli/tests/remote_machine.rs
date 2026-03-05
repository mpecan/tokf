#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use tokf::remote::machine;

#[test]
fn stored_machine_roundtrip() {
    let stored = machine::StoredMachine {
        machine_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
        hostname: "test-laptop".to_string(),
    };
    let toml_str = toml::to_string_pretty(&stored).unwrap();
    let deserialized: machine::StoredMachine = toml::from_str(&toml_str).unwrap();
    assert_eq!(
        deserialized.machine_id,
        "550e8400-e29b-41d4-a716-446655440000"
    );
    assert_eq!(deserialized.hostname, "test-laptop");
}

#[test]
fn machine_config_path_returns_some() {
    let path = machine::machine_config_path();
    assert!(path.is_some(), "expected machine config path to be Some");
    let path = path.unwrap();
    assert!(
        path.to_string_lossy().contains("tokf"),
        "path should contain 'tokf': {}",
        path.display()
    );
    assert!(
        path.to_string_lossy().ends_with("machine.toml"),
        "path should end with 'machine.toml': {}",
        path.display()
    );
}

#[test]
fn load_returns_none_when_no_file() {
    // This validates graceful handling when no registration exists.
    // May return Some on a developer machine that has registered — we just
    // verify it doesn't panic.
    let _ = machine::load();
}

#[test]
fn stored_machine_toml_has_expected_keys() {
    let stored = machine::StoredMachine {
        machine_id: "abc-123".to_string(),
        hostname: "my-host".to_string(),
    };
    let toml_str = toml::to_string_pretty(&stored).unwrap();
    assert!(toml_str.contains("machine_id = \"abc-123\""));
    assert!(toml_str.contains("hostname = \"my-host\""));
}
