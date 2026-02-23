pub mod mock;
pub mod noop;
pub mod r2;

/// Abstraction over blob storage (R2 / S3-compatible).
#[async_trait::async_trait]
pub trait StorageClient: Send + Sync {
    /// Upload bytes to the given key. Returns the key on success.
    async fn put(&self, key: &str, body: Vec<u8>) -> anyhow::Result<String>;

    /// Download bytes by key. Returns `None` if the key doesn't exist.
    async fn get(&self, key: &str) -> anyhow::Result<Option<Vec<u8>>>;

    /// Check whether a key exists.
    async fn exists(&self, key: &str) -> anyhow::Result<bool>;
}

/// Upload a filter TOML if not already stored. Returns the R2 key.
///
/// Key format: `filters/{content_hash}/filter.toml`
///
/// # Errors
///
/// Returns an error if the storage call fails.
pub async fn upload_filter(
    storage: &dyn StorageClient,
    content_hash: &str,
    filter_bytes: Vec<u8>,
) -> anyhow::Result<String> {
    let key = format!("filters/{content_hash}/filter.toml");
    if storage.exists(&key).await? {
        return Ok(key);
    }
    storage.put(&key, filter_bytes).await
}

/// Upload a test file if not already stored. Returns the R2 key.
///
/// Key format: `filters/{content_hash}/tests/{filename}`
///
/// # Errors
///
/// Returns an error if the filename contains path traversal characters or if
/// the storage call fails.
pub async fn upload_test(
    storage: &dyn StorageClient,
    content_hash: &str,
    filename: &str,
    test_bytes: Vec<u8>,
) -> anyhow::Result<String> {
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        anyhow::bail!("invalid test filename: {filename:?}");
    }
    let key = format!("filters/{content_hash}/tests/{filename}");
    if storage.exists(&key).await? {
        return Ok(key);
    }
    storage.put(&key, test_bytes).await
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use mock::InMemoryStorageClient;

    #[tokio::test]
    async fn upload_filter_creates_correct_key() {
        let storage = InMemoryStorageClient::new();
        let key = upload_filter(&storage, "abc123", b"[filter]\nname = \"test\"".to_vec())
            .await
            .unwrap();
        assert_eq!(key, "filters/abc123/filter.toml");
    }

    #[tokio::test]
    async fn upload_filter_dedup_skips_second_put() {
        let storage = InMemoryStorageClient::new();
        let content = b"[filter]\nname = \"test\"".to_vec();

        upload_filter(&storage, "abc123", content.clone())
            .await
            .unwrap();
        assert_eq!(storage.put_count(), 1);

        // Second upload with same hash should skip put
        upload_filter(&storage, "abc123", content).await.unwrap();
        assert_eq!(storage.put_count(), 1, "should not call put again");
    }

    #[tokio::test]
    async fn upload_test_creates_correct_key() {
        let storage = InMemoryStorageClient::new();
        let key = upload_test(&storage, "abc123", "basic.toml", b"test content".to_vec())
            .await
            .unwrap();
        assert_eq!(key, "filters/abc123/tests/basic.toml");
    }

    #[tokio::test]
    async fn upload_test_dedup_skips_second_put() {
        let storage = InMemoryStorageClient::new();

        upload_test(&storage, "abc123", "basic.toml", b"test content".to_vec())
            .await
            .unwrap();
        assert_eq!(storage.put_count(), 1);

        upload_test(&storage, "abc123", "basic.toml", b"test content".to_vec())
            .await
            .unwrap();
        assert_eq!(storage.put_count(), 1, "should not call put again");
    }

    #[tokio::test]
    async fn get_returns_uploaded_bytes() {
        let storage = InMemoryStorageClient::new();
        let content = b"hello world".to_vec();
        storage.put("test/key", content.clone()).await.unwrap();

        let retrieved = storage.get("test/key").await.unwrap();
        assert_eq!(retrieved, Some(content));
    }

    #[tokio::test]
    async fn get_returns_none_for_missing_key() {
        let storage = InMemoryStorageClient::new();
        let retrieved = storage.get("nonexistent").await.unwrap();
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn exists_returns_correct_results() {
        let storage = InMemoryStorageClient::new();
        assert!(!storage.exists("key").await.unwrap());
        storage.put("key", b"data".to_vec()).await.unwrap();
        assert!(storage.exists("key").await.unwrap());
    }

    #[tokio::test]
    async fn upload_test_rejects_path_traversal() {
        let storage = InMemoryStorageClient::new();
        let result = upload_test(&storage, "abc", "../evil.toml", b"x".to_vec()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn upload_test_rejects_slash_in_filename() {
        let storage = InMemoryStorageClient::new();
        let result = upload_test(&storage, "abc", "sub/file.toml", b"x".to_vec()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn upload_test_rejects_backslash_in_filename() {
        let storage = InMemoryStorageClient::new();
        let result = upload_test(&storage, "abc", "sub\\file.toml", b"x".to_vec()).await;
        assert!(result.is_err());
    }
}
