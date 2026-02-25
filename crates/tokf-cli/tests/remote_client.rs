#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::significant_drop_tightening
)]

use tokf::remote::client;

fn http_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap()
}

// ── register_machine ──────────────────────────────────────────────────────────

#[test]
fn register_machine_success() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/machines")
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "machine_id": "550e8400-e29b-41d4-a716-446655440000",
                "hostname": "test-host",
                "created_at": "2025-01-01T00:00:00Z",
                "last_sync_at": null
            }"#,
        )
        .create();

    let result = client::register_machine(
        &http_client(),
        &server.url(),
        "test-token",
        "550e8400-e29b-41d4-a716-446655440000",
        "test-host",
    );

    assert!(result.is_ok(), "expected Ok, got: {result:?}");
    let machine = result.unwrap();
    assert_eq!(machine.machine_id, "550e8400-e29b-41d4-a716-446655440000");
    assert_eq!(machine.hostname, "test-host");
    mock.assert();
}

#[test]
fn register_machine_server_error_returns_err() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/machines")
        .with_status(500)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error": "internal server error"}"#)
        .create();

    let result = client::register_machine(
        &http_client(),
        &server.url(),
        "test-token",
        "550e8400-e29b-41d4-a716-446655440000",
        "test-host",
    );

    assert!(result.is_err(), "expected Err for 500");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("500"),
        "error should mention status code: {err}"
    );
    mock.assert();
}

#[test]
fn register_machine_401_returns_auth_hint() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/machines")
        .with_status(401)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error": "unauthorized"}"#)
        .create();

    let result = client::register_machine(
        &http_client(),
        &server.url(),
        "bad-token",
        "550e8400-e29b-41d4-a716-446655440000",
        "test-host",
    );

    assert!(result.is_err(), "expected Err for 401");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("401") && err.contains("tokf auth login"),
        "error should mention 401 and re-auth hint: {err}"
    );
    mock.assert();
}

#[test]
fn register_machine_sends_bearer_token() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/machines")
        .match_header("authorization", "Bearer my-secret-token")
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "machine_id": "550e8400-e29b-41d4-a716-446655440000",
                "hostname": "test-host",
                "created_at": "2025-01-01T00:00:00Z"
            }"#,
        )
        .create();

    let _ = client::register_machine(
        &http_client(),
        &server.url(),
        "my-secret-token",
        "550e8400-e29b-41d4-a716-446655440000",
        "test-host",
    );
    mock.assert();
}

// ── list_machines ─────────────────────────────────────────────────────────────

#[test]
fn list_machines_empty_list() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/api/machines")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[]")
        .create();

    let result = client::list_machines(&http_client(), &server.url(), "test-token");
    assert!(result.is_ok(), "expected Ok, got: {result:?}");
    assert_eq!(result.unwrap().len(), 0);
    mock.assert();
}

#[test]
fn list_machines_with_entries() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/api/machines")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
                {
                    "machine_id": "550e8400-e29b-41d4-a716-446655440000",
                    "hostname": "laptop-1",
                    "created_at": "2025-01-01T00:00:00Z",
                    "last_sync_at": "2025-02-01T12:00:00Z"
                },
                {
                    "machine_id": "660e8400-e29b-41d4-a716-446655440000",
                    "hostname": "desktop-2",
                    "created_at": "2025-01-15T00:00:00Z",
                    "last_sync_at": null
                }
            ]"#,
        )
        .create();

    let result = client::list_machines(&http_client(), &server.url(), "test-token");
    assert!(result.is_ok(), "expected Ok, got: {result:?}");
    let machines = result.unwrap();
    assert_eq!(machines.len(), 2);
    assert_eq!(machines[0].hostname, "laptop-1");
    assert_eq!(
        machines[0].last_sync_at.as_deref(),
        Some("2025-02-01T12:00:00Z")
    );
    assert_eq!(machines[1].hostname, "desktop-2");
    assert!(machines[1].last_sync_at.is_none());
    mock.assert();
}

#[test]
fn list_machines_server_error_returns_err() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/api/machines")
        .with_status(500)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error": "internal server error"}"#)
        .create();

    let result = client::list_machines(&http_client(), &server.url(), "test-token");
    assert!(result.is_err(), "expected Err for 500");
    mock.assert();
}

#[test]
fn list_machines_401_returns_auth_hint() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/api/machines")
        .with_status(401)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error": "unauthorized"}"#)
        .create();

    let result = client::list_machines(&http_client(), &server.url(), "bad-token");
    assert!(result.is_err(), "expected Err for 401");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("401") && err.contains("tokf auth login"),
        "error should mention 401 and re-auth hint: {err}"
    );
    mock.assert();
}
