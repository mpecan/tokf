use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

use crate::auth::mock::NoOpGitHubClient;
use crate::rate_limit::{PublishRateLimiter, SyncRateLimiter};
use crate::state::AppState;
use crate::storage::mock::InMemoryStorageClient;

use super::super::test_helpers::{
    DEFAULT_PASSING_TEST, MIT_ACCEPT, insert_test_user, make_multipart, make_state,
    make_state_with_storage, post_filter,
};

const VALID_FILTER_TOML: &[u8] = b"command = \"my-tool\"\n";

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_filter_creates_record(pool: PgPool) {
    let (_, token) = insert_test_user(&pool, "alice_pub").await;
    let app = crate::routes::create_router(make_state(pool.clone()));

    let resp = post_filter(
        app,
        &token,
        &[
            ("filter", VALID_FILTER_TOML),
            MIT_ACCEPT,
            DEFAULT_PASSING_TEST,
        ],
    )
    .await;

    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["content_hash"].is_string());
    assert_eq!(json["command_pattern"], "my-tool");
    assert_eq!(json["author"], "alice_pub");
    assert!(
        json["registry_url"]
            .as_str()
            .unwrap()
            .starts_with("https://registry.tokf.net/filters/"),
        "registry_url should be a real URL, got: {}",
        json["registry_url"]
    );
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_filter_with_tests_creates_filter_test_rows(pool: PgPool) {
    let (_, token) = insert_test_user(&pool, "alice_tests").await;
    let storage = Arc::new(InMemoryStorageClient::new());
    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));

    let resp = post_filter(
        app,
        &token,
        &[
            ("filter", VALID_FILTER_TOML),
            MIT_ACCEPT,
            (
                "test:basic.toml",
                b"name = \"basic\"\ninline = \"ok output\"\n\n[[expect]]\ncontains = \"ok\"\n",
            ),
            (
                "test:advanced.toml",
                b"name = \"advanced\"\ninline = \"ok result\"\n\n[[expect]]\ncontains = \"ok\"\n",
            ),
        ],
    )
    .await;

    assert_eq!(resp.status(), StatusCode::CREATED);
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let hash = json["content_hash"].as_str().unwrap();

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM filter_tests WHERE filter_hash = $1")
        .bind(hash)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2, "expected 2 filter_tests rows");

    // Verify R2 received filter + 2 test files = 3 puts
    assert_eq!(storage.put_count(), 3, "expected 3 R2 put calls");
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_filter_duplicate_returns_200(pool: PgPool) {
    let (_, token) = insert_test_user(&pool, "alice_dup").await;

    let fields = &[
        ("filter", VALID_FILTER_TOML),
        MIT_ACCEPT,
        DEFAULT_PASSING_TEST,
    ];

    let app = crate::routes::create_router(make_state(pool.clone()));
    let resp = post_filter(app, &token, fields).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let app = crate::routes::create_router(make_state(pool.clone()));
    let resp = post_filter(app, &token, fields).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_filter_rejects_invalid_toml(pool: PgPool) {
    let (_, token) = insert_test_user(&pool, "alice_bad_toml").await;
    let app = crate::routes::create_router(make_state(pool));

    let resp = post_filter(
        app,
        &token,
        &[("filter", b"not valid toml [[["), MIT_ACCEPT],
    )
    .await;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_filter_rejects_oversized_filter(pool: PgPool) {
    let (_, token) = insert_test_user(&pool, "alice_big").await;
    let app = crate::routes::create_router(make_state(pool));

    let oversized = vec![b'x'; super::MAX_FILTER_SIZE + 1];
    let resp = post_filter(app, &token, &[("filter", &oversized), MIT_ACCEPT]).await;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json["error"].as_str().unwrap().contains("64 KB"),
        "expected size limit message, got: {}",
        json["error"]
    );
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_filter_rejects_oversized_total(pool: PgPool) {
    let (_, token) = insert_test_user(&pool, "alice_total_big").await;
    let app = crate::routes::create_router(make_state(pool));

    // filter is small, but test file pushes total over 1 MB
    let big_test = vec![b'x'; super::MAX_TOTAL_SIZE];
    let resp = post_filter(
        app,
        &token,
        &[
            ("filter", VALID_FILTER_TOML),
            MIT_ACCEPT,
            ("test:big.toml", &big_test),
        ],
    )
    .await;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json["error"].as_str().unwrap().contains("1 MB"),
        "expected total size limit message, got: {}",
        json["error"]
    );
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_filter_requires_auth(pool: PgPool) {
    let app = crate::routes::create_router(make_state(pool));

    let (body, content_type) = make_multipart(&[("filter", VALID_FILTER_TOML), MIT_ACCEPT]);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/filters")
                .header("content-type", content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_filter_rejects_missing_filter_field(pool: PgPool) {
    let (_, token) = insert_test_user(&pool, "alice_no_filter").await;
    let app = crate::routes::create_router(make_state(pool));

    // Send ONLY the license field, no filter
    let resp = post_filter(app, &token, &[MIT_ACCEPT]).await;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("missing required 'filter'"),
        "expected missing filter message, got: {}",
        json["error"]
    );
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_filter_rejects_missing_mit_acceptance(pool: PgPool) {
    let (_, token) = insert_test_user(&pool, "alice_no_license").await;
    let app = crate::routes::create_router(make_state(pool));

    // Send filter but NOT the mit_license_accepted field
    let resp = post_filter(app, &token, &[("filter", VALID_FILTER_TOML)]).await;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json["error"].as_str().unwrap().contains("MIT license"),
        "expected MIT license message, got: {}",
        json["error"]
    );
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_filter_stores_canonical_command(pool: PgPool) {
    let (_, token) = insert_test_user(&pool, "alice_canonical").await;
    let app = crate::routes::create_router(make_state(pool.clone()));

    // Filter whose command is "git push" — canonical_command should be "git"
    let toml = b"command = \"git push\"\n";
    let resp = post_filter(
        app,
        &token,
        &[("filter", toml), MIT_ACCEPT, DEFAULT_PASSING_TEST],
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let hash = json["content_hash"].as_str().unwrap();

    let canonical: String =
        sqlx::query_scalar("SELECT canonical_command FROM filters WHERE content_hash = $1")
            .bind(hash)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        canonical, "git",
        "canonical_command should be the first word"
    );

    let pattern: String =
        sqlx::query_scalar("SELECT command_pattern FROM filters WHERE content_hash = $1")
            .bind(hash)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        pattern, "git push",
        "command_pattern should be the full pattern"
    );
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_filter_rate_limits_user(pool: PgPool) {
    let (_, token) = insert_test_user(&pool, "alice_rl").await;

    // Create state with very low rate limit (1 per hour)
    let state = AppState {
        db: pool.clone(),
        github: Arc::new(NoOpGitHubClient),
        storage: Arc::new(InMemoryStorageClient::new()),
        github_client_id: "test-client-id".to_string(),
        github_client_secret: "test-client-secret".to_string(),
        trust_proxy: false,
        public_url: "https://registry.tokf.net".to_string(),
        publish_rate_limiter: Arc::new(PublishRateLimiter::new(1, 3600)),
        search_rate_limiter: Arc::new(PublishRateLimiter::new(1000, 3600)),
        sync_rate_limiter: Arc::new(SyncRateLimiter::new(100, 3600)),
    };

    let app = crate::routes::create_router(state.clone());
    let resp = post_filter(
        app,
        &token,
        &[
            ("filter", VALID_FILTER_TOML),
            MIT_ACCEPT,
            DEFAULT_PASSING_TEST,
        ],
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "first publish should succeed"
    );

    // Upload a different filter (different content) to avoid the 200-OK deduplicate path
    let app = crate::routes::create_router(state);
    let different_toml = b"command = \"other-tool\"\n";
    let resp = post_filter(
        app,
        &token,
        &[("filter", different_toml), MIT_ACCEPT, DEFAULT_PASSING_TEST],
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "second publish should be rate limited"
    );
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_filter_rejects_missing_tests(pool: PgPool) {
    let (_, token) = insert_test_user(&pool, "alice_no_tests").await;
    let app = crate::routes::create_router(make_state(pool));

    let resp = post_filter(app, &token, &[("filter", VALID_FILTER_TOML), MIT_ACCEPT]).await;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("at least one test file"),
        "expected missing test error, got: {}",
        json["error"]
    );
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_filter_rejects_failing_tests(pool: PgPool) {
    let (_, token) = insert_test_user(&pool, "alice_fail_tests").await;
    let app = crate::routes::create_router(make_state(pool));

    // Test expects "not present" but filter passes through empty inline data
    let failing_test =
        b"name = \"bad\"\ninline = \"hello\"\n\n[[expect]]\ncontains = \"not present\"\n";
    let resp = post_filter(
        app,
        &token,
        &[
            ("filter", VALID_FILTER_TOML),
            MIT_ACCEPT,
            ("test:bad.toml", failing_test),
        ],
    )
    .await;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json["error"].as_str().unwrap().contains("tests failed"),
        "expected test failure message, got: {}",
        json["error"]
    );
}
