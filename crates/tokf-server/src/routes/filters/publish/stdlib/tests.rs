use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

use crate::auth::token::hash_token;
use crate::routes::filters::test_helpers::make_state;
use crate::storage::mock::InMemoryStorageClient;

use super::{StdlibFilterEntry, StdlibPublishRequest, StdlibTestFile};

/// Insert a service token into the DB and return the raw token string.
async fn insert_service_token(pool: &sqlx::PgPool, description: &str) -> String {
    let token = crate::auth::token::generate_token();
    let token_hash = hash_token(&token);
    sqlx::query("INSERT INTO service_tokens (token_hash, description) VALUES ($1, $2)")
        .bind(&token_hash)
        .bind(description)
        .execute(pool)
        .await
        .expect("failed to insert service token");
    token
}

fn make_valid_request() -> StdlibPublishRequest {
    StdlibPublishRequest {
        filters: vec![StdlibFilterEntry {
            filter_toml: "command = \"my-tool\"\n".to_string(),
            test_files: vec![StdlibTestFile {
                filename: "default.toml".to_string(),
                content: "name = \"default\"\ninline = \"\"\n\n[[expect]]\nequals = \"\"\n"
                    .to_string(),
            }],
            author_github_username: "testuser".to_string(),
        }],
    }
}

async fn post_stdlib(
    app: axum::Router,
    token: &str,
    req: &StdlibPublishRequest,
) -> axum::response::Response {
    let body = serde_json::to_vec(req).expect("failed to serialize request");
    app.oneshot(
        Request::builder()
            .method("POST")
            .uri("/api/filters/publish-stdlib")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap(),
    )
    .await
    .unwrap()
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_stdlib_creates_filter_with_is_stdlib(pool: PgPool) {
    let token = insert_service_token(&pool, "ci-test").await;
    let state = make_state(pool.clone());
    let app = crate::routes::create_router(state);

    let req = make_valid_request();
    let resp = post_stdlib(app, &token, &req).await;

    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["published"], 1);
    assert_eq!(json["skipped"], 0);
    assert!(json["failed"].as_array().unwrap().is_empty());

    // Verify is_stdlib = true in DB
    let is_stdlib: bool = sqlx::query_scalar("SELECT is_stdlib FROM filters LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(is_stdlib, "filter should be marked as stdlib");
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_stdlib_is_idempotent(pool: PgPool) {
    let token = insert_service_token(&pool, "ci-test").await;

    // First publish
    let state = make_state(pool.clone());
    let app = crate::routes::create_router(state);
    let req = make_valid_request();
    let resp = post_stdlib(app, &token, &req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Second publish (same content)
    let state = make_state(pool.clone());
    let app = crate::routes::create_router(state);
    let resp = post_stdlib(app, &token, &req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["published"], 0);
    assert_eq!(json["skipped"], 1);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_stdlib_rejects_invalid_token(pool: PgPool) {
    let state = make_state(pool.clone());
    let app = crate::routes::create_router(state);

    let req = make_valid_request();
    let resp = post_stdlib(app, "invalid-token", &req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_stdlib_rejects_missing_auth(pool: PgPool) {
    let state = make_state(pool.clone());
    let app = crate::routes::create_router(state);

    let body = serde_json::to_vec(&make_valid_request()).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/filters/publish-stdlib")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_stdlib_creates_ghost_user(pool: PgPool) {
    let token = insert_service_token(&pool, "ci-test").await;
    let state = make_state(pool.clone());
    let app = crate::routes::create_router(state);

    let req = StdlibPublishRequest {
        filters: vec![StdlibFilterEntry {
            filter_toml: "command = \"ghost-tool\"\n".to_string(),
            test_files: vec![StdlibTestFile {
                filename: "default.toml".to_string(),
                content: "name = \"default\"\ninline = \"\"\n\n[[expect]]\nequals = \"\"\n"
                    .to_string(),
            }],
            author_github_username: "ghost-contributor".to_string(),
        }],
    };
    let resp = post_stdlib(app, &token, &req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Verify ghost user was created with visible = false
    let (username, visible): (String, bool) =
        sqlx::query_as("SELECT username, visible FROM users WHERE username = 'ghost-contributor'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(username, "ghost-contributor");
    assert!(!visible, "ghost user should not be visible");
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_stdlib_stores_tests_in_storage(pool: PgPool) {
    let token = insert_service_token(&pool, "ci-test").await;
    let storage = Arc::new(InMemoryStorageClient::new());
    let mut state = make_state(pool.clone());
    state.storage = storage.clone();
    let app = crate::routes::create_router(state);

    let req = make_valid_request();
    let resp = post_stdlib(app, &token, &req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    // 1 filter + 1 test file = 2 puts
    assert_eq!(storage.put_count(), 2, "expected 2 R2 put calls");
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_stdlib_reports_failing_filter(pool: PgPool) {
    let token = insert_service_token(&pool, "ci-test").await;
    let state = make_state(pool.clone());
    let app = crate::routes::create_router(state);

    let req = StdlibPublishRequest {
        filters: vec![StdlibFilterEntry {
            filter_toml: "not valid toml [[[".to_string(),
            test_files: vec![StdlibTestFile {
                filename: "default.toml".to_string(),
                content: "name = \"default\"\ninline = \"\"\n\n[[expect]]\nequals = \"\"\n"
                    .to_string(),
            }],
            author_github_username: "testuser".to_string(),
        }],
    };
    let resp = post_stdlib(app, &token, &req).await;

    // Multi-status since there's a failure
    assert_eq!(resp.status(), StatusCode::MULTI_STATUS);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["published"], 0);
    assert_eq!(json["failed"].as_array().unwrap().len(), 1);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_stdlib_batch_mixed_results(pool: PgPool) {
    let token = insert_service_token(&pool, "ci-test").await;

    let test_content = "name = \"default\"\ninline = \"\"\n\n[[expect]]\nequals = \"\"\n";

    // First publish filter-a to set up a skip
    let state = make_state(pool.clone());
    let app = crate::routes::create_router(state);
    let seed_req = StdlibPublishRequest {
        filters: vec![StdlibFilterEntry {
            filter_toml: "command = \"filter-a\"\n".to_string(),
            test_files: vec![StdlibTestFile {
                filename: "default.toml".to_string(),
                content: test_content.to_string(),
            }],
            author_github_username: "testuser".to_string(),
        }],
    };
    let resp = post_stdlib(app, &token, &seed_req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Now send a batch with: duplicate filter-a (skip), new filter-b (publish), invalid (fail)
    let state = make_state(pool.clone());
    let app = crate::routes::create_router(state);
    let batch_req = StdlibPublishRequest {
        filters: vec![
            StdlibFilterEntry {
                filter_toml: "command = \"filter-a\"\n".to_string(),
                test_files: vec![StdlibTestFile {
                    filename: "default.toml".to_string(),
                    content: test_content.to_string(),
                }],
                author_github_username: "testuser".to_string(),
            },
            StdlibFilterEntry {
                filter_toml: "command = \"filter-b\"\n".to_string(),
                test_files: vec![StdlibTestFile {
                    filename: "default.toml".to_string(),
                    content: test_content.to_string(),
                }],
                author_github_username: "testuser".to_string(),
            },
            StdlibFilterEntry {
                filter_toml: "invalid toml [[[".to_string(),
                test_files: vec![StdlibTestFile {
                    filename: "default.toml".to_string(),
                    content: test_content.to_string(),
                }],
                author_github_username: "testuser".to_string(),
            },
        ],
    };
    let resp = post_stdlib(app, &token, &batch_req).await;

    assert_eq!(resp.status(), StatusCode::MULTI_STATUS);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["published"], 1, "filter-b should be published");
    assert_eq!(json["skipped"], 1, "filter-a should be skipped (duplicate)");
    assert_eq!(
        json["failed"].as_array().unwrap().len(),
        1,
        "invalid toml should fail"
    );
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn publish_stdlib_rejects_invalid_username(pool: PgPool) {
    let token = insert_service_token(&pool, "ci-test").await;
    let state = make_state(pool.clone());
    let app = crate::routes::create_router(state);

    let req = StdlibPublishRequest {
        filters: vec![StdlibFilterEntry {
            filter_toml: "command = \"my-tool\"\n".to_string(),
            test_files: vec![StdlibTestFile {
                filename: "default.toml".to_string(),
                content: "name = \"default\"\ninline = \"\"\n\n[[expect]]\nequals = \"\"\n"
                    .to_string(),
            }],
            author_github_username: "-invalid-".to_string(),
        }],
    };
    let resp = post_stdlib(app, &token, &req).await;

    assert_eq!(resp.status(), StatusCode::MULTI_STATUS);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["failed"].as_array().unwrap().len(), 1);
    let error = json["failed"][0]["error"].as_str().unwrap();
    assert!(
        error.contains("GitHub username"),
        "error should mention username: {error}"
    );
}
