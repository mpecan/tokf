use crate::auth::credentials;

/// Request timeout for lightweight operations (search, status queries).
pub const LIGHT_TIMEOUT_SECS: u64 = 10;
/// Request timeout for heavy operations (downloads, uploads).
pub const HEAVY_TIMEOUT_SECS: u64 = 30;
const CONNECT_TIMEOUT_SECS: u64 = 5;

/// Load stored auth credentials and validate they are not expired.
///
/// # Errors
///
/// Returns an error if credentials are absent or the token has expired.
pub fn load_auth() -> anyhow::Result<credentials::LoadedAuth> {
    let auth = credentials::load()
        .ok_or_else(|| anyhow::anyhow!("not logged in — run `tokf auth login` first"))?;
    if auth.is_expired() {
        anyhow::bail!("token has expired — run `tokf auth login` to re-authenticate");
    }
    Ok(auth)
}

/// Build a blocking HTTP client with the given request timeout.
///
/// # Errors
///
/// Returns an error if the client cannot be constructed (e.g., invalid TLS config).
pub fn build_client(timeout_secs: u64) -> anyhow::Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .connect_timeout(std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS))
        .build()
        .map_err(|e| anyhow::anyhow!("could not build HTTP client: {e}"))
}

/// Perform an authenticated GET and deserialize the JSON response.
///
/// # Errors
///
/// Returns an error if the server is unreachable, returns a non-success
/// status, or the response body cannot be deserialized.
pub fn authed_get<T: serde::de::DeserializeOwned>(
    client: &reqwest::blocking::Client,
    url: &str,
    token: &str,
) -> anyhow::Result<T> {
    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .map_err(|e| anyhow::anyhow!("could not reach {url}: {e}"))?;
    let resp = super::require_success(resp)?;
    resp.json::<T>()
        .map_err(|e| anyhow::anyhow!("invalid response from server: {e}"))
}
