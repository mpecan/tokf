use std::sync::Arc;

use axum::http::StatusCode;
use http_body_util::BodyExt;
use sqlx::PgPool;

use crate::storage::mock::InMemoryStorageClient;

use super::test_helpers::{
    get_request, insert_test_user, make_state, make_state_with_storage, publish_filter_helper,
};

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn search_returns_empty_for_empty_registry(pool: PgPool) {
    let (_, token) = insert_test_user(&pool, "search_empty").await;
    let app = crate::routes::create_router(make_state(pool));

    let resp = get_request(app, &token, "/api/filters").await;

    assert_eq!(resp.status(), StatusCode::OK);
    // Search responses must include rate-limit headers.
    assert!(
        resp.headers().contains_key("x-ratelimit-limit"),
        "search response should include x-ratelimit-limit"
    );
    assert!(
        resp.headers().contains_key("x-ratelimit-remaining"),
        "search response should include x-ratelimit-remaining"
    );
    assert!(
        resp.headers().contains_key("x-ratelimit-reset"),
        "search response should include x-ratelimit-reset"
    );
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json, serde_json::json!([]));
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn search_matches_command_pattern(pool: PgPool) {
    let (_, token) = insert_test_user(&pool, "search_match").await;
    let storage = Arc::new(InMemoryStorageClient::new());

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    publish_filter_helper(app, &token, b"command = \"git push\"\n", &[]).await;

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    publish_filter_helper(app, &token, b"command = \"cargo build\"\n", &[]).await;

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let resp = get_request(app, &token, "/api/filters?q=git").await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let results: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(results.len(), 1, "expected 1 result for q=git");
    assert_eq!(results[0]["command_pattern"], "git push");
    assert_eq!(
        results[0]["is_stdlib"], false,
        "community filter should have is_stdlib=false"
    );
}

/// Publish a list of filter TOMLs, then search with the given URI and return the result list.
async fn publish_and_search(
    pool: PgPool,
    username: &str,
    tomls: &[&[u8]],
    search_uri: &str,
) -> Vec<serde_json::Value> {
    let (_, token) = insert_test_user(&pool, username).await;
    let storage = Arc::new(InMemoryStorageClient::new());

    for toml in tomls {
        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        publish_filter_helper(app, &token, toml, &[]).await;
    }

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let resp = get_request(app, &token, search_uri).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn search_empty_q_returns_all(pool: PgPool) {
    let results = publish_and_search(
        pool,
        "search_all",
        &[b"command = \"git push\"\n", b"command = \"cargo check\"\n"],
        "/api/filters",
    )
    .await;
    assert_eq!(results.len(), 2, "expected all 2 results");
}

const UNKNOWN_HASH: &str = "deadbeef00000000000000000000000000000000000000000000000000000000";

/// Assert that requesting an unknown filter hash at the given path suffix returns 404.
async fn assert_unknown_hash_returns_404(pool: PgPool, username: &str, path_suffix: &str) {
    let (_, token) = insert_test_user(&pool, username).await;
    let app = crate::routes::create_router(make_state(pool));
    let resp = get_request(
        app,
        &token,
        &format!("/api/filters/{UNKNOWN_HASH}{path_suffix}"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn get_filter_returns_404_for_unknown_hash(pool: PgPool) {
    assert_unknown_hash_returns_404(pool, "get_404", "").await;
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn get_filter_returns_details(pool: PgPool) {
    let (_, token) = insert_test_user(&pool, "get_details").await;
    let storage = Arc::new(InMemoryStorageClient::new());

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let hash = publish_filter_helper(
        app,
        &token,
        b"command = \"git push\"\n",
        &[(
            "test:basic.toml",
            b"name = \"basic\"\ninline = \"ok output\"\n\n[[expect]]\ncontains = \"ok\"\n",
        )],
    )
    .await;

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let resp = get_request(app, &token, &format!("/api/filters/{hash}")).await;

    assert_eq!(resp.status(), StatusCode::OK);
    // Get-filter responses must include rate-limit headers.
    assert!(
        resp.headers().contains_key("x-ratelimit-limit"),
        "get-filter response should include x-ratelimit-limit"
    );
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["content_hash"], hash);
    assert_eq!(json["command_pattern"], "git push");
    assert_eq!(json["author"], "get_details");
    assert_eq!(json["savings_pct"], 0.0);
    assert_eq!(json["total_commands"], 0);
    assert_eq!(json["test_count"], 1);
    assert!(
        json["registry_url"].as_str().unwrap().contains(&hash),
        "registry_url should contain hash"
    );
    assert!(
        json["created_at"].is_string(),
        "created_at should be a string"
    );
    assert_eq!(
        json["is_stdlib"], false,
        "community-published filter should not be stdlib"
    );
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn download_returns_toml_content(pool: PgPool) {
    let (_, token) = insert_test_user(&pool, "dl_toml").await;
    let storage = Arc::new(InMemoryStorageClient::new());
    let filter_toml = b"command = \"git push\"\n";

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let hash = publish_filter_helper(app, &token, filter_toml, &[]).await;

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let resp = get_request(app, &token, &format!("/api/filters/{hash}/download")).await;

    assert_eq!(resp.status(), StatusCode::OK);
    // Download responses must include rate-limit headers.
    assert!(
        resp.headers().contains_key("x-ratelimit-limit"),
        "download response should include x-ratelimit-limit"
    );
    assert!(
        resp.headers().contains_key("x-ratelimit-remaining"),
        "download response should include x-ratelimit-remaining"
    );
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["filter_toml"].as_str().unwrap(),
        std::str::from_utf8(filter_toml).unwrap()
    );
    // publish_filter_helper auto-adds a default passing test when none provided
    let test_files = json["test_files"].as_array().unwrap();
    assert_eq!(test_files.len(), 1, "expected 1 auto-added default test");
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn download_returns_test_files(pool: PgPool) {
    let (_, token) = insert_test_user(&pool, "dl_tests").await;
    let storage = Arc::new(InMemoryStorageClient::new());

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let hash = publish_filter_helper(
        app,
        &token,
        b"command = \"git push\"\n",
        &[
            (
                "test:basic.toml",
                b"name = \"basic\"\ninline = \"ok output\"\n\n[[expect]]\ncontains = \"ok\"\n",
            ),
            (
                "test:edge.toml",
                b"name = \"edge\"\ninline = \"ok output\"\n\n[[expect]]\ncontains = \"ok\"\n",
            ),
        ],
    )
    .await;

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let resp = get_request(app, &token, &format!("/api/filters/{hash}/download")).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let test_files = json["test_files"].as_array().unwrap();
    assert_eq!(test_files.len(), 2, "expected 2 test files");

    let filenames: std::collections::HashSet<&str> = test_files
        .iter()
        .map(|f| f["filename"].as_str().unwrap())
        .collect();
    assert!(filenames.contains("basic.toml"), "expected basic.toml");
    assert!(filenames.contains("edge.toml"), "expected edge.toml");
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn download_returns_404_for_unknown_hash(pool: PgPool) {
    assert_unknown_hash_returns_404(pool, "dl_404", "/download").await;
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn search_limit_is_respected(pool: PgPool) {
    let results = publish_and_search(
        pool,
        "search_limit",
        &[
            b"command = \"git push\"\n",
            b"command = \"git pull\"\n",
            b"command = \"git fetch\"\n",
        ],
        "/api/filters?limit=1",
    )
    .await;
    assert_eq!(results.len(), 1, "limit=1 should return at most 1 result");
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn search_rejects_oversized_query(pool: PgPool) {
    let (_, token) = insert_test_user(&pool, "search_big_q").await;
    let app = crate::routes::create_router(make_state(pool));

    let long_query = "a".repeat(201);
    let resp = get_request(app, &token, &format!("/api/filters?q={long_query}")).await;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn search_wildcard_injection_does_not_return_all(pool: PgPool) {
    let (_, token) = insert_test_user(&pool, "wildcard_test").await;
    let storage = Arc::new(InMemoryStorageClient::new());

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    publish_filter_helper(app, &token, b"command = \"cargo build\"\n", &[]).await;

    // Search for "_" — without escaping this would match any single character
    // and return all filters. With proper escaping it matches literal underscore.
    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let resp = get_request(app, &token, "/api/filters?q=_").await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let results: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        results.len(),
        0,
        "escaped underscore should match nothing (no underscore in 'cargo build')"
    );
}
