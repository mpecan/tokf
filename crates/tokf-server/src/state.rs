use std::sync::Arc;

use crate::auth::github::GitHubClient;
use crate::storage::StorageClient;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::PgPool,
    pub github: Arc<dyn GitHubClient>,
    pub storage: Arc<dyn StorageClient>,
    pub github_client_id: String,
    pub github_client_secret: String,
    pub trust_proxy: bool,
}
