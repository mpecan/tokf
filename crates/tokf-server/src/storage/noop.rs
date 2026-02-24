use super::StorageClient;

/// No-op storage client for when R2 is not configured.
/// All writes are silently discarded; reads always return `None`.
pub struct NoOpStorageClient;

#[async_trait::async_trait]
impl StorageClient for NoOpStorageClient {
    async fn put(&self, key: &str, _body: Vec<u8>) -> anyhow::Result<String> {
        Ok(key.to_string())
    }

    async fn get(&self, _key: &str) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(None)
    }

    async fn exists(&self, _key: &str) -> anyhow::Result<bool> {
        Ok(false)
    }
}
