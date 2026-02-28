use std::thread;
use std::time::Duration;

/// Maximum number of retry attempts after a 429 response.
const MAX_RETRIES: u32 = 3;

/// Minimum backoff between retries (seconds).
const BASE_BACKOFF_SECS: u64 = 1;

/// Execute `f` with exponential backoff and jitter on HTTP 429 responses.
///
/// Retries up to 3 times with delays of 1 s, 2 s, 4 s plus random jitter
/// (0-500 ms) to prevent thundering-herd effects when many clients retry
/// simultaneously. When the server provides a `Retry-After` value, the
/// actual delay is `max(computed_backoff, retry_after)` (still with jitter).
///
/// Returns the error from the final attempt if all retries are exhausted, or
/// any non-429 error immediately.
///
/// # Errors
///
/// Propagates the final error if all attempts fail or the first non-429 error.
pub fn with_retry<T, F>(operation: &str, mut f: F) -> anyhow::Result<T>
where
    F: FnMut() -> anyhow::Result<T>,
{
    let mut attempt = 0u32;
    loop {
        match f() {
            Ok(val) => return Ok(val),
            Err(e) => {
                if !is_rate_limited(&e) || attempt >= MAX_RETRIES {
                    return Err(e);
                }
                let computed = BASE_BACKOFF_SECS << attempt;
                let backoff = parse_retry_after(&e).map_or(computed, |ra| ra.max(computed));
                let jitter_ms = jitter();
                attempt += 1;
                eprintln!(
                    "[tokf] {operation}: rate limited, retrying in {backoff}s ({attempt}/{MAX_RETRIES})"
                );
                thread::sleep(Duration::from_secs(backoff) + Duration::from_millis(jitter_ms));
            }
        }
    }
}

/// Return a pseudo-random jitter in the range 0–499 ms to prevent thundering
/// herd effects when multiple clients retry at the same time.
fn jitter() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    u64::from(nanos % 500)
}

/// Check if the error is a [`RateLimitedError`] via downcast.
fn is_rate_limited(err: &anyhow::Error) -> bool {
    err.downcast_ref::<super::RateLimitedError>().is_some()
}

/// Extract the `retry_after_secs` from a [`RateLimitedError`] via downcast.
fn parse_retry_after(err: &anyhow::Error) -> Option<u64> {
    err.downcast_ref::<super::RateLimitedError>()
        .map(|e| e.retry_after_secs)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::remote::RateLimitedError;
    use std::cell::Cell;

    /// Helper: build a rate-limited `anyhow::Error` with the given retry-after.
    fn rate_limited_err(retry_after_secs: u64) -> anyhow::Error {
        RateLimitedError { retry_after_secs }.into()
    }

    #[test]
    fn returns_success_on_first_try() {
        let result = with_retry("test", || Ok(42));
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn does_not_retry_non_429_errors() {
        let calls = Cell::new(0u32);
        let result: anyhow::Result<()> = with_retry("test", || {
            calls.set(calls.get() + 1);
            anyhow::bail!("server returned HTTP 500: internal error")
        });
        assert!(result.is_err());
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn retries_on_429_up_to_max() {
        let calls = Cell::new(0u32);
        let result: anyhow::Result<()> = with_retry("test", || {
            calls.set(calls.get() + 1);
            Err(rate_limited_err(0))
        });
        assert!(result.is_err());
        // 1 initial + 3 retries = 4 total
        assert_eq!(calls.get(), 4);
    }

    #[test]
    fn succeeds_after_retry() {
        let calls = Cell::new(0u32);
        let result = with_retry("test", || {
            calls.set(calls.get() + 1);
            if calls.get() < 2 {
                return Err(rate_limited_err(0));
            }
            Ok("success")
        });
        assert_eq!(result.unwrap(), "success");
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn parses_retry_after_from_structured_error() {
        let err = rate_limited_err(120);
        assert_eq!(parse_retry_after(&err), Some(120));
    }

    #[test]
    fn returns_none_for_non_rate_limit_error() {
        let err = anyhow::anyhow!("some other error");
        assert_eq!(parse_retry_after(&err), None);
    }

    #[test]
    fn backoff_uses_max_of_computed_and_server_value() {
        // Server says 0s, computed is 1s → should use 1s (computed)
        let err = rate_limited_err(0);
        let computed = BASE_BACKOFF_SECS; // attempt 0 → 1s
        let backoff = parse_retry_after(&err).map_or(computed, |ra| ra.max(computed));
        assert_eq!(backoff, 1);

        // Server says 10s, computed is 1s → should use 10s (server)
        let err = rate_limited_err(10);
        let backoff = parse_retry_after(&err).map_or(computed, |ra| ra.max(computed));
        assert_eq!(backoff, 10);
    }
}
