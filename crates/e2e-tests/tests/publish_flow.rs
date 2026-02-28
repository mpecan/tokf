//! E2E tests for filter publish, search, download, and update-tests flows.
//!
//! Each test is `#[ignore]` — only runs when `DATABASE_URL` is set.
//! These tests require `InMemoryStorageClient` (via `TestHarness::with_storage`)
//! so that filter TOML and test files actually persist in memory.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod harness;

const FILTER_TOML: &[u8] = b"command = \"git push\"\n";

fn valid_test(name: &str) -> (String, Vec<u8>) {
    let content =
        format!("name = \"{name}\"\ninline = \"ok output\"\n\n[[expect]]\ncontains = \"ok\"\n");
    (format!("{name}.toml"), content.into_bytes())
}

fn default_test() -> (String, Vec<u8>) {
    (
        "default.toml".to_string(),
        b"name = \"default\"\ninline = \"\"\n\n[[expect]]\nequals = \"\"\n".to_vec(),
    )
}

/// Publish a filter → verify is_new=true, hash returned.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn publish_filter_returns_hash(pool: PgPool) {
    let h = harness::TestHarness::with_storage(pool).await;

    let (is_new, resp) = h
        .blocking_publish(FILTER_TOML.to_vec(), vec![default_test()])
        .await;

    assert!(is_new, "expected is_new=true for first publish");
    assert!(!resp.content_hash.is_empty());
    assert_eq!(resp.command_pattern, "git push");
    assert_eq!(resp.author, "testuser");
}

/// Publish with invalid test file → error (server validates test files on publish).
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn publish_with_invalid_test_fails(pool: PgPool) {
    let h = harness::TestHarness::with_storage(pool).await;

    let bad_test = ("bad.toml".to_string(), b"name = \"bad\"\n".to_vec());
    let result = h.try_publish(FILTER_TOML.to_vec(), vec![bad_test]).await;

    assert!(result.is_err());
}

/// Publish same filter twice → second publish returns is_new=false.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn duplicate_publish_returns_existing(pool: PgPool) {
    let h = harness::TestHarness::with_storage(pool).await;

    let (is_new1, resp1) = h
        .blocking_publish(FILTER_TOML.to_vec(), vec![default_test()])
        .await;
    assert!(is_new1);

    let (is_new2, resp2) = h
        .blocking_publish(FILTER_TOML.to_vec(), vec![default_test()])
        .await;
    assert!(!is_new2, "expected is_new=false for duplicate publish");
    assert_eq!(resp1.content_hash, resp2.content_hash);
}

/// Publish with tests → search → get details → download → verify test files.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn publish_search_download_lifecycle(pool: PgPool) {
    let h = harness::TestHarness::with_storage(pool).await;

    let tests = vec![valid_test("basic"), valid_test("edge")];
    let (is_new, resp) = h.blocking_publish(FILTER_TOML.to_vec(), tests).await;
    assert!(is_new);

    // Search by command pattern
    let results = h.blocking_search_filters("git", 10).await;
    assert!(
        results.iter().any(|f| f.content_hash == resp.content_hash),
        "published filter should appear in search results"
    );

    // Get details
    let details = h.blocking_get_filter(&resp.content_hash).await;
    assert_eq!(details.content_hash, resp.content_hash);
    assert_eq!(details.command_pattern, "git push");
    assert_eq!(details.test_count, 2);

    // Download
    let dl = h.blocking_download_filter(&resp.content_hash).await;
    assert!(dl.filter_toml.contains("git push"));
    assert_eq!(dl.test_files.len(), 2);
    let filenames: std::collections::HashSet<_> =
        dl.test_files.iter().map(|f| f.filename.as_str()).collect();
    assert!(filenames.contains("basic.toml"));
    assert!(filenames.contains("edge.toml"));
}

/// Publish with 1 test → update to 3 tests → verify via download.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn update_tests_replaces_test_suite(pool: PgPool) {
    let h = harness::TestHarness::with_storage(pool).await;

    // Publish with 1 test
    let (_, resp) = h
        .blocking_publish(FILTER_TOML.to_vec(), vec![valid_test("old")])
        .await;

    // Update to 3 tests
    let new_tests = vec![valid_test("new1"), valid_test("new2"), valid_test("new3")];
    let update_resp = h.blocking_update_tests(&resp.content_hash, new_tests).await;
    assert_eq!(update_resp.test_count, 3);
    assert_eq!(update_resp.content_hash, resp.content_hash);

    // Verify via download: old test gone, 3 new tests present
    let dl = h.blocking_download_filter(&resp.content_hash).await;
    assert_eq!(dl.test_files.len(), 3, "expected 3 test files after update");
    let filenames: std::collections::HashSet<_> =
        dl.test_files.iter().map(|f| f.filename.as_str()).collect();
    assert!(!filenames.contains("old.toml"), "old test should be gone");
    assert!(filenames.contains("new1.toml"));
    assert!(filenames.contains("new2.toml"));
    assert!(filenames.contains("new3.toml"));
}

/// Update tests on unknown hash → error.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn update_tests_unknown_hash_fails(pool: PgPool) {
    let h = harness::TestHarness::with_storage(pool).await;

    let fake_hash = "0".repeat(64);
    let result = h.try_update_tests(&fake_hash, vec![valid_test("x")]).await;

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("not found"),
        "expected 'not found' error, got: {err_msg}"
    );
}

/// Update tests twice → verify only the latest set survives.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn double_update_tests_round_trip(pool: PgPool) {
    let h = harness::TestHarness::with_storage(pool).await;

    let (_, resp) = h
        .blocking_publish(FILTER_TOML.to_vec(), vec![valid_test("v1")])
        .await;

    // First update: 2 tests
    h.blocking_update_tests(
        &resp.content_hash,
        vec![valid_test("v2a"), valid_test("v2b")],
    )
    .await;

    // Second update: 1 test
    let update = h
        .blocking_update_tests(&resp.content_hash, vec![valid_test("v3")])
        .await;
    assert_eq!(update.test_count, 1);

    // Download and verify only v3 survives
    let dl = h.blocking_download_filter(&resp.content_hash).await;
    assert_eq!(dl.test_files.len(), 1);
    assert_eq!(dl.test_files[0].filename, "v3.toml");
}

/// Non-author update → error via try_update_tests_with_token.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn non_author_update_tests_fails(pool: PgPool) {
    let h = harness::TestHarness::with_storage(pool).await;

    let (_, resp) = h
        .blocking_publish(FILTER_TOML.to_vec(), vec![valid_test("original")])
        .await;

    // Create a second user
    let other_token = h.create_other_user_token().await;

    // Other user tries to update tests
    let result = h
        .try_update_tests_with_token(
            &resp.content_hash,
            vec![valid_test("hijacked")],
            &other_token,
        )
        .await;

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("not the author"),
        "expected 'not the author' error, got: {err_msg}"
    );
}

/// Invalid test content → error via try_update_tests.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn update_with_invalid_test_content_fails(pool: PgPool) {
    let h = harness::TestHarness::with_storage(pool).await;

    let (_, resp) = h
        .blocking_publish(FILTER_TOML.to_vec(), vec![valid_test("good")])
        .await;

    // Try to update with invalid test file (no [[expect]] block)
    let bad_test = ("bad.toml".to_string(), b"name = \"bad\"\n".to_vec());
    let result = h.try_update_tests(&resp.content_hash, vec![bad_test]).await;

    assert!(result.is_err());
}

/// Publish → get details → verify test_count is correct.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn get_filter_shows_correct_test_count(pool: PgPool) {
    let h = harness::TestHarness::with_storage(pool).await;

    let tests = vec![valid_test("a"), valid_test("b")];
    let (_, resp) = h.blocking_publish(FILTER_TOML.to_vec(), tests).await;

    let details = h.blocking_get_filter(&resp.content_hash).await;
    assert_eq!(details.test_count, 2);

    // Update to 1 test
    let update_resp = h
        .blocking_update_tests(&resp.content_hash, vec![valid_test("only")])
        .await;
    assert_eq!(update_resp.test_count, 1);

    // Verify details updated
    let details = h.blocking_get_filter(&resp.content_hash).await;
    assert_eq!(details.test_count, 1, "test_count should reflect update");
}
