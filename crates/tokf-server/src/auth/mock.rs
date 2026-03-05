use crate::auth::github::{
    AccessTokenResponse, DeviceCodeResponse, GitHubClient, GitHubOrg, GitHubUser,
};

/// A mock `GitHubClient` for tests that don't exercise the auth routes.
/// All methods return reasonable defaults.
pub struct NoOpGitHubClient;

/// A mock `GitHubClient` that returns immediate success on all operations.
///
/// Useful for integration tests that need the full device flow to complete
/// without hitting real GitHub APIs.
#[cfg(any(test, feature = "test-helpers"))]
pub struct SuccessGitHubClient;

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

#[cfg(any(test, feature = "test-helpers"))]
#[async_trait::async_trait]
impl GitHubClient for SuccessGitHubClient {
    async fn request_device_code(&self, _client_id: &str) -> anyhow::Result<DeviceCodeResponse> {
        Ok(DeviceCodeResponse {
            device_code: format!("dc-{}", rand::random::<u32>()),
            user_code: "TEST-1234".to_string(),
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
        Ok(AccessTokenResponse::Success {
            access_token: "gho_test_token".to_string(),
            token_type: "bearer".to_string(),
            scope: "read:user,read:org".to_string(),
        })
    }

    async fn get_user(&self, _access_token: &str) -> anyhow::Result<GitHubUser> {
        Ok(GitHubUser {
            id: 12345,
            login: "testuser".to_string(),
            avatar_url: "https://avatars.githubusercontent.com/u/12345".to_string(),
            html_url: "https://github.com/testuser".to_string(),
        })
    }

    async fn get_user_orgs(&self, _access_token: &str) -> anyhow::Result<Vec<GitHubOrg>> {
        Ok(vec![GitHubOrg {
            login: "test-org".to_string(),
        }])
    }
}
