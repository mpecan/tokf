use serde::{Deserialize, Serialize};

// ── Response types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: i64,
    pub interval: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum AccessTokenResponse {
    Success {
        access_token: String,
        token_type: String,
        scope: String,
    },
    Pending {
        error: String,
        error_description: Option<String>,
        interval: Option<i64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubUser {
    pub id: i64,
    pub login: String,
    pub avatar_url: String,
    pub html_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubOrg {
    pub login: String,
}

// ── Trait ────────────────────────────────────────────────────────────────────

#[async_trait::async_trait]
pub trait GitHubClient: Send + Sync {
    async fn request_device_code(&self, client_id: &str) -> anyhow::Result<DeviceCodeResponse>;

    async fn poll_access_token(
        &self,
        client_id: &str,
        client_secret: &str,
        device_code: &str,
    ) -> anyhow::Result<AccessTokenResponse>;

    async fn get_user(&self, access_token: &str) -> anyhow::Result<GitHubUser>;

    async fn get_user_orgs(&self, access_token: &str) -> anyhow::Result<Vec<GitHubOrg>>;
}

// ── Real implementation ─────────────────────────────────────────────────────

pub struct RealGitHubClient {
    http: reqwest::Client,
}

impl RealGitHubClient {
    /// Creates a new GitHub API client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be built.
    pub fn new() -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent("tokf-server")
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        Ok(Self { http })
    }

    async fn authed_get<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        access_token: &str,
    ) -> anyhow::Result<T> {
        Ok(self
            .http
            .get(url)
            .header("Accept", "application/json")
            .bearer_auth(access_token)
            .send()
            .await?
            .error_for_status()?
            .json::<T>()
            .await?)
    }
}

#[async_trait::async_trait]
impl GitHubClient for RealGitHubClient {
    async fn request_device_code(&self, client_id: &str) -> anyhow::Result<DeviceCodeResponse> {
        let resp = self
            .http
            .post("https://github.com/login/device/code")
            .header("Accept", "application/json")
            .form(&[("client_id", client_id), ("scope", "read:user,read:org")])
            .send()
            .await?
            .error_for_status()?
            .json::<DeviceCodeResponse>()
            .await?;
        Ok(resp)
    }

    async fn poll_access_token(
        &self,
        client_id: &str,
        client_secret: &str,
        device_code: &str,
    ) -> anyhow::Result<AccessTokenResponse> {
        let resp = self
            .http
            .post("https://github.com/login/oauth/access_token")
            .header("Accept", "application/json")
            .form(&[
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("device_code", device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await?
            .error_for_status()?
            .json::<AccessTokenResponse>()
            .await?;
        Ok(resp)
    }

    async fn get_user(&self, access_token: &str) -> anyhow::Result<GitHubUser> {
        self.authed_get("https://api.github.com/user", access_token)
            .await
    }

    async fn get_user_orgs(&self, access_token: &str) -> anyhow::Result<Vec<GitHubOrg>> {
        self.authed_get("https://api.github.com/user/orgs", access_token)
            .await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn deserializes_device_code_response() {
        let json = r#"{
            "device_code": "dc-123",
            "user_code": "ABCD-1234",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900,
            "interval": 5
        }"#;
        let resp: DeviceCodeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.device_code, "dc-123");
        assert_eq!(resp.user_code, "ABCD-1234");
        assert_eq!(resp.interval, 5);
    }

    #[test]
    fn deserializes_access_token_success() {
        let json = r#"{
            "access_token": "gho_abc123",
            "token_type": "bearer",
            "scope": "read:user,read:org"
        }"#;
        let resp: AccessTokenResponse = serde_json::from_str(json).unwrap();
        assert!(matches!(resp, AccessTokenResponse::Success { .. }));
    }

    #[test]
    fn deserializes_access_token_pending() {
        let json = r#"{
            "error": "authorization_pending",
            "error_description": "The authorization request is still pending."
        }"#;
        let resp: AccessTokenResponse = serde_json::from_str(json).unwrap();
        match resp {
            AccessTokenResponse::Pending { error, .. } => {
                assert_eq!(error, "authorization_pending");
            }
            AccessTokenResponse::Success { .. } => panic!("expected Pending variant"),
        }
    }

    #[test]
    fn deserializes_access_token_slow_down() {
        let json = r#"{
            "error": "slow_down",
            "error_description": "Too many requests.",
            "interval": 10
        }"#;
        let resp: AccessTokenResponse = serde_json::from_str(json).unwrap();
        match resp {
            AccessTokenResponse::Pending {
                error, interval, ..
            } => {
                assert_eq!(error, "slow_down");
                assert_eq!(interval, Some(10));
            }
            AccessTokenResponse::Success { .. } => panic!("expected Pending variant"),
        }
    }

    #[test]
    fn deserializes_github_user() {
        let json = r#"{
            "id": 42,
            "login": "octocat",
            "avatar_url": "https://avatars.githubusercontent.com/u/42",
            "html_url": "https://github.com/octocat"
        }"#;
        let user: GitHubUser = serde_json::from_str(json).unwrap();
        assert_eq!(user.id, 42);
        assert_eq!(user.login, "octocat");
    }

    #[test]
    fn deserializes_github_orgs() {
        let json = r#"[{"login": "org-a"}, {"login": "org-b"}]"#;
        let orgs: Vec<GitHubOrg> = serde_json::from_str(json).unwrap();
        assert_eq!(orgs.len(), 2);
        assert_eq!(orgs[0].login, "org-a");
    }
}
