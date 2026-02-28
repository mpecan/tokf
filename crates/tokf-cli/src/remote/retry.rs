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
/// simultaneously. If the server's `Retry-After` value is larger than the
/// computed backoff, it is used instead (still with jitter).
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
                let backoff = parse_retry_after(&e).unwrap_or(BASE_BACKOFF_SECS << attempt);
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

fn is_rate_limited(err: &anyhow::Error) -> bool {
    err.to_string().contains("HTTP 429")
}

fn parse_retry_after(err: &anyhow::Error) -> Option<u64> {
    let msg = err.to_string();
    // Format: "rate limit exceeded — try again in Ns (HTTP 429)"
    msg.split("try again in ")
        .nth(1)?
        .split('s')
        .next()?
        .parse()
        .ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::cell::Cell;

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
            anyhow::bail!("rate limit exceeded — try again in 0s (HTTP 429)")
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
                anyhow::bail!("rate limit exceeded — try again in 0s (HTTP 429)")
            }
            Ok("success")
        });
        assert_eq!(result.unwrap(), "success");
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn parses_retry_after_from_error() {
        let err = anyhow::anyhow!("rate limit exceeded — try again in 120s (HTTP 429)");
        assert_eq!(parse_retry_after(&err), Some(120));
    }

    #[test]
    fn returns_none_for_unparseable_retry_after() {
        let err = anyhow::anyhow!("some other error");
        assert_eq!(parse_retry_after(&err), None);
    }
}
