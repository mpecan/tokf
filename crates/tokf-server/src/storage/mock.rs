use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::StorageClient;

/// In-memory storage client for unit tests.
/// Tracks `put` and `delete` call counts for assertions.
#[allow(clippy::expect_used)]
pub struct InMemoryStorageClient {
    data: Mutex<HashMap<String, Vec<u8>>>,
    put_calls: AtomicUsize,
    delete_calls: AtomicUsize,
}

impl Default for InMemoryStorageClient {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryStorageClient {
    pub fn new() -> Self {
        Self {
            data: Mutex::new(HashMap::new()),
            put_calls: AtomicUsize::new(0),
            delete_calls: AtomicUsize::new(0),
        }
    }

    pub fn put_count(&self) -> usize {
        self.put_calls.load(Ordering::Relaxed)
    }

    pub fn delete_count(&self) -> usize {
        self.delete_calls.load(Ordering::Relaxed)
    }
}

#[allow(clippy::expect_used)]
#[async_trait::async_trait]
impl StorageClient for InMemoryStorageClient {
    async fn put(&self, key: &str, body: Vec<u8>) -> anyhow::Result<String> {
        self.put_calls.fetch_add(1, Ordering::Relaxed);
        self.data
            .lock()
            .expect("lock poisoned")
            .insert(key.to_string(), body);
        Ok(key.to_string())
    }

    async fn get(&self, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(self.data.lock().expect("lock poisoned").get(key).cloned())
    }

    async fn exists(&self, key: &str) -> anyhow::Result<bool> {
        Ok(self.data.lock().expect("lock poisoned").contains_key(key))
    }

    async fn delete(&self, key: &str) -> anyhow::Result<()> {
        self.delete_calls.fetch_add(1, Ordering::Relaxed);
        self.data.lock().expect("lock poisoned").remove(key);
        Ok(())
    }
}
