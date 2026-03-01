// Not all test binaries use every harness method — each test file compiles
// the harness independently, so some items appear unused per-binary.
#![allow(
    dead_code,
    unused_imports,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::missing_panics_doc
)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use sqlx::PgPool;
use tokio::task::JoinHandle;

use tokf::auth::credentials::LoadedAuth;
use tokf::remote::client::{MachineInfo, RegisteredMachine};
use tokf::remote::filter_client::{self, DownloadedFilter, FilterDetails, FilterSummary};
use tokf::remote::gain_client::{GainResponse, GlobalGainResponse};
use tokf::remote::http::Client;
use tokf::remote::machine::StoredMachine;
use tokf::remote::publish_client::{self, PublishResponse, UpdateTestsResponse};
use tokf::remote::sync_client::{self, SyncEvent, SyncRequest, SyncResponse};
use tokf::tracking;
use tokf_server::auth::github::GitHubClient;
use tokf_server::auth::mock::{NoOpGitHubClient, SuccessGitHubClient};
use tokf_server::routes::{create_router, test_helpers};
use tokf_server::storage::mock::InMemoryStorageClient;

/// Reusable test harness that spins up an in-process axum server
/// backed by a real `CockroachDB` pool and provides helpers for
/// CLI-level operations.
pub struct TestHarness {
    pub server_addr: SocketAddr,
    pub base_url: String,
    pub token: String,
    pub user_id: i64,
    pub machine_id: uuid::Uuid,
    pub sqlite_path: PathBuf,
    pub pool: PgPool,
    _temp_dir: tempfile::TempDir,
    server_handle: JoinHandle<()>,
}

impl Drop for TestHarness {
    fn drop(&mut self) {
        self.server_handle.abort();
    }
}

impl TestHarness {
    /// Create a harness with `NoOpGitHubClient` (auth routes won't complete the
    /// device flow, but pre-created tokens work fine for authenticated calls).
    pub async fn new(pool: PgPool) -> Self {
        Self::with_github(pool, Arc::new(NoOpGitHubClient)).await
    }

    /// Create a harness with `InMemoryStorageClient` so filter publish/download
    /// operations actually persist bytes in memory (unlike `NoOpStorageClient`).
    pub async fn with_storage(pool: PgPool) -> Self {
        Self::with_github_and_storage(
            pool,
            Arc::new(NoOpGitHubClient),
            Arc::new(InMemoryStorageClient::new()),
        )
        .await
    }

    /// Create a harness with `SuccessGitHubClient` (device flow completes
    /// immediately, useful for auth E2E tests).
    pub async fn with_github_mock(pool: PgPool) -> Self {
        Self::with_github(pool, Arc::new(SuccessGitHubClient)).await
    }

    /// Create a harness with a custom `AppState` mutation closure.
    /// Useful for overriding specific fields like rate limiters.
    pub async fn with_custom_state<F>(pool: PgPool, mutate: F) -> Self
    where
        F: FnOnce(&mut tokf_server::state::AppState),
    {
        Self::build(
            pool,
            Arc::new(NoOpGitHubClient),
            Arc::new(InMemoryStorageClient::new()),
            Some(mutate),
        )
        .await
    }

    async fn with_github(pool: PgPool, github: Arc<dyn GitHubClient>) -> Self {
        Self::build::<fn(&mut tokf_server::state::AppState)>(
            pool,
            github,
            Arc::new(tokf_server::storage::noop::NoOpStorageClient),
            None,
        )
        .await
    }

    async fn with_github_and_storage(
        pool: PgPool,
        github: Arc<dyn GitHubClient>,
        storage: Arc<dyn tokf_server::storage::StorageClient>,
    ) -> Self {
        Self::build::<fn(&mut tokf_server::state::AppState)>(pool, github, storage, None).await
    }

    /// Core builder: creates user/machine, builds state, starts server.
    async fn build<F>(
        pool: PgPool,
        github: Arc<dyn GitHubClient>,
        storage: Arc<dyn tokf_server::storage::StorageClient>,
        mutate: Option<F>,
    ) -> Self
    where
        F: FnOnce(&mut tokf_server::state::AppState),
    {
        let (user_id, token) = test_helpers::create_user_and_token(&pool).await;
        let machine_id = test_helpers::create_machine(&pool, user_id).await;

        let mut state = test_helpers::make_state(pool.clone());
        state.github = github;
        state.storage = storage;
        if let Some(f) = mutate {
            f(&mut state);
        }

        let (server_addr, base_url, server_handle) = Self::start_server(state).await;

        let temp_dir = tempfile::TempDir::new().unwrap();
        let sqlite_path = temp_dir.path().join("tracking.db");

        Self {
            server_addr,
            base_url,
            token,
            user_id,
            machine_id,
            sqlite_path,
            pool,
            _temp_dir: temp_dir,
            server_handle,
        }
    }

    /// Bind to a random port, spawn the axum server, and wait for readiness.
    async fn start_server(
        state: tokf_server::state::AppState,
    ) -> (SocketAddr, String, JoinHandle<()>) {
        let app = create_router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let handle = tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });

        let base_url = format!("http://{addr}");
        let client = reqwest::Client::new();
        let mut ready = false;
        for _ in 0..40 {
            if client
                .get(format!("{base_url}/health"))
                .send()
                .await
                .is_ok()
            {
                ready = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(ready, "server did not become ready within 200ms");

        (addr, base_url, handle)
    }

    /// Create a second user+token pair (for testing authorization).
    pub async fn create_other_user_token(&self) -> String {
        let (_, token) = test_helpers::create_user_and_token(&self.pool).await;
        token
    }

    /// Open (or create) the `SQLite` tracking database.
    pub fn open_tracking_db(&self) -> rusqlite::Connection {
        tracking::open_db(&self.sqlite_path).unwrap()
    }

    /// Record a tracking event in the local `SQLite` database.
    #[allow(clippy::unused_self, clippy::too_many_arguments)]
    pub fn record_event(
        &self,
        conn: &rusqlite::Connection,
        command: &str,
        filter_name: Option<&str>,
        filter_hash: Option<&str>,
        input_bytes: usize,
        output_bytes: usize,
    ) {
        let event = tracking::build_event(
            command,
            filter_name,
            filter_hash,
            input_bytes,
            output_bytes,
            0,
            0,
            false,
        );
        tracking::record_event(conn, &event).unwrap();
    }

    /// Construct a `LoadedAuth` pointing at this harness's server.
    pub fn loaded_auth(&self) -> LoadedAuth {
        LoadedAuth {
            token: self.token.clone(),
            username: "testuser".to_string(),
            server_url: self.base_url.clone(),
            // Far-future expiry (year ~2554) so the token is always valid.
            expires_at: 18_446_744_073,
            mit_license_accepted: None,
        }
    }

    /// Construct a `StoredMachine` with this harness's machine ID.
    pub fn stored_machine(&self) -> StoredMachine {
        StoredMachine {
            machine_id: self.machine_id.to_string(),
            hostname: "test-host".to_string(),
        }
    }

    /// Build a `Client` with this harness's base URL and token.
    pub fn client(&self) -> Client {
        Client::new(&self.base_url, Some(&self.token)).unwrap()
    }

    /// Build a `Client` with a custom token.
    pub fn client_with_token(&self, token: &str) -> Client {
        Client::new(&self.base_url, Some(token)).unwrap()
    }

    /// Build an unauthenticated `Client`.
    pub fn unauthenticated_client(&self) -> Client {
        Client::unauthenticated(&self.base_url).unwrap()
    }

    // ── Sync request builders ───────────────────────────────────

    /// Build a `SyncRequest` from local `SQLite` events using the harness's machine ID.
    pub fn build_sync_request(&self, conn: &rusqlite::Connection) -> SyncRequest {
        self.build_sync_request_for_machine(conn, &self.machine_id.to_string())
    }

    /// Build a `SyncRequest` from local `SQLite` events using a custom machine ID.
    #[allow(clippy::unused_self)]
    pub fn build_sync_request_for_machine(
        &self,
        conn: &rusqlite::Connection,
        machine_id: &str,
    ) -> SyncRequest {
        let last_id = tracking::get_last_synced_id(conn).unwrap();
        let events = tracking::get_events_since(conn, last_id).unwrap();
        let sync_events: Vec<SyncEvent> = events
            .iter()
            .map(|e| SyncEvent {
                id: e.id,
                filter_name: e.filter_name.clone(),
                filter_hash: e.filter_hash.clone(),
                input_tokens: e.input_tokens_est,
                output_tokens: e.output_tokens_est,
                command_count: 1,
                recorded_at: e.timestamp.clone(),
            })
            .collect();
        SyncRequest {
            machine_id: machine_id.to_string(),
            last_event_id: last_id,
            events: sync_events,
        }
    }

    // ── Blocking helpers (wrap spawn_blocking boilerplate) ───────

    /// Sync a pre-built request to the remote server.
    pub async fn blocking_sync_request(&self, req: &SyncRequest) -> SyncResponse {
        let req = req.clone();
        let client = self.client();
        tokio::task::spawn_blocking(move || sync_client::sync_events(&client, &req).unwrap())
            .await
            .unwrap()
    }

    /// Attempt sync and return the `Result` (for error-path tests).
    pub async fn try_sync_with_token(
        &self,
        req: &SyncRequest,
        token: &str,
    ) -> anyhow::Result<SyncResponse> {
        let req = req.clone();
        let client = self.client_with_token(token);
        tokio::task::spawn_blocking(move || sync_client::sync_events(&client, &req))
            .await
            .unwrap()
    }

    /// Fetch the authenticated user's gain summary.
    pub async fn blocking_gain(&self) -> GainResponse {
        let client = self.client();
        tokio::task::spawn_blocking(move || tokf::remote::gain_client::get_gain(&client).unwrap())
            .await
            .unwrap()
    }

    /// Fetch the global (unauthenticated) gain summary.
    pub async fn blocking_global_gain(&self) -> GlobalGainResponse {
        let client = self.unauthenticated_client();
        tokio::task::spawn_blocking(move || {
            tokf::remote::gain_client::get_global_gain(&client).unwrap()
        })
        .await
        .unwrap()
    }

    /// Register a machine via the CLI client function.
    pub async fn blocking_register_machine(
        &self,
        machine_id: &str,
        hostname: &str,
    ) -> RegisteredMachine {
        let client = self.client();
        let machine_id = machine_id.to_string();
        let hostname = hostname.to_string();
        tokio::task::spawn_blocking(move || {
            tokf::remote::client::register_machine(&client, &machine_id, &hostname).unwrap()
        })
        .await
        .unwrap()
    }

    /// List machines for the authenticated user.
    pub async fn blocking_list_machines(&self) -> Vec<MachineInfo> {
        let client = self.client();
        tokio::task::spawn_blocking(move || tokf::remote::client::list_machines(&client).unwrap())
            .await
            .unwrap()
    }

    /// Fetch gain using a specific token (for auth-flow tests).
    pub async fn blocking_gain_with_token(&self, token: &str) -> GainResponse {
        let client = self.client_with_token(token);
        tokio::task::spawn_blocking(move || tokf::remote::gain_client::get_gain(&client).unwrap())
            .await
            .unwrap()
    }

    // ── Filter helpers ──────────────────────────────────────────

    /// Try to publish a filter (returns Result for error-path tests).
    pub async fn try_publish(
        &self,
        filter_bytes: Vec<u8>,
        test_files: Vec<(String, Vec<u8>)>,
    ) -> anyhow::Result<(bool, PublishResponse)> {
        let client = self.client();
        tokio::task::spawn_blocking(move || {
            publish_client::publish_filter(&client, &filter_bytes, &test_files)
        })
        .await
        .unwrap()
    }

    /// Publish a filter with optional test files. Returns `(is_new, response)`.
    pub async fn blocking_publish(
        &self,
        filter_bytes: Vec<u8>,
        test_files: Vec<(String, Vec<u8>)>,
    ) -> (bool, PublishResponse) {
        let client = self.client();
        tokio::task::spawn_blocking(move || {
            publish_client::publish_filter(&client, &filter_bytes, &test_files).unwrap()
        })
        .await
        .unwrap()
    }

    /// Update the test suite for a published filter.
    pub async fn blocking_update_tests(
        &self,
        hash: &str,
        test_files: Vec<(String, Vec<u8>)>,
    ) -> UpdateTestsResponse {
        let client = self.client();
        let hash = hash.to_string();
        tokio::task::spawn_blocking(move || {
            publish_client::update_tests(&client, &hash, &test_files).unwrap()
        })
        .await
        .unwrap()
    }

    /// Run a fallible client call on a blocking thread with the harness credentials.
    async fn try_client_call<T, F>(&self, f: F) -> anyhow::Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Client) -> anyhow::Result<T> + Send + 'static,
    {
        let client = self.client();
        tokio::task::spawn_blocking(move || f(&client))
            .await
            .unwrap()
    }

    /// Try to update tests (returns Result for error-path tests).
    pub async fn try_update_tests(
        &self,
        hash: &str,
        test_files: Vec<(String, Vec<u8>)>,
    ) -> anyhow::Result<UpdateTestsResponse> {
        let hash = hash.to_string();
        self.try_client_call(move |client| publish_client::update_tests(client, &hash, &test_files))
            .await
    }

    /// Try to update tests with a custom token (for auth tests).
    pub async fn try_update_tests_with_token(
        &self,
        hash: &str,
        test_files: Vec<(String, Vec<u8>)>,
        token: &str,
    ) -> anyhow::Result<UpdateTestsResponse> {
        let client = self.client_with_token(token);
        let hash = hash.to_string();
        tokio::task::spawn_blocking(move || {
            publish_client::update_tests(&client, &hash, &test_files)
        })
        .await
        .unwrap()
    }

    /// Search the filter registry.
    pub async fn blocking_search_filters(&self, query: &str, limit: usize) -> Vec<FilterSummary> {
        let client = self.client();
        let query = query.to_string();
        tokio::task::spawn_blocking(move || {
            filter_client::search_filters(&client, &query, limit).unwrap()
        })
        .await
        .unwrap()
    }

    /// Try to search filters (returns Result for error-path tests).
    pub async fn try_search_filters(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<FilterSummary>> {
        let query = query.to_string();
        self.try_client_call(move |client| filter_client::search_filters(client, &query, limit))
            .await
    }

    /// Get filter details by hash.
    pub async fn blocking_get_filter(&self, hash: &str) -> FilterDetails {
        let client = self.client();
        let hash = hash.to_string();
        tokio::task::spawn_blocking(move || filter_client::get_filter(&client, &hash).unwrap())
            .await
            .unwrap()
    }

    /// Download filter TOML + test files by hash.
    pub async fn blocking_download_filter(&self, hash: &str) -> DownloadedFilter {
        let client = self.client();
        let hash = hash.to_string();
        tokio::task::spawn_blocking(move || filter_client::download_filter(&client, &hash).unwrap())
            .await
            .unwrap()
    }
}
