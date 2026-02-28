pub mod client;
pub mod filter_client;
pub mod gain_client;
pub mod http;
pub mod machine;
pub mod publish_client;
pub mod retry;
pub mod sync_client;

/// Structured error for HTTP 429 responses, allowing retry logic to branch
/// on the type rather than parsing error message strings.
#[derive(Debug)]
pub struct RateLimitedError {
    pub retry_after_secs: u64,
}

impl std::fmt::Display for RateLimitedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "rate limit exceeded — try again in {}s (HTTP 429)",
            self.retry_after_secs
        )
    }
}

impl std::error::Error for RateLimitedError {}

/// Consume a response and return it if the status is successful.
///
/// On 401 Unauthorized, returns a specific error prompting re-authentication.
/// On 429 Too Many Requests, returns a [`RateLimitedError`] with the parsed
/// `Retry-After` value (defaulting to 60 s).
/// On other non-2xx statuses, includes the response body in the error message.
///
/// # Errors
///
/// Returns an error for any non-2xx status code.
pub(crate) fn require_success(
    resp: reqwest::blocking::Response,
) -> anyhow::Result<reqwest::blocking::Response> {
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        anyhow::bail!(
            "server returned HTTP 401 Unauthorized — run `tokf auth login` to re-authenticate"
        );
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(60);
        return Err(RateLimitedError {
            retry_after_secs: retry_after,
        }
        .into());
    }
    if !status.is_success() {
        let text = resp
            .text()
            .map_err(|e| anyhow::anyhow!("could not read response body: {e}"))?;
        anyhow::bail!("server returned HTTP {status}: {text}");
    }
    Ok(resp)
}
