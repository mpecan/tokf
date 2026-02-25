#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::significant_drop_tightening
)]

use tokf::auth::client;

fn http_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap()
}

// ── initiate_device_flow ─────────────────────────────────────────────────

#[test]
fn initiate_device_flow_success() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/auth/device")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "device_code": "dc-123",
                "user_code": "ABCD-1234",
                "verification_uri": "https://github.com/login/device",
                "expires_in": 900,
                "interval": 5
            }"#,
        )
        .create();

    let resp = client::initiate_device_flow(&http_client(), &server.url()).unwrap();
    assert_eq!(resp.device_code, "dc-123");
    assert_eq!(resp.user_code, "ABCD-1234");
    assert_eq!(resp.expires_in, 900);
    assert!(resp.verification_uri_complete.is_none());
    mock.assert();
}

#[test]
fn initiate_device_flow_with_complete_uri() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/auth/device")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "device_code": "dc-123",
                "user_code": "ABCD-1234",
                "verification_uri": "https://github.com/login/device",
                "verification_uri_complete": "https://github.com/login/device?user_code=ABCD-1234",
                "expires_in": 900,
                "interval": 5
            }"#,
        )
        .create();

    let resp = client::initiate_device_flow(&http_client(), &server.url()).unwrap();
    assert_eq!(
        resp.verification_uri_complete.as_deref(),
        Some("https://github.com/login/device?user_code=ABCD-1234")
    );
    mock.assert();
}

#[test]
fn initiate_device_flow_server_error() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/auth/device")
        .with_status(500)
        .with_body("Internal Server Error")
        .create();

    let err = client::initiate_device_flow(&http_client(), &server.url()).unwrap_err();
    assert!(
        err.to_string().contains("HTTP 500"),
        "expected HTTP 500 in error, got: {err}"
    );
    mock.assert();
}

#[test]
fn initiate_device_flow_malformed_json() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/auth/device")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"invalid": true}"#)
        .create();

    let err = client::initiate_device_flow(&http_client(), &server.url()).unwrap_err();
    assert!(
        err.to_string().contains("invalid response"),
        "expected 'invalid response' in error, got: {err}"
    );
    mock.assert();
}

#[test]
fn initiate_device_flow_unreachable() {
    let err = client::initiate_device_flow(&http_client(), "http://localhost:1").unwrap_err();
    assert!(
        err.to_string().contains("could not reach"),
        "expected 'could not reach' in error, got: {err}"
    );
}

// ── poll_token ───────────────────────────────────────────────────────────

#[test]
fn poll_token_success() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/auth/token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "access_token": "tok_secret",
                "token_type": "bearer",
                "expires_in": 7776000,
                "user": {
                    "id": 42,
                    "username": "octocat",
                    "avatar_url": "https://avatars.githubusercontent.com/u/42"
                }
            }"#,
        )
        .create();

    let result = client::poll_token(&http_client(), &server.url(), "dc-123").unwrap();
    match result {
        client::PollResult::Success(resp) => {
            assert_eq!(resp.access_token, "tok_secret");
            assert_eq!(resp.user.username, "octocat");
        }
        other => panic!("expected Success, got: {other:?}"),
    }
    mock.assert();
}

#[test]
fn poll_token_pending() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/auth/token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error": "authorization_pending"}"#)
        .create();

    let result = client::poll_token(&http_client(), &server.url(), "dc-123").unwrap();
    match result {
        client::PollResult::Pending { interval } => {
            assert_eq!(interval, 5); // default when not provided
        }
        other => panic!("expected Pending, got: {other:?}"),
    }
    mock.assert();
}

#[test]
fn poll_token_slow_down() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/auth/token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error": "slow_down", "interval": 15}"#)
        .create();

    let result = client::poll_token(&http_client(), &server.url(), "dc-123").unwrap();
    match result {
        client::PollResult::SlowDown { interval } => {
            assert_eq!(interval, 15);
        }
        other => panic!("expected SlowDown, got: {other:?}"),
    }
    mock.assert();
}

#[test]
fn poll_token_client_error_denied() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/auth/token")
        .with_status(403)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":"access_denied","error_description":"The user denied the request"}"#)
        .create();

    let result = client::poll_token(&http_client(), &server.url(), "dc-123").unwrap();
    match result {
        client::PollResult::Failed(msg) => {
            assert_eq!(msg, "The user denied the request");
        }
        other => panic!("expected Failed, got: {other:?}"),
    }
    mock.assert();
}

#[test]
fn poll_token_client_error_expired() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/auth/token")
        .with_status(400)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":"expired_token"}"#)
        .create();

    let result = client::poll_token(&http_client(), &server.url(), "dc-123").unwrap();
    match result {
        client::PollResult::Failed(msg) => {
            assert_eq!(msg, "expired_token");
        }
        other => panic!("expected Failed, got: {other:?}"),
    }
    mock.assert();
}

#[test]
fn poll_token_server_error_5xx() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/auth/token")
        .with_status(502)
        .with_body("Bad Gateway")
        .create();

    let err = client::poll_token(&http_client(), &server.url(), "dc-123").unwrap_err();
    assert!(
        err.to_string().contains("HTTP 502"),
        "expected HTTP 502 in error, got: {err}"
    );
    mock.assert();
}

#[test]
fn poll_token_unparseable_200() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/auth/token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("not json at all")
        .create();

    let err = client::poll_token(&http_client(), &server.url(), "dc-123").unwrap_err();
    assert!(
        err.to_string().contains("unexpected response"),
        "expected 'unexpected response' in error, got: {err}"
    );
    mock.assert();
}

#[test]
fn poll_token_unknown_error_string() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/api/auth/token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error": "some_unknown_error"}"#)
        .create();

    let result = client::poll_token(&http_client(), &server.url(), "dc-123").unwrap();
    match result {
        client::PollResult::Failed(msg) => {
            assert_eq!(msg, "some_unknown_error");
        }
        other => panic!("expected Failed, got: {other:?}"),
    }
    mock.assert();
}

#[test]
fn poll_token_4xx_raw_text_sanitized() {
    let mut server = mockito::Server::new();
    let body = format!(
        "error with control chars \x1b[31m and long text {}",
        "x".repeat(300)
    );
    let mock = server
        .mock("POST", "/api/auth/token")
        .with_status(400)
        .with_body(&body)
        .create();

    let result = client::poll_token(&http_client(), &server.url(), "dc-123").unwrap();
    match result {
        client::PollResult::Failed(msg) => {
            // Should be truncated and control chars stripped
            assert!(msg.len() <= 260, "message too long: {}", msg.len());
            assert!(!msg.contains('\x1b'), "control chars not stripped");
        }
        other => panic!("expected Failed, got: {other:?}"),
    }
    mock.assert();
}
