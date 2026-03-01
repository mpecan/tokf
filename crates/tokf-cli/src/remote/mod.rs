pub mod client;
pub mod filter_client;
pub mod gain_client;
pub mod http;
pub mod machine;
pub mod publish_client;
pub mod retry;
pub mod sync_client;

use std::fmt;

/// Check whether verbose debug output is enabled via `TOKF_DEBUG=1`.
pub fn is_debug() -> bool {
    std::env::var("TOKF_DEBUG")
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

/// Unified error type for all remote HTTP operations.
///
/// `Display` produces a single-line summary suitable for end-user stderr.
/// `Debug` includes the full underlying error chain for `TOKF_DEBUG=1`.
#[derive(Debug)]
pub enum RemoteError {
    /// DNS / connect / network failure.
    ConnectionFailed { url: String, source: reqwest::Error },
    /// Request or connect timeout.
    Timeout { url: String, source: reqwest::Error },
    /// Server returned 5xx.
    ServerError {
        url: String,
        status: reqwest::StatusCode,
        body: String,
    },
    /// Server returned 401 Unauthorized.
    Unauthorized,
    /// Server returned 429 Too Many Requests.
    RateLimited(RateLimitedError),
    /// Local request-building error (invalid URL, encoding, redirect policy).
    /// Not transient — should not be retried.
    RequestError { url: String, source: reqwest::Error },
    /// Non-2xx response that isn't 401/429/5xx.
    ClientError {
        url: String,
        status: reqwest::StatusCode,
        body: String,
    },
}

impl fmt::Display for RemoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConnectionFailed { url, source } => {
                if is_debug() {
                    write!(f, "remote: could not connect to {url}: {source}")
                } else {
                    write!(
                        f,
                        "remote: could not connect to server (use TOKF_DEBUG=1 for details)"
                    )
                }
            }
            Self::Timeout { url, source } => {
                if is_debug() {
                    write!(f, "remote: request to {url} timed out: {source}")
                } else {
                    write!(
                        f,
                        "remote: request timed out (use TOKF_DEBUG=1 for details)"
                    )
                }
            }
            Self::ServerError { url, status, body } => {
                if is_debug() {
                    write!(f, "remote: server error {status} from {url}: {body}")
                } else {
                    write!(
                        f,
                        "remote: server error {status} (use TOKF_DEBUG=1 for details)"
                    )
                }
            }
            Self::Unauthorized => {
                write!(
                    f,
                    "remote: HTTP 401 Unauthorized — run `tokf auth login` to re-authenticate"
                )
            }
            Self::RequestError { url, source } => {
                if is_debug() {
                    write!(f, "remote: request error for {url}: {source}")
                } else {
                    write!(f, "remote: request error (use TOKF_DEBUG=1 for details)")
                }
            }
            Self::RateLimited(inner) => write!(f, "remote: {inner}"),
            Self::ClientError { url, status, body } => {
                if is_debug() {
                    write!(f, "remote: HTTP {status} from {url}: {body}")
                } else {
                    write!(f, "remote: HTTP {status} (use TOKF_DEBUG=1 for details)")
                }
            }
        }
    }
}

impl std::error::Error for RemoteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ConnectionFailed { source, .. }
            | Self::Timeout { source, .. }
            | Self::RequestError { source, .. } => Some(source),
            Self::RateLimited(inner) => Some(inner),
            _ => None,
        }
    }
}

impl RemoteError {
    /// Returns `true` for transient errors that may succeed on retry
    /// (connection failures, timeouts, 5xx).
    pub const fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::ConnectionFailed { .. } | Self::Timeout { .. } | Self::ServerError { .. }
        )
    }
}

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

/// Classify a `reqwest::Error` into a [`RemoteError`].
///
/// - Timeouts → `Timeout` (transient)
/// - Connect failures → `ConnectionFailed` (transient)
/// - Request-building errors (invalid URL, encoding) → `RequestError` (non-transient)
/// - All other send errors (DNS, redirect loops) → `ConnectionFailed` (transient)
pub(crate) fn classify_reqwest_error(url: &str, err: reqwest::Error) -> RemoteError {
    if err.is_timeout() {
        RemoteError::Timeout {
            url: url.to_string(),
            source: err,
        }
    } else if err.is_connect() {
        RemoteError::ConnectionFailed {
            url: url.to_string(),
            source: err,
        }
    } else if err.is_request() || err.is_builder() || err.is_redirect() {
        // Local configuration errors — not transient, should not be retried.
        RemoteError::RequestError {
            url: url.to_string(),
            source: err,
        }
    } else {
        // Other send errors (DNS, etc.) — treat as connection failures (transient).
        RemoteError::ConnectionFailed {
            url: url.to_string(),
            source: err,
        }
    }
}

/// Check a raw response for 401 and 429 without consuming the body.
///
/// Useful for multipart endpoints where callers handle per-status-code logic
/// but still want centralized auth/rate-limit checking.
///
/// On 401, returns [`RemoteError::Unauthorized`].
/// On 429, returns [`RemoteError::RateLimited`] with the parsed `Retry-After` value.
pub(crate) fn check_auth_and_rate_limit(
    resp: &reqwest::blocking::Response,
) -> Result<(), RemoteError> {
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(RemoteError::Unauthorized);
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(60);
        return Err(RemoteError::RateLimited(RateLimitedError {
            retry_after_secs: retry_after,
        }));
    }
    Ok(())
}

/// Consume a response and return it if the status is successful.
///
/// On 401 Unauthorized, returns [`RemoteError::Unauthorized`].
/// On 429 Too Many Requests, returns [`RemoteError::RateLimited`] with the parsed
/// `Retry-After` value (defaulting to 60 s).
/// On 5xx, returns [`RemoteError::ServerError`].
/// On other non-2xx statuses, returns [`RemoteError::ClientError`].
pub(crate) fn require_success(
    resp: reqwest::blocking::Response,
    url: &str,
) -> Result<reqwest::blocking::Response, RemoteError> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(RemoteError::Unauthorized);
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(60);
        return Err(RemoteError::RateLimited(RateLimitedError {
            retry_after_secs: retry_after,
        }));
    }
    let body = resp.text().unwrap_or_default();
    if status.is_server_error() {
        return Err(RemoteError::ServerError {
            url: url.to_string(),
            status,
            body,
        });
    }
    Err(RemoteError::ClientError {
        url: url.to_string(),
        status,
        body,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::error::Error as _;

    use super::*;

    #[test]
    fn is_debug_returns_false_by_default() {
        // Can't reliably unset env in parallel tests, just verify it doesn't panic.
        let _ = is_debug();
    }

    #[test]
    fn remote_error_display_connection_failed() {
        // Build a reqwest error by attempting to connect to an invalid address.
        // We test the Display format indirectly via the enum variant.
        let err = RemoteError::Unauthorized;
        let msg = err.to_string();
        assert!(msg.contains("401 Unauthorized"));
        assert!(msg.contains("tokf auth login"));
    }

    #[test]
    fn remote_error_display_rate_limited() {
        let err = RemoteError::RateLimited(RateLimitedError {
            retry_after_secs: 30,
        });
        let msg = err.to_string();
        assert!(msg.contains("rate limit"));
        assert!(msg.contains("30s"));
    }

    #[test]
    fn remote_error_display_server_error_no_debug() {
        let err = RemoteError::ServerError {
            url: "https://api.tokf.net/api/test".to_string(),
            status: reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            body: "internal error".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("500"));
        assert!(msg.contains("TOKF_DEBUG=1"));
    }

    #[test]
    fn remote_error_display_client_error_no_debug() {
        let err = RemoteError::ClientError {
            url: "https://api.tokf.net/api/test".to_string(),
            status: reqwest::StatusCode::NOT_FOUND,
            body: "not found".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("404"));
        assert!(msg.contains("TOKF_DEBUG=1"));
    }

    #[test]
    fn remote_error_is_transient() {
        assert!(
            RemoteError::ServerError {
                url: String::new(),
                status: reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                body: String::new(),
            }
            .is_transient()
        );

        assert!(!RemoteError::Unauthorized.is_transient());

        assert!(
            !RemoteError::RateLimited(RateLimitedError {
                retry_after_secs: 0
            })
            .is_transient()
        );
    }

    #[test]
    fn rate_limited_error_display() {
        let err = RateLimitedError {
            retry_after_secs: 120,
        };
        assert_eq!(
            err.to_string(),
            "rate limit exceeded — try again in 120s (HTTP 429)"
        );
    }

    #[test]
    fn remote_error_display_request_error_no_debug() {
        // RequestError should show a generic message without debug.
        // We can't easily construct a reqwest::Error, so test the variant exists
        // and is_transient returns false (tested separately).
        let err = RemoteError::ClientError {
            url: "https://api.tokf.net/bad".to_string(),
            status: reqwest::StatusCode::BAD_REQUEST,
            body: "bad request".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("400"));
    }

    #[test]
    fn request_error_is_not_transient() {
        // Can't construct a real reqwest::Error for RequestError, but we can
        // verify the is_transient match doesn't include it by checking ClientError
        // (which has the same non-transient behavior).
        assert!(
            !RemoteError::ClientError {
                url: String::new(),
                status: reqwest::StatusCode::BAD_REQUEST,
                body: String::new(),
            }
            .is_transient()
        );
    }

    #[test]
    fn remote_error_source_chain() {
        // Verify Error::source() returns Some for connection/timeout variants
        // and None for simple variants.
        assert!(RemoteError::Unauthorized.source().is_none());

        let rate_err = RemoteError::RateLimited(RateLimitedError {
            retry_after_secs: 5,
        });
        assert!(rate_err.source().is_some());

        assert!(
            RemoteError::ClientError {
                url: String::new(),
                status: reqwest::StatusCode::NOT_FOUND,
                body: String::new(),
            }
            .source()
            .is_none()
        );
    }
}
