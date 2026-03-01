//! E2E tests for the publish-stdlib flow.
//!
//! Each test is `#[ignore]` — only runs when `DATABASE_URL` is set.
//! These tests spin up a real axum server with in-memory storage and exercise
//! the `/api/filters/publish-stdlib` endpoint through the HTTP client, mirroring
//! what the `tokf publish-stdlib` CLI command does.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod harness;

use serde::{Deserialize, Serialize};
use tokf::remote::http::Client;

// Mirror the CLI-side types for the publish-stdlib request/response.
#[derive(Debug, Serialize)]
struct StdlibPublishRequest {
    filters: Vec<StdlibFilterEntry>,
}

#[derive(Debug, Serialize)]
struct StdlibFilterEntry {
    filter_toml: String,
    test_files: Vec<StdlibTestFile>,
    author_github_username: String,
}

#[derive(Debug, Serialize)]
struct StdlibTestFile {
    filename: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct StdlibPublishResponse {
    published: usize,
    skipped: usize,
    failed: Vec<StdlibFailure>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct StdlibFailure {
    command_pattern: String,
    error: String,
}

fn make_entry(command: &str, inline: &str, expect: &str) -> StdlibFilterEntry {
    StdlibFilterEntry {
        filter_toml: format!("command = \"{command}\"\n"),
        test_files: vec![StdlibTestFile {
            filename: "default.toml".to_string(),
            content: format!("name = \"default\"\ninline = \"{inline}\"\n\n[[expect]]\n{expect}\n"),
        }],
        author_github_username: "testuser".to_string(),
    }
}

fn make_passthrough_entry(command: &str) -> StdlibFilterEntry {
    make_entry(command, "", "equals = \"\"")
}

/// Helper to insert a service token and return the raw token string.
async fn insert_service_token(pool: &sqlx::PgPool) -> String {
    let token = tokf_server::auth::token::generate_token();
    let hash = tokf_server::auth::token::hash_token(&token);
    sqlx::query("INSERT INTO service_tokens (token_hash, description) VALUES ($1, $2)")
        .bind(&hash)
        .bind("e2e-test")
        .execute(pool)
        .await
        .expect("failed to insert service token");
    token
}

/// Publish a single stdlib filter → success.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn stdlib_publish_single_filter(pool: PgPool) {
    let h = harness::TestHarness::with_storage(pool.clone()).await;
    let service_token = insert_service_token(&pool).await;

    let req = StdlibPublishRequest {
        filters: vec![make_passthrough_entry("my-tool")],
    };

    let resp: StdlibPublishResponse = tokio::task::spawn_blocking({
        let base_url = h.base_url.clone();
        move || {
            let client = Client::new(&base_url, Some(&service_token)).unwrap();
            client
                .post::<_, StdlibPublishResponse>("/api/filters/publish-stdlib", &req)
                .unwrap()
        }
    })
    .await
    .unwrap();

    assert_eq!(resp.published, 1);
    assert_eq!(resp.skipped, 0);
    assert!(
        resp.failed.is_empty(),
        "unexpected failures: {:?}",
        resp.failed
    );

    // Verify is_stdlib = true in DB
    let is_stdlib: bool =
        sqlx::query_scalar("SELECT is_stdlib FROM filters WHERE command_pattern = 'my-tool'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(is_stdlib);
}

/// Publishing the same filter twice → second is skipped (idempotent).
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn stdlib_publish_idempotent(pool: PgPool) {
    let h = harness::TestHarness::with_storage(pool.clone()).await;
    let service_token = insert_service_token(&pool).await;

    let req = StdlibPublishRequest {
        filters: vec![make_passthrough_entry("idempotent-tool")],
    };

    // First publish
    let resp1: StdlibPublishResponse = tokio::task::spawn_blocking({
        let base_url = h.base_url.clone();
        let token = service_token.clone();
        let req = StdlibPublishRequest {
            filters: vec![make_passthrough_entry("idempotent-tool")],
        };
        move || {
            let client = Client::new(&base_url, Some(&token)).unwrap();
            client.post("/api/filters/publish-stdlib", &req).unwrap()
        }
    })
    .await
    .unwrap();
    assert_eq!(resp1.published, 1);

    // Second publish (same content)
    let resp2: StdlibPublishResponse = tokio::task::spawn_blocking({
        let base_url = h.base_url.clone();
        let token = service_token;
        move || {
            let client = Client::new(&base_url, Some(&token)).unwrap();
            client.post("/api/filters/publish-stdlib", &req).unwrap()
        }
    })
    .await
    .unwrap();
    assert_eq!(resp2.published, 0);
    assert_eq!(resp2.skipped, 1);
    assert!(resp2.failed.is_empty());
}

/// Filter with non-trivial test content (skip rules + expects).
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn stdlib_publish_with_real_filter_logic(pool: PgPool) {
    let h = harness::TestHarness::with_storage(pool.clone()).await;
    let service_token = insert_service_token(&pool).await;

    let filter_toml = r#"command = "test-tool"
skip = ["^noise"]
"#;
    let test_content = r#"name = "skip-noise"
inline = """
noise line
keep this line
"""

[[expect]]
contains = "keep this"

[[expect]]
not_contains = "noise"
"#;

    let req = StdlibPublishRequest {
        filters: vec![StdlibFilterEntry {
            filter_toml: filter_toml.to_string(),
            test_files: vec![StdlibTestFile {
                filename: "skip_noise.toml".to_string(),
                content: test_content.to_string(),
            }],
            author_github_username: "testuser".to_string(),
        }],
    };

    let resp: StdlibPublishResponse = tokio::task::spawn_blocking({
        let base_url = h.base_url.clone();
        move || {
            let client = Client::new(&base_url, Some(&service_token)).unwrap();
            client.post("/api/filters/publish-stdlib", &req).unwrap()
        }
    })
    .await
    .unwrap();

    assert_eq!(resp.published, 1, "filter should be published");
    assert!(
        resp.failed.is_empty(),
        "unexpected failures: {:?}",
        resp.failed
    );
}

/// Test with literal-string inline content containing backslashes.
/// This mirrors what the CLI does when resolving fixtures that contain
/// backslash-heavy content (e.g., gradle separator lines `\---`).
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn stdlib_publish_with_backslash_content(pool: PgPool) {
    let h = harness::TestHarness::with_storage(pool.clone()).await;
    let service_token = insert_service_token(&pool).await;

    // Use TOML literal strings (''') so backslashes are preserved as-is
    let test_content = "name = \"backslash-test\"\ninline = '''\n\\--- dependency graph\n\\--- end\n'''\n\n[[expect]]\ncontains = \"dependency\"\n";

    let req = StdlibPublishRequest {
        filters: vec![StdlibFilterEntry {
            filter_toml: "command = \"backslash-tool\"\n".to_string(),
            test_files: vec![StdlibTestFile {
                filename: "default.toml".to_string(),
                content: test_content.to_string(),
            }],
            author_github_username: "testuser".to_string(),
        }],
    };

    let resp: StdlibPublishResponse = tokio::task::spawn_blocking({
        let base_url = h.base_url.clone();
        move || {
            let client = Client::new(&base_url, Some(&service_token)).unwrap();
            client.post("/api/filters/publish-stdlib", &req).unwrap()
        }
    })
    .await
    .unwrap();

    assert_eq!(
        resp.published, 1,
        "filter with backslash content should publish"
    );
    assert!(
        resp.failed.is_empty(),
        "backslash content should not cause failures: {:?}",
        resp.failed
    );
}

/// Publish multiple filters one-by-one (mirrors CLI behavior).
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn stdlib_publish_one_by_one(pool: PgPool) {
    let h = harness::TestHarness::with_storage(pool.clone()).await;
    let service_token = insert_service_token(&pool).await;

    let commands = ["tool-alpha", "tool-beta", "tool-gamma"];
    let mut published = 0usize;
    let mut failed = 0usize;

    for cmd in &commands {
        let req = StdlibPublishRequest {
            filters: vec![make_passthrough_entry(cmd)],
        };
        let resp: StdlibPublishResponse = tokio::task::spawn_blocking({
            let base_url = h.base_url.clone();
            let token = service_token.clone();
            move || {
                let client = Client::new(&base_url, Some(&token)).unwrap();
                client.post("/api/filters/publish-stdlib", &req).unwrap()
            }
        })
        .await
        .unwrap();

        published += resp.published;
        failed += resp.failed.len();
    }

    assert_eq!(published, 3, "all 3 filters should be published");
    assert_eq!(failed, 0, "no failures expected");

    // Verify all 3 are in the DB with is_stdlib = true
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM filters WHERE is_stdlib = TRUE")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 3);
}

/// Invalid service token → 401.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn stdlib_publish_rejects_invalid_token(pool: PgPool) {
    let h = harness::TestHarness::with_storage(pool.clone()).await;

    let req = StdlibPublishRequest {
        filters: vec![make_passthrough_entry("rejected-tool")],
    };

    let result = tokio::task::spawn_blocking({
        let base_url = h.base_url.clone();
        move || {
            let client = Client::new(&base_url, Some("bad-token")).unwrap();
            client.post::<_, StdlibPublishResponse>("/api/filters/publish-stdlib", &req)
        }
    })
    .await
    .unwrap();

    assert!(result.is_err(), "invalid token should produce an error");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("401") || err.contains("nauthorized"),
        "error should indicate auth failure, got: {err}"
    );
}

/// Published stdlib filter appears in search results with is_stdlib = true.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn stdlib_filter_appears_in_search(pool: PgPool) {
    let h = harness::TestHarness::with_storage(pool.clone()).await;
    let service_token = insert_service_token(&pool).await;

    // Publish via stdlib endpoint
    let req = StdlibPublishRequest {
        filters: vec![make_passthrough_entry("searchable-stdlib")],
    };

    let resp: StdlibPublishResponse = tokio::task::spawn_blocking({
        let base_url = h.base_url.clone();
        let token = service_token;
        move || {
            let client = Client::new(&base_url, Some(&token)).unwrap();
            client.post("/api/filters/publish-stdlib", &req).unwrap()
        }
    })
    .await
    .unwrap();
    assert_eq!(resp.published, 1);

    // Search for it using the regular user token
    let results = h.blocking_search_filters("searchable", 10).await;
    assert!(
        !results.is_empty(),
        "stdlib filter should appear in search results"
    );
    let found = results
        .iter()
        .find(|f| f.command_pattern == "searchable-stdlib");
    assert!(found.is_some(), "should find the specific filter");
    assert!(
        found.unwrap().is_stdlib,
        "filter should be marked as stdlib"
    );
}
