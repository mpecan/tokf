use std::net::SocketAddr;

use axum::extract::ConnectInfo;
use axum::http::HeaderMap;
use axum::http::request::Parts;

use crate::rate_limit::RateLimitResult;

/// Axum extractor that resolves the TCP peer IP from `ConnectInfo<SocketAddr>`.
///
/// Returns `None` when `ConnectInfo` is unavailable (e.g. in tests that use
/// `Router::oneshot` without `into_make_service_with_connect_info`).
pub struct PeerIp(pub Option<String>);

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for PeerIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let ip = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ci| ci.0.ip().to_string());
        Ok(Self(ip))
    }
}

/// Extract the client IP address from request headers and/or peer address.
///
/// When `trust_proxy` is true, uses the first entry in the `X-Forwarded-For`
/// header. Falls back to `peer_ip` (the TCP socket address) when the header is
/// absent, or to `"unknown"` when no peer address is available.
///
/// # Security
///
/// `X-Forwarded-For` is trivially spoofable by clients. Setting
/// `trust_proxy = true` is only safe when the server sits behind a trusted
/// reverse proxy (e.g. Cloudflare, AWS ALB, nginx) that overwrites the header
/// with the real client IP. In direct-exposure deployments, leave
/// `trust_proxy = false` — the function will use the TCP socket address which
/// cannot be spoofed.
pub fn extract_ip(headers: &HeaderMap, trust_proxy: bool, peer_ip: Option<&str>) -> String {
    if trust_proxy
        && let Some(ip) = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').next())
            .map(|s| s.trim().to_string())
    {
        return ip;
    }
    peer_ip.map_or_else(|| "unknown".to_string(), ToString::to_string)
}

/// Pick the most restrictive of two rate-limit results (fewest remaining calls).
pub const fn most_restrictive(a: RateLimitResult, b: RateLimitResult) -> RateLimitResult {
    if a.remaining <= b.remaining { a } else { b }
}

/// Build `X-RateLimit-*` response headers from a [`RateLimitResult`].
///
/// **Information disclosure note:** these headers reveal the configured limit,
/// current remaining quota, and window reset time to the client. This is
/// standard practice (GitHub, Stripe, etc.) and helps well-behaved clients
/// pace their requests. Attackers can already probe limits empirically, so the
/// headers do not meaningfully expand the attack surface.
///
/// Numeric-to-string-to-`HeaderValue` parsing is infallible, so the internal
/// `unwrap()` calls cannot panic.
#[allow(clippy::unwrap_used, clippy::missing_panics_doc)]
pub fn rate_limit_headers(result: &RateLimitResult) -> HeaderMap {
    let mut headers = HeaderMap::new();
    // Parsing numeric strings into HeaderValues never fails.
    headers.insert(
        "x-ratelimit-limit",
        result.limit.to_string().parse().unwrap(),
    );
    headers.insert(
        "x-ratelimit-remaining",
        result.remaining.to_string().parse().unwrap(),
    );
    headers.insert(
        "x-ratelimit-reset",
        result.reset_after_secs.to_string().parse().unwrap(),
    );
    headers
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // ── extract_ip ──────────────────────────────────────────────────────

    #[test]
    fn extract_ip_from_forwarded_for_single_ip() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "203.0.113.50".parse().unwrap());
        assert_eq!(extract_ip(&headers, true, None), "203.0.113.50");
    }

    #[test]
    fn extract_ip_from_forwarded_for_multiple_ips() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            "203.0.113.50, 70.41.3.18, 150.172.238.178".parse().unwrap(),
        );
        assert_eq!(extract_ip(&headers, true, None), "203.0.113.50");
    }

    #[test]
    fn extract_ip_trims_whitespace() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "  10.0.0.1 , 10.0.0.2".parse().unwrap());
        assert_eq!(extract_ip(&headers, true, None), "10.0.0.1");
    }

    #[test]
    fn extract_ip_returns_peer_when_header_missing() {
        let headers = HeaderMap::new();
        assert_eq!(
            extract_ip(&headers, true, Some("192.168.1.1")),
            "192.168.1.1"
        );
    }

    #[test]
    fn extract_ip_returns_unknown_when_no_header_and_no_peer() {
        let headers = HeaderMap::new();
        assert_eq!(extract_ip(&headers, true, None), "unknown");
    }

    #[test]
    fn extract_ip_uses_peer_when_trust_proxy_false() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "203.0.113.50".parse().unwrap());
        assert_eq!(extract_ip(&headers, false, Some("10.0.0.99")), "10.0.0.99");
    }

    #[test]
    fn extract_ip_returns_unknown_when_trust_proxy_false_and_no_peer() {
        let headers = HeaderMap::new();
        assert_eq!(extract_ip(&headers, false, None), "unknown");
    }

    // ── most_restrictive ────────────────────────────────────────────────

    fn make_result(allowed: bool, remaining: u32, limit: u32) -> RateLimitResult {
        RateLimitResult {
            allowed,
            limit,
            remaining,
            reset_after_secs: 60,
        }
    }

    #[test]
    fn most_restrictive_picks_fewer_remaining() {
        let a = make_result(true, 5, 10);
        let b = make_result(true, 8, 20);
        let r = most_restrictive(a, b);
        assert_eq!(r.remaining, 5);
        assert_eq!(r.limit, 10);
    }

    #[test]
    fn most_restrictive_picks_denied_over_allowed() {
        let denied = make_result(false, 0, 10);
        let allowed = make_result(true, 3, 20);
        let r = most_restrictive(denied, allowed);
        assert!(!r.allowed);
        assert_eq!(r.remaining, 0);
    }

    #[test]
    fn most_restrictive_picks_denied_when_second_is_denied() {
        let allowed = make_result(true, 3, 20);
        let denied = make_result(false, 0, 10);
        let r = most_restrictive(allowed, denied);
        assert!(!r.allowed);
        assert_eq!(r.remaining, 0);
    }

    #[test]
    fn most_restrictive_equal_remaining_picks_first() {
        let a = make_result(true, 5, 10);
        let b = make_result(true, 5, 20);
        let r = most_restrictive(a, b);
        // When equal, returns `a` (the first argument)
        assert_eq!(r.limit, 10);
    }

    // ── rate_limit_headers ──────────────────────────────────────────────

    #[test]
    fn rate_limit_headers_contains_correct_values() {
        let result = RateLimitResult {
            allowed: true,
            limit: 60,
            remaining: 42,
            reset_after_secs: 30,
        };
        let headers = rate_limit_headers(&result);
        assert_eq!(headers["x-ratelimit-limit"], "60");
        assert_eq!(headers["x-ratelimit-remaining"], "42");
        assert_eq!(headers["x-ratelimit-reset"], "30");
    }

    #[test]
    fn rate_limit_headers_zero_remaining() {
        let result = RateLimitResult {
            allowed: false,
            limit: 10,
            remaining: 0,
            reset_after_secs: 3600,
        };
        let headers = rate_limit_headers(&result);
        assert_eq!(headers["x-ratelimit-limit"], "10");
        assert_eq!(headers["x-ratelimit-remaining"], "0");
        assert_eq!(headers["x-ratelimit-reset"], "3600");
    }
}
