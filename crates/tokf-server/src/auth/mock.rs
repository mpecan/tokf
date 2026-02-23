use crate::auth::github::{
    AccessTokenResponse, DeviceCodeResponse, GitHubClient, GitHubOrg, GitHubUser,
};

/// A mock `GitHubClient` for tests that don't exercise the auth routes.
/// All methods return reasonable defaults.
pub struct NoOpGitHubClient;

#[async_trait::async_trait]
impl GitHubClient for NoOpGitHubClient {
    async fn request_device_code(&self, _client_id: &str) -> anyhow::Result<DeviceCodeResponse> {
        Ok(DeviceCodeResponse {
            device_code: "mock-dc".to_string(),
            user_code: "MOCK-1234".to_string(),
            verification_uri: "https://github.com/login/device".to_string(),
            expires_in: 900,
            interval: 5,
        })
    }

    async fn poll_access_token(
        &self,
        _client_id: &str,
        _client_secret: &str,
        _device_code: &str,
    ) -> anyhow::Result<AccessTokenResponse> {
        Ok(AccessTokenResponse::Pending {
            error: "authorization_pending".to_string(),
            error_description: None,
            interval: None,
        })
    }

    async fn get_user(&self, _access_token: &str) -> anyhow::Result<GitHubUser> {
        Ok(GitHubUser {
            id: 1,
            login: "mock-user".to_string(),
            avatar_url: "https://example.com/avatar.png".to_string(),
            html_url: "https://github.com/mock-user".to_string(),
        })
    }

    async fn get_user_orgs(&self, _access_token: &str) -> anyhow::Result<Vec<GitHubOrg>> {
        Ok(vec![])
    }
}
