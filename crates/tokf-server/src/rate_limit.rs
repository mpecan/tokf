use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Outcome of a rate-limit check.
#[derive(Debug, Clone, Copy)]
pub struct RateLimitResult {
    /// Whether the request is allowed.
    pub allowed: bool,
    /// Maximum requests allowed in this window.
    pub limit: u32,
    /// Requests remaining in the current window (0 when denied).
    pub remaining: u32,
    /// Seconds until the current window resets.
    pub reset_after_secs: u64,
}

/// Per-key sliding-window rate limiter.
///
/// Each key gets `max_per_window` allowed calls within `window`. After the
/// window elapses since the first call in the current window, the counter
/// resets automatically.
pub struct RateLimiter<K: Eq + Hash> {
    window: Mutex<HashMap<K, (u32, Instant)>>,
    max_per_window: u32,
    window_duration: Duration,
}

impl<K: Eq + Hash> RateLimiter<K> {
    /// Evict expired entries once the map exceeds this many keys.
    const EVICTION_THRESHOLD: usize = 10_000;

    pub fn new(max_per_window: u32, window_secs: u64) -> Self {
        Self {
            window: Mutex::new(HashMap::new()),
            max_per_window,
            window_duration: Duration::from_secs(window_secs),
        }
    }

    /// Check whether the key is within its rate limit and increment the counter.
    ///
    /// Returns a [`RateLimitResult`] with the allowed flag, limit, remaining
    /// quota, and seconds until the window resets.
    ///
    /// Periodically evicts expired entries to bound memory usage.
    // `guard` must outlive `entry` because `entry` borrows from the HashMap behind the lock.
    #[allow(clippy::significant_drop_tightening)]
    pub fn check_and_increment(&self, key: K) -> RateLimitResult {
        let now = Instant::now();
        let mut guard = self
            .window
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        // Evict expired entries when the map grows large to prevent unbounded memory.
        if guard.len() > Self::EVICTION_THRESHOLD {
            guard.retain(|_, (_, ts)| now.duration_since(*ts) < self.window_duration);
        }

        let entry = guard.entry(key).or_insert((0, now));

        // Window expired — reset counter.
        if now.duration_since(entry.1) >= self.window_duration {
            *entry = (1, now);
            return RateLimitResult {
                allowed: true,
                limit: self.max_per_window,
                remaining: self.max_per_window.saturating_sub(1),
                reset_after_secs: self.window_duration.as_secs(),
            };
        }

        let reset_after_secs = self
            .window_duration
            .saturating_sub(now.duration_since(entry.1))
            .as_secs();

        // Over limit — deny.
        if entry.0 >= self.max_per_window {
            return RateLimitResult {
                allowed: false,
                limit: self.max_per_window,
                remaining: 0,
                reset_after_secs,
            };
        }

        // Within limit — allow and increment.
        entry.0 += 1;
        RateLimitResult {
            allowed: true,
            limit: self.max_per_window,
            remaining: self.max_per_window.saturating_sub(entry.0),
            reset_after_secs,
        }
    }
}

/// Rate limiter keyed by user ID (i64); used for publish endpoint.
pub type PublishRateLimiter = RateLimiter<i64>;

/// Rate limiter keyed by machine UUID (as u128); used for sync endpoint.
pub type SyncRateLimiter = RateLimiter<u128>;

/// Rate limiter keyed by IP address string; used for per-IP endpoint limits.
pub type IpRateLimiter = RateLimiter<String>;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn allows_calls_within_limit() {
        let limiter = PublishRateLimiter::new(3, 3600);
        assert!(limiter.check_and_increment(1).allowed);
        assert!(limiter.check_and_increment(1).allowed);
        assert!(limiter.check_and_increment(1).allowed);
    }

    #[test]
    fn blocks_calls_over_limit() {
        let limiter = PublishRateLimiter::new(2, 3600);
        assert!(limiter.check_and_increment(42).allowed);
        assert!(limiter.check_and_increment(42).allowed);
        assert!(!limiter.check_and_increment(42).allowed);
        assert!(!limiter.check_and_increment(42).allowed);
    }

    #[test]
    fn different_users_are_independent() {
        let limiter = PublishRateLimiter::new(1, 3600);
        assert!(limiter.check_and_increment(1).allowed);
        assert!(!limiter.check_and_increment(1).allowed);
        assert!(limiter.check_and_increment(2).allowed); // user 2 has fresh quota
    }

    #[test]
    fn sync_rate_limiter_allows_within_limit() {
        let limiter = SyncRateLimiter::new(3, 3600);
        let uuid = 0xdead_beef_u128;
        assert!(limiter.check_and_increment(uuid).allowed);
        assert!(limiter.check_and_increment(uuid).allowed);
        assert!(limiter.check_and_increment(uuid).allowed);
    }

    #[test]
    fn sync_rate_limiter_blocks_over_limit() {
        let limiter = SyncRateLimiter::new(2, 3600);
        let uuid = 0xcafe_babe_u128;
        assert!(limiter.check_and_increment(uuid).allowed);
        assert!(limiter.check_and_increment(uuid).allowed);
        assert!(!limiter.check_and_increment(uuid).allowed);
    }

    #[test]
    fn sync_different_machines_are_independent() {
        let limiter = SyncRateLimiter::new(1, 3600);
        let m1 = 0x1111_u128;
        let m2 = 0x2222_u128;
        assert!(limiter.check_and_increment(m1).allowed);
        assert!(!limiter.check_and_increment(m1).allowed);
        assert!(limiter.check_and_increment(m2).allowed); // machine 2 has fresh quota
    }

    #[test]
    fn result_remaining_decrements_correctly() {
        let limiter = PublishRateLimiter::new(3, 3600);
        let r1 = limiter.check_and_increment(1);
        assert_eq!(r1.remaining, 2);
        assert_eq!(r1.limit, 3);

        let r2 = limiter.check_and_increment(1);
        assert_eq!(r2.remaining, 1);

        let r3 = limiter.check_and_increment(1);
        assert_eq!(r3.remaining, 0);
        assert!(r3.allowed);

        let r4 = limiter.check_and_increment(1);
        assert_eq!(r4.remaining, 0);
        assert!(!r4.allowed);
    }

    #[test]
    fn result_denied_has_zero_remaining() {
        let limiter = PublishRateLimiter::new(1, 3600);
        let _ = limiter.check_and_increment(1);
        let denied = limiter.check_and_increment(1);
        assert!(!denied.allowed);
        assert_eq!(denied.remaining, 0);
        assert_eq!(denied.limit, 1);
    }

    #[test]
    fn result_reset_after_within_window() {
        let limiter = PublishRateLimiter::new(10, 3600);
        let r = limiter.check_and_increment(1);
        // reset_after_secs should be close to 3600 (the full window)
        assert!(r.reset_after_secs <= 3600);
        assert!(r.reset_after_secs >= 3599);
    }

    #[test]
    fn ip_rate_limiter_basic() {
        let limiter = IpRateLimiter::new(2, 60);
        let ip = "192.168.1.1".to_string();
        assert!(limiter.check_and_increment(ip.clone()).allowed);
        assert!(limiter.check_and_increment(ip.clone()).allowed);
        assert!(!limiter.check_and_increment(ip).allowed);
        // Different IP gets fresh quota
        assert!(limiter.check_and_increment("10.0.0.1".to_string()).allowed);
    }

    #[test]
    fn eviction_removes_expired_entries() {
        // Use a 1-second window so entries expire quickly.
        let limiter = IpRateLimiter::new(100, 1);

        // Insert entries above the eviction threshold.
        for i in 0..=IpRateLimiter::EVICTION_THRESHOLD {
            limiter.check_and_increment(format!("10.0.{}.{}", i / 256, i % 256));
        }

        let count_before = limiter.window.lock().unwrap().len();
        assert!(
            count_before > IpRateLimiter::EVICTION_THRESHOLD,
            "should have > threshold entries before eviction"
        );

        // Wait for the window to expire.
        std::thread::sleep(std::time::Duration::from_millis(1100));

        // Next check triggers eviction.
        limiter.check_and_increment("trigger".to_string());
        let count_after = limiter.window.lock().unwrap().len();
        assert_eq!(
            count_after, 1,
            "all expired entries should be evicted, only 'trigger' remains"
        );
    }
}
