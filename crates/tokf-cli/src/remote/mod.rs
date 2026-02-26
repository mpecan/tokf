pub mod client;
pub mod filter_client;
pub mod http;
pub mod machine;
pub mod publish_client;

/// Consume a response and return it if the status is successful.
///
/// On 401 Unauthorized, returns a specific error prompting re-authentication.
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
    if !status.is_success() {
        let text = resp
            .text()
            .map_err(|e| anyhow::anyhow!("could not read response body: {e}"))?;
        anyhow::bail!("server returned HTTP {status}: {text}");
    }
    Ok(resp)
}
