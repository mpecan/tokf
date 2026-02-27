use axum::{Json, extract::State, http::StatusCode};
use bytes::Bytes;
use serde::Serialize;
use tokf_common::config::types::FilterConfig;
use tokf_common::hash::canonical_hash;

use crate::auth::token::AuthUser;
use crate::error::AppError;
use crate::state::AppState;
use crate::storage;

pub(super) const MAX_FILTER_SIZE: usize = 64 * 1_024;
pub(super) const MAX_TOTAL_SIZE: usize = 1_024 * 1_024;

// ── Response type ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PublishFilterResponse {
    pub content_hash: String,
    pub command_pattern: String,
    pub author: String,
    pub registry_url: String,
}

// ── Internal helpers ──────────────────────────────────────────────────────────

struct MultipartFields {
    filter_bytes: Vec<u8>,
    test_files: Vec<(String, Vec<u8>)>,
    mit_license_accepted: bool,
}

async fn parse_multipart(
    multipart: &mut axum::extract::Multipart,
) -> Result<MultipartFields, AppError> {
    let mut filter_bytes: Option<Vec<u8>> = None;
    let mut test_files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut total_size: usize = 0;
    let mut mit_license_accepted = false;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?
    {
        let Some(name) = field.name() else {
            tracing::debug!("ignoring unnamed multipart field");
            continue;
        };
        let name = name.to_string();
        let bytes: Bytes = field
            .bytes()
            .await
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
        total_size = total_size.saturating_add(bytes.len());
        if total_size > MAX_TOTAL_SIZE {
            return Err(AppError::BadRequest(
                "upload exceeds 1 MB total size limit".to_string(),
            ));
        }
        if name == "filter" {
            if bytes.len() > MAX_FILTER_SIZE {
                return Err(AppError::BadRequest(
                    "filter TOML exceeds 64 KB size limit".to_string(),
                ));
            }
            filter_bytes = Some(bytes.to_vec());
        } else if name == "mit_license_accepted" {
            mit_license_accepted = bytes.as_ref() == b"true";
        } else if let Some(filename) = name.strip_prefix("test:")
            && !filename.is_empty()
        {
            test_files.push((filename.to_string(), bytes.to_vec()));
        }
    }

    let filter_bytes = filter_bytes
        .ok_or_else(|| AppError::BadRequest("missing required 'filter' field".to_string()))?;
    Ok(MultipartFields {
        filter_bytes,
        test_files,
        mit_license_accepted,
    })
}

/// Grouped fields for inserting a filter record.
struct FilterInsert<'a> {
    content_hash: &'a str,
    command_pattern: &'a str,
    canonical_command: &'a str,
    author_id: i64,
    r2_key: &'a str,
}

/// Returns `(author_username, was_new)` for the filter with `content_hash`.
///
/// Attempts an INSERT and uses the result to distinguish new vs duplicate.
/// Uploading to R2 before this call means orphaned objects are possible on DB
/// failure, but they are harmless (no user-visible state and R2 uploads are
/// idempotent on retry).
async fn upsert_filter_record(
    state: &AppState,
    insert: &FilterInsert<'_>,
    author_username: &str,
) -> Result<(String, bool), AppError> {
    let result = sqlx::query(
        "INSERT INTO filters (content_hash, command_pattern, canonical_command, author_id, r2_key)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (content_hash) DO NOTHING",
    )
    .bind(insert.content_hash)
    .bind(insert.command_pattern)
    .bind(insert.canonical_command)
    .bind(insert.author_id)
    .bind(insert.r2_key)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        // Duplicate — fetch the original author's username
        let existing_author: String = sqlx::query_scalar(
            "SELECT u.username FROM filters f
             JOIN users u ON u.id = f.author_id
             WHERE f.content_hash = $1",
        )
        .bind(insert.content_hash)
        .fetch_one(&state.db)
        .await?;
        return Ok((existing_author, false));
    }

    Ok((author_username.to_string(), true))
}

pub(super) async fn upload_tests(
    state: &AppState,
    content_hash: &str,
    test_files: Vec<(String, Vec<u8>)>,
) -> Result<Vec<String>, AppError> {
    let mut keys = Vec::new();
    for (filename, bytes) in test_files {
        let key = storage::upload_test(&*state.storage, content_hash, &filename, bytes)
            .await
            .map_err(|e| AppError::Internal(format!("storage error for {filename}: {e}")))?;
        keys.push(key);
    }
    Ok(keys)
}

pub(super) async fn insert_filter_tests(
    pool: &sqlx::PgPool,
    content_hash: &str,
    test_r2_keys: &[String],
) -> Result<(), AppError> {
    for key in test_r2_keys {
        sqlx::query("INSERT INTO filter_tests (filter_hash, r2_key) VALUES ($1, $2)")
            .bind(content_hash)
            .bind(key)
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Validated, hashed filter ready for storage.
struct PreparedFilter {
    content_hash: String,
    command_pattern: String,
    canonical_command: String,
    filter_bytes: Vec<u8>,
    test_files: Vec<(String, Vec<u8>)>,
}

/// Validate the multipart fields and compute the content hash.
fn prepare_filter(fields: MultipartFields) -> Result<PreparedFilter, AppError> {
    if !fields.mit_license_accepted {
        return Err(AppError::BadRequest(
            "MIT license not accepted — set 'mit_license_accepted' to 'true'".to_string(),
        ));
    }
    let toml_str = std::str::from_utf8(&fields.filter_bytes)
        .map_err(|_| AppError::BadRequest("filter TOML is not valid UTF-8".to_string()))?;
    let config: FilterConfig = toml::from_str(toml_str)
        .map_err(|e| AppError::BadRequest(format!("invalid filter TOML: {e}")))?;
    let command_pattern = config.command.first().to_string();
    if command_pattern.is_empty() {
        return Err(AppError::BadRequest(
            "filter must have at least one non-empty command".to_string(),
        ));
    }
    let canonical_command = command_pattern
        .split_whitespace()
        .next()
        .unwrap_or(&command_pattern)
        .to_string();
    // Validate test files
    for (filename, bytes) in &fields.test_files {
        tokf_common::test_case::validate(bytes)
            .map_err(|e| AppError::BadRequest(format!("{filename}: {e}")))?;
    }

    let content_hash =
        canonical_hash(&config).map_err(|e| AppError::Internal(format!("hash error: {e}")))?;
    Ok(PreparedFilter {
        content_hash,
        command_pattern,
        canonical_command,
        filter_bytes: fields.filter_bytes,
        test_files: fields.test_files,
    })
}

// ── POST /api/filters ─────────────────────────────────────────────────────────

/// Publish a filter TOML and optional test files to the community registry.
///
/// Accepts a multipart form with:
/// - `filter` — filter TOML bytes (required, ≤ 64 KB)
/// - `mit_license_accepted` — must be `"true"` to acknowledge MIT license (required)
/// - `test:<filename>` — individual test TOML files (optional, total upload ≤ 1 MB)
///
/// The server computes the content hash from the uploaded bytes; clients never
/// supply a hash. This prevents hash forgery.
///
/// Returns `201 Created` on first publish, `200 OK` if the same hash was
/// already in the registry (idempotent).
///
/// # Errors
///
/// - `400 Bad Request` if the multipart is malformed, the TOML is invalid,
///   size limits are exceeded, or MIT license was not accepted.
/// - `401 Unauthorized` if the bearer token is missing or invalid.
/// - `429 Too Many Requests` if the user exceeds publish rate limits.
/// - `500 Internal Server Error` on storage or database failures.
pub async fn publish_filter(
    auth: AuthUser,
    State(state): State<AppState>,
    mut multipart: axum::extract::Multipart,
) -> Result<(StatusCode, Json<PublishFilterResponse>), AppError> {
    if !state.publish_rate_limiter.check_and_increment(auth.user_id) {
        return Err(AppError::RateLimited);
    }

    let fields = parse_multipart(&mut multipart).await?;
    let prepared = prepare_filter(fields)?;

    // Upload to R2 first (idempotent). If the DB insert below fails, the R2
    // object is orphaned but harmless — no user-visible state changes, and
    // the upload is retried correctly on the next publish attempt.
    let r2_key = storage::upload_filter(
        &*state.storage,
        &prepared.content_hash,
        prepared.filter_bytes,
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;
    let test_r2_keys = upload_tests(&state, &prepared.content_hash, prepared.test_files).await?;

    let insert = FilterInsert {
        content_hash: &prepared.content_hash,
        command_pattern: &prepared.command_pattern,
        canonical_command: &prepared.canonical_command,
        author_id: auth.user_id,
        r2_key: &r2_key,
    };
    let (author, is_new) = upsert_filter_record(&state, &insert, &auth.username).await?;

    if is_new {
        insert_filter_tests(&state.db, &prepared.content_hash, &test_r2_keys).await?;
    }

    let registry_url = format!("{}/filters/{}", state.public_url, prepared.content_hash);
    let status = if is_new {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((
        status,
        Json(PublishFilterResponse {
            content_hash: prepared.content_hash,
            command_pattern: prepared.command_pattern,
            author,
            registry_url,
        }),
    ))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
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
        MIT_ACCEPT, insert_test_user, make_multipart, make_state, make_state_with_storage,
        post_filter,
    };

    const VALID_FILTER_TOML: &[u8] = b"command = \"my-tool\"\n";

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn publish_filter_creates_record(pool: PgPool) {
        let (_, token) = insert_test_user(&pool, "alice_pub").await;
        let app = crate::routes::create_router(make_state(pool.clone()));

        let resp = post_filter(app, &token, &[("filter", VALID_FILTER_TOML), MIT_ACCEPT]).await;

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
        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));

        let resp = post_filter(
            app,
            &token,
            &[
                ("filter", VALID_FILTER_TOML),
                MIT_ACCEPT,
                (
                    "test:basic.toml",
                    b"name = \"basic\"\n\n[[expect]]\ncontains = \"ok\"\n",
                ),
                (
                    "test:advanced.toml",
                    b"name = \"advanced\"\n\n[[expect]]\ncontains = \"ok\"\n",
                ),
            ],
        )
        .await;

        assert_eq!(resp.status(), StatusCode::CREATED);
        let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        let hash = json["content_hash"].as_str().unwrap();

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM filter_tests WHERE filter_hash = $1")
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

        let app = crate::routes::create_router(make_state(pool.clone()));
        let resp = post_filter(app, &token, &[("filter", VALID_FILTER_TOML), MIT_ACCEPT]).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let app = crate::routes::create_router(make_state(pool.clone()));
        let resp = post_filter(app, &token, &[("filter", VALID_FILTER_TOML), MIT_ACCEPT]).await;
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
        let resp = post_filter(app, &token, &[("filter", toml), MIT_ACCEPT]).await;
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
        let resp = post_filter(app, &token, &[("filter", VALID_FILTER_TOML), MIT_ACCEPT]).await;
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "first publish should succeed"
        );

        // Upload a different filter (different content) to avoid the 200-OK deduplicate path
        let app = crate::routes::create_router(state);
        let different_toml = b"command = \"other-tool\"\n";
        let resp = post_filter(app, &token, &[("filter", different_toml), MIT_ACCEPT]).await;
        assert_eq!(
            resp.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "second publish should be rate limited"
        );
    }
}
