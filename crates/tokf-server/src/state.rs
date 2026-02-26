use std::sync::Arc;

use crate::auth::github::GitHubClient;
use crate::rate_limit::PublishRateLimiter;
use crate::storage::StorageClient;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::PgPool,
    pub github: Arc<dyn GitHubClient>,
    pub storage: Arc<dyn StorageClient>,
    pub github_client_id: String,
    pub github_client_secret: String,
    pub trust_proxy: bool,
    pub public_url: String,
    pub publish_rate_limiter: Arc<PublishRateLimiter>,
    /// Rate limiter for search/download endpoints (higher limit than publish).
    pub search_rate_limiter: Arc<PublishRateLimiter>,
}
