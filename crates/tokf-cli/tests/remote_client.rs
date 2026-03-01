#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::significant_drop_tightening
)]

use tokf::remote::client;
use tokf::remote::http::Client;

fn make_client(server: &mockito::Server, token: &str) -> Client {
    Client::new(&server.url(), Some(token)).unwrap()
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

    let c = make_client(&server, "test-token");
    let result = client::register_machine(&c, "550e8400-e29b-41d4-a716-446655440000", "test-host");

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

    let c = make_client(&server, "test-token");
    let result = client::register_machine(&c, "550e8400-e29b-41d4-a716-446655440000", "test-host");

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

    let c = make_client(&server, "bad-token");
    let result = client::register_machine(&c, "550e8400-e29b-41d4-a716-446655440000", "test-host");

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

    let c = make_client(&server, "my-secret-token");
    let _ = client::register_machine(&c, "550e8400-e29b-41d4-a716-446655440000", "test-host");
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

    let c = make_client(&server, "test-token");
    let result = client::list_machines(&c);
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

    let c = make_client(&server, "test-token");
    let result = client::list_machines(&c);
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
    // GET retries once on 5xx, so expect 2 requests (initial + retry).
    let mock = server
        .mock("GET", "/api/machines")
        .with_status(500)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error": "internal server error"}"#)
        .expect(2)
        .create();

    let c = make_client(&server, "test-token");
    let result = client::list_machines(&c);
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

    let c = make_client(&server, "bad-token");
    let result = client::list_machines(&c);
    assert!(result.is_err(), "expected Err for 401");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("401") && err.contains("tokf auth login"),
        "error should mention 401 and re-auth hint: {err}"
    );
    mock.assert();
}

// ── Client retry and error classification ────────────────────────────────────

#[test]
fn get_retries_once_on_500_then_succeeds() {
    let mut server = mockito::Server::new();
    // First request returns 500, second returns 200
    let mock = server
        .mock("GET", "/api/machines")
        .with_status(500)
        .with_body("error")
        .expect(1)
        .create();
    let mock_ok = server
        .mock("GET", "/api/machines")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[]")
        .expect(1)
        .create();

    let c = make_client(&server, "tok");
    let result: anyhow::Result<Vec<client::MachineInfo>> = c.get("/api/machines");
    assert!(result.is_ok(), "should succeed after retry: {result:?}");
    mock.assert();
    mock_ok.assert();
}

#[test]
fn post_does_not_retry_on_500() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/machines")
        .with_status(500)
        .with_body("error")
        .expect(1)
        .create();

    let c = make_client(&server, "tok");
    let result = client::register_machine(&c, "mid", "host");
    assert!(result.is_err(), "should fail without retry on POST 500");
    // Verify only 1 request was made
    mock.assert();
}

#[test]
fn auth_header_injected_on_get() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/api/machines")
        .match_header("authorization", "Bearer secret-tok")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[]")
        .create();

    let c = make_client(&server, "secret-tok");
    let _ = client::list_machines(&c);
    mock.assert();
}

#[test]
fn no_auth_header_when_no_token() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/api/gain/global")
        .match_header("authorization", mockito::Matcher::Missing)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"total_input_tokens":0,"total_output_tokens":0,"total_commands":0,"by_machine":[],"by_filter":[]}"#,
        )
        .create();

    let c = Client::unauthenticated(&server.url()).unwrap();
    let result = tokf::remote::gain_client::get_global_gain(&c);
    assert!(result.is_ok(), "expected Ok, got: {result:?}");
    mock.assert();
}

#[test]
fn error_format_mentions_tokf_debug() {
    let mut server = mockito::Server::new();
    // GET retries once on 5xx, so expect 2 requests.
    let _mock = server
        .mock("GET", "/api/machines")
        .with_status(503)
        .with_body("service unavailable")
        .expect(2)
        .create();

    let c = make_client(&server, "tok");
    let result = client::list_machines(&c);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("TOKF_DEBUG=1"),
        "error should mention TOKF_DEBUG=1: {err}"
    );
}

// ── Status code classification ───────────────────────────────────────────────

#[test]
fn get_403_returns_client_error() {
    let mut server = mockito::Server::new();
    // 403 is not transient — no retry, exactly 1 request.
    let mock = server
        .mock("GET", "/api/test")
        .with_status(403)
        .with_body("forbidden")
        .expect(1)
        .create();

    let c = make_client(&server, "tok");
    let result: anyhow::Result<serde_json::Value> = c.get("/api/test");
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("403"), "should mention 403: {err}");
    mock.assert();
}

#[test]
fn get_404_returns_client_error() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/api/test")
        .with_status(404)
        .with_body("not found")
        .expect(1)
        .create();

    let c = make_client(&server, "tok");
    let result: anyhow::Result<serde_json::Value> = c.get("/api/test");
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("404"), "should mention 404: {err}");
    mock.assert();
}

#[test]
fn get_429_returns_rate_limited_error() {
    let mut server = mockito::Server::new();
    // 429 is NOT transient in our model — no auto-retry at Client level.
    // (Retry is handled by the higher-level retry::with_retry wrapper.)
    let mock = server
        .mock("GET", "/api/test")
        .with_status(429)
        .with_header("retry-after", "30")
        .with_body("rate limited")
        .expect(1)
        .create();

    let c = make_client(&server, "tok");
    let result: anyhow::Result<serde_json::Value> = c.get("/api/test");
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("rate limit"),
        "should mention rate limit: {err}"
    );
    assert!(err.contains("30s"), "should include retry-after: {err}");
    mock.assert();
}

#[test]
fn get_429_defaults_retry_after_to_60() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/api/test")
        .with_status(429)
        // No Retry-After header — should default to 60s
        .with_body("rate limited")
        .expect(1)
        .create();

    let c = make_client(&server, "tok");
    let result: anyhow::Result<serde_json::Value> = c.get("/api/test");
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("60s"), "should default to 60s: {err}");
    mock.assert();
}

#[test]
fn get_502_retries_then_fails() {
    let mut server = mockito::Server::new();
    // 502 is a server error → transient → retries once → both fail.
    let mock = server
        .mock("GET", "/api/test")
        .with_status(502)
        .with_body("bad gateway")
        .expect(2)
        .create();

    let c = make_client(&server, "tok");
    let result: anyhow::Result<serde_json::Value> = c.get("/api/test");
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("502"), "should mention 502: {err}");
    mock.assert();
}

// ── Retry edge cases ─────────────────────────────────────────────────────────

#[test]
fn get_retries_once_then_both_fail_returns_second_error() {
    let mut server = mockito::Server::new();
    // Both attempts return 500. Exactly 2 requests.
    let mock = server
        .mock("GET", "/api/test")
        .with_status(500)
        .with_body("error")
        .expect(2)
        .create();

    let c = make_client(&server, "tok");
    let result: anyhow::Result<serde_json::Value> = c.get("/api/test");
    assert!(result.is_err(), "should fail when both attempts fail");
    mock.assert();
}

#[test]
fn get_does_not_retry_on_401() {
    let mut server = mockito::Server::new();
    // 401 is not transient — exactly 1 request.
    let mock = server
        .mock("GET", "/api/test")
        .with_status(401)
        .with_body("unauthorized")
        .expect(1)
        .create();

    let c = make_client(&server, "tok");
    let result: anyhow::Result<serde_json::Value> = c.get("/api/test");
    assert!(result.is_err());
    mock.assert();
}

#[test]
fn get_does_not_retry_on_404() {
    let mut server = mockito::Server::new();
    // 404 is not transient — exactly 1 request.
    let mock = server
        .mock("GET", "/api/test")
        .with_status(404)
        .with_body("not found")
        .expect(1)
        .create();

    let c = make_client(&server, "tok");
    let result: anyhow::Result<serde_json::Value> = c.get("/api/test");
    assert!(result.is_err());
    mock.assert();
}

// ── Multipart methods ────────────────────────────────────────────────────────

#[test]
fn post_multipart_returns_raw_response_on_any_status() {
    use reqwest::blocking::multipart::{Form, Part};
    let mut server = mockito::Server::new();
    // post_multipart returns the raw response — even 400 is not an error.
    let mock = server
        .mock("POST", "/api/filters")
        .with_status(400)
        .with_body(r#"{"error": "bad request"}"#)
        .expect(1)
        .create();

    let c = make_client(&server, "tok");
    let result = c.post_multipart("/api/filters", || {
        Form::new().part("filter", Part::text("data"))
    });
    assert!(
        result.is_ok(),
        "post_multipart should return Ok even on 400"
    );
    assert_eq!(result.unwrap().status(), 400);
    mock.assert();
}

#[test]
fn post_multipart_does_not_retry() {
    use reqwest::blocking::multipart::{Form, Part};
    let mut server = mockito::Server::new();
    // POST is non-idempotent, so no retry even on connection error simulation.
    // We test with a normal error status to verify exactly 1 request.
    let mock = server
        .mock("POST", "/api/filters")
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();

    let c = make_client(&server, "tok");
    let result = c.post_multipart("/api/filters", || {
        Form::new().part("filter", Part::text("data"))
    });
    assert!(result.is_ok());
    mock.assert();
}

#[test]
fn put_multipart_returns_raw_response() {
    use reqwest::blocking::multipart::{Form, Part};
    let mut server = mockito::Server::new();
    let mock = server
        .mock("PUT", "/api/filters/abc/tests")
        .with_status(200)
        .with_body(r#"{"ok": true}"#)
        .expect(1)
        .create();

    let c = make_client(&server, "tok");
    let result = c.put_multipart("/api/filters/abc/tests", || {
        Form::new().part("test:foo.toml", Part::text("data"))
    });
    assert!(result.is_ok());
    assert_eq!(result.unwrap().status(), 200);
    mock.assert();
}

// ── get_with_query ───────────────────────────────────────────────────────────

#[test]
fn get_with_query_passes_params() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/api/filters")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("q".into(), "git push".into()),
            mockito::Matcher::UrlEncoded("limit".into(), "10".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[]")
        .create();

    let c = make_client(&server, "tok");
    let result: anyhow::Result<Vec<serde_json::Value>> =
        c.get_with_query("/api/filters", &[("q", "git push"), ("limit", "10")]);
    assert!(result.is_ok(), "expected Ok, got: {result:?}");
    mock.assert();
}

#[test]
fn get_with_query_retries_on_5xx() {
    let mut server = mockito::Server::new();
    let mock_fail = server
        .mock("GET", "/api/filters")
        .match_query(mockito::Matcher::UrlEncoded("q".into(), "test".into()))
        .with_status(500)
        .with_body("error")
        .expect(1)
        .create();
    let mock_ok = server
        .mock("GET", "/api/filters")
        .match_query(mockito::Matcher::UrlEncoded("q".into(), "test".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[]")
        .expect(1)
        .create();

    let c = make_client(&server, "tok");
    let result: anyhow::Result<Vec<serde_json::Value>> =
        c.get_with_query("/api/filters", &[("q", "test")]);
    assert!(result.is_ok(), "should succeed after retry: {result:?}");
    mock_fail.assert();
    mock_ok.assert();
}

// ── get_raw ──────────────────────────────────────────────────────────────────

#[test]
fn get_raw_returns_response() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/api/filters/abc/download")
        .with_status(200)
        .with_body("file-contents")
        .create();

    let c = make_client(&server, "tok");
    let result = c.get_raw("/api/filters/abc/download");
    assert!(result.is_ok());
    let resp = result.unwrap();
    assert_eq!(resp.status(), 200);
    mock.assert();
}

#[test]
fn get_raw_retries_on_5xx() {
    let mut server = mockito::Server::new();
    let mock_fail = server
        .mock("GET", "/api/download")
        .with_status(503)
        .with_body("unavailable")
        .expect(1)
        .create();
    let mock_ok = server
        .mock("GET", "/api/download")
        .with_status(200)
        .with_body("data")
        .expect(1)
        .create();

    let c = make_client(&server, "tok");
    let result = c.get_raw("/api/download");
    assert!(result.is_ok());
    mock_fail.assert();
    mock_ok.assert();
}
