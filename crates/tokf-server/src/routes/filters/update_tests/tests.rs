use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

use crate::storage::StorageClient;
use crate::storage::mock::InMemoryStorageClient;

use super::super::test_helpers::{
    insert_test_user, make_multipart, make_state_with_storage, publish_filter_helper, put_tests,
};

const VALID_FILTER_TOML: &[u8] = b"command = \"my-tool\"\n";

fn valid_test(name: &str) -> Vec<u8> {
    format!("name = \"{name}\"\ninline = \"hello world\"\n\n[[expect]]\ncontains = \"hello\"\n")
        .into_bytes()
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn author_can_update_tests(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let (_, token) = insert_test_user(&pool, "author_update").await;

    // Publish with 1 test file
    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let hash = publish_filter_helper(
        app,
        &token,
        VALID_FILTER_TOML,
        &[("test:old.toml", &valid_test("old"))],
    )
    .await;

    // Verify 1 test in DB
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM filter_tests WHERE filter_hash = $1")
        .bind(&hash)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "expected 1 test before update");

    // Update with 2 new test files
    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let resp = put_tests(
        app,
        &token,
        &hash,
        &[
            ("test:new1.toml", &valid_test("new1")),
            ("test:new2.toml", &valid_test("new2")),
        ],
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["test_count"], 2);

    // Verify DB now has 2 rows
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM filter_tests WHERE filter_hash = $1")
        .bind(&hash)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2, "expected 2 tests after update");

    // Verify old test file was deleted from storage
    assert!(
        !storage
            .exists(&format!("filters/{hash}/tests/old.toml"))
            .await
            .unwrap(),
        "old test file should be deleted from storage"
    );
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn non_author_gets_403(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let (_, alice_token) = insert_test_user(&pool, "alice_forbidden").await;
    let (_, bob_token) = insert_test_user(&pool, "bob_forbidden").await;

    // Alice publishes
    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let hash = publish_filter_helper(app, &alice_token, VALID_FILTER_TOML, &[]).await;

    // Bob tries to update tests
    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let resp = put_tests(
        app,
        &bob_token,
        &hash,
        &[("test:new.toml", &valid_test("new"))],
    )
    .await;

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn unknown_hash_gets_404(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let (_, token) = insert_test_user(&pool, "user_404").await;

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let fake_hash = "0".repeat(64);
    let resp = put_tests(
        app,
        &token,
        &fake_hash,
        &[("test:new.toml", &valid_test("new"))],
    )
    .await;

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn empty_test_upload_gets_400(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let (_, token) = insert_test_user(&pool, "user_empty").await;

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let hash = publish_filter_helper(app, &token, VALID_FILTER_TOML, &[]).await;

    // PUT with no test files
    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let resp = put_tests(app, &token, &hash, &[]).await;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("at least one test file"),
        "expected empty test error, got: {}",
        json["error"]
    );
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn invalid_test_toml_gets_400(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let (_, token) = insert_test_user(&pool, "user_invalid_toml").await;

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let hash = publish_filter_helper(app, &token, VALID_FILTER_TOML, &[]).await;

    // PUT with malformed TOML
    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let resp = put_tests(
        app,
        &token,
        &hash,
        &[("test:bad.toml", b"not valid toml [[[")],
    )
    .await;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn auth_required(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let (_, token) = insert_test_user(&pool, "user_auth_req").await;

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let hash = publish_filter_helper(app, &token, VALID_FILTER_TOML, &[]).await;

    // PUT without auth header
    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let (body, content_type) = make_multipart(&[("test:basic.toml", &valid_test("basic"))]);
    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/filters/{hash}/tests"))
                .header("content-type", content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn returns_updated_filter_details(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let (_, token) = insert_test_user(&pool, "user_details").await;

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let hash = publish_filter_helper(app, &token, VALID_FILTER_TOML, &[]).await;

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let resp = put_tests(
        app,
        &token,
        &hash,
        &[
            ("test:a.toml", &valid_test("a")),
            ("test:b.toml", &valid_test("b")),
            ("test:c.toml", &valid_test("c")),
        ],
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["content_hash"], hash);
    assert_eq!(json["command_pattern"], "my-tool");
    assert_eq!(json["author"], "user_details");
    assert_eq!(json["test_count"], 3);
    assert!(
        json["registry_url"].as_str().unwrap().contains(&hash),
        "registry_url should contain hash"
    );
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn invalid_hash_format_gets_400(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let (_, token) = insert_test_user(&pool, "user_bad_hash").await;

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    // Too short
    let resp = put_tests(
        app,
        &token,
        "abc123",
        &[("test:new.toml", &valid_test("new"))],
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    // Non-hex characters
    let resp = put_tests(
        app,
        &token,
        &format!("{}zzzz", "0".repeat(60)),
        &[("test:new.toml", &valid_test("new"))],
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn semantically_invalid_test_gets_400(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let (_, token) = insert_test_user(&pool, "user_semantic").await;

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let hash = publish_filter_helper(app, &token, VALID_FILTER_TOML, &[]).await;

    // Valid TOML but missing [[expect]] block
    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let resp = put_tests(
        app,
        &token,
        &hash,
        &[("test:no_expect.toml", b"name = \"no expects\"")],
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn zero_to_n_tests_update(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let (_, token) = insert_test_user(&pool, "user_zero_n").await;

    // Publish with no explicit tests (auto-adds 1 default passing test)
    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let hash = publish_filter_helper(app, &token, VALID_FILTER_TOML, &[]).await;

    // Verify 1 test (auto-added default)
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM filter_tests WHERE filter_hash = $1")
        .bind(&hash)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "expected 1 auto-added default test before update");

    // Update to 2 tests
    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let resp = put_tests(
        app,
        &token,
        &hash,
        &[
            ("test:a.toml", &valid_test("a")),
            ("test:b.toml", &valid_test("b")),
        ],
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM filter_tests WHERE filter_hash = $1")
        .bind(&hash)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2, "expected 2 tests after update");
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn double_update_replaces_fully(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let (_, token) = insert_test_user(&pool, "user_double").await;

    // Publish with 1 test
    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let hash = publish_filter_helper(
        app,
        &token,
        VALID_FILTER_TOML,
        &[("test:v1.toml", &valid_test("v1"))],
    )
    .await;

    // First update: 2 tests
    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let resp = put_tests(
        app,
        &token,
        &hash,
        &[
            ("test:v2a.toml", &valid_test("v2a")),
            ("test:v2b.toml", &valid_test("v2b")),
        ],
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Second update: 1 test
    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let resp = put_tests(app, &token, &hash, &[("test:v3.toml", &valid_test("v3"))]).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify only 1 test remains
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM filter_tests WHERE filter_hash = $1")
        .bind(&hash)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "expected 1 test after second update");

    // Verify v2 tests were cleaned from storage
    assert!(
        !storage
            .exists(&format!("filters/{hash}/tests/v2a.toml"))
            .await
            .unwrap(),
        "v2a should be deleted from storage"
    );
    assert!(
        !storage
            .exists(&format!("filters/{hash}/tests/v2b.toml"))
            .await
            .unwrap(),
        "v2b should be deleted from storage"
    );
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn old_storage_objects_cleaned_up(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let (_, token) = insert_test_user(&pool, "user_cleanup").await;

    // Publish with 2 tests
    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let hash = publish_filter_helper(
        app,
        &token,
        VALID_FILTER_TOML,
        &[
            ("test:old1.toml", &valid_test("old1")),
            ("test:old2.toml", &valid_test("old2")),
        ],
    )
    .await;

    let initial_delete_count = storage.delete_count();

    // Update to 1 test
    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let resp = put_tests(app, &token, &hash, &[("test:new.toml", &valid_test("new"))]).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 2 old tests should have been deleted from storage
    assert_eq!(
        storage.delete_count() - initial_delete_count,
        2,
        "expected 2 delete calls for old test files"
    );
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn update_tests_rejects_failing_tests(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let (_, token) = insert_test_user(&pool, "user_fail_update").await;

    // Publish with passing test
    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let hash = publish_filter_helper(
        app,
        &token,
        VALID_FILTER_TOML,
        &[("test:pass.toml", &valid_test("pass"))],
    )
    .await;

    // Try to update with a failing test
    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let failing_test =
        b"name = \"bad\"\ninline = \"hello\"\n\n[[expect]]\ncontains = \"not present\"\n";
    let resp = put_tests(app, &token, &hash, &[("test:bad.toml", failing_test)]).await;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json["error"].as_str().unwrap().contains("tests failed"),
        "expected test failure message, got: {}",
        json["error"]
    );

    // Verify old tests were NOT swapped (still 1 from initial publish)
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM filter_tests WHERE filter_hash = $1")
        .bind(&hash)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "tests should not have been swapped");
}
