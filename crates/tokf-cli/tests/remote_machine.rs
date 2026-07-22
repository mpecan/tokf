#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use tokf::remote::machine;

use tokf::runtime::Runtime;

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

/// The machine config lives directly under the runtime's user directory.
///
/// This used to assert the path *contained* `tokf`, which only held for the
/// platform-default layout — isolating the test broke it. Assert the actual
/// relationship instead.
#[test]
fn machine_config_path_sits_under_the_user_dir() {
    let rt = Runtime::isolated();
    let path = machine::machine_config_path(&rt).expect("isolated runtime resolves a config dir");
    assert_eq!(path, rt.user_dir().unwrap().join("machine.toml"));
}

#[test]
fn load_returns_none_when_no_file() {
    // Isolation makes this assertable: the runtime's home is a fresh temp dir,
    // so no machine.toml can exist. Before, this could only check "doesn't
    // panic", because a developer machine that had registered returned Some.
    let rt = Runtime::isolated();
    assert!(machine::load(&rt).is_none());
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
