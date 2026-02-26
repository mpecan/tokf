use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Mutex;
use std::time::{Duration, Instant};

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
    pub fn new(max_per_window: u32, window_secs: u64) -> Self {
        Self {
            window: Mutex::new(HashMap::new()),
            max_per_window,
            window_duration: Duration::from_secs(window_secs),
        }
    }

    /// Returns `true` if the call is allowed and increments the counter.
    /// Returns `false` if the key has exceeded its quota for this window.
    // `guard` must outlive `entry` because `entry` borrows from the HashMap behind the lock.
    #[allow(clippy::significant_drop_tightening)]
    pub fn check_and_increment(&self, key: K) -> bool {
        let now = Instant::now();
        let mut guard = self
            .window
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = guard.entry(key).or_insert((0, now));
        if now.duration_since(entry.1) >= self.window_duration {
            *entry = (1, now);
            return true;
        }
        if entry.0 >= self.max_per_window {
            return false;
        }
        entry.0 += 1;
        true
    }
}

/// Rate limiter keyed by user ID (i64); used for publish endpoint.
pub type PublishRateLimiter = RateLimiter<i64>;

/// Rate limiter keyed by machine UUID (as u128); used for sync endpoint.
pub type SyncRateLimiter = RateLimiter<u128>;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn allows_calls_within_limit() {
        let limiter = PublishRateLimiter::new(3, 3600);
        assert!(limiter.check_and_increment(1));
        assert!(limiter.check_and_increment(1));
        assert!(limiter.check_and_increment(1));
    }

    #[test]
    fn blocks_calls_over_limit() {
        let limiter = PublishRateLimiter::new(2, 3600);
        assert!(limiter.check_and_increment(42));
        assert!(limiter.check_and_increment(42));
        assert!(!limiter.check_and_increment(42));
        assert!(!limiter.check_and_increment(42));
    }

    #[test]
    fn different_users_are_independent() {
        let limiter = PublishRateLimiter::new(1, 3600);
        assert!(limiter.check_and_increment(1));
        assert!(!limiter.check_and_increment(1));
        assert!(limiter.check_and_increment(2)); // user 2 has fresh quota
    }

    #[test]
    fn sync_rate_limiter_allows_within_limit() {
        let limiter = SyncRateLimiter::new(3, 3600);
        let uuid = 0xdead_beef_u128;
        assert!(limiter.check_and_increment(uuid));
        assert!(limiter.check_and_increment(uuid));
        assert!(limiter.check_and_increment(uuid));
    }

    #[test]
    fn sync_rate_limiter_blocks_over_limit() {
        let limiter = SyncRateLimiter::new(2, 3600);
        let uuid = 0xcafe_babe_u128;
        assert!(limiter.check_and_increment(uuid));
        assert!(limiter.check_and_increment(uuid));
        assert!(!limiter.check_and_increment(uuid));
    }

    #[test]
    fn sync_different_machines_are_independent() {
        let limiter = SyncRateLimiter::new(1, 3600);
        let m1 = 0x1111_u128;
        let m2 = 0x2222_u128;
        assert!(limiter.check_and_increment(m1));
        assert!(!limiter.check_and_increment(m1));
        assert!(limiter.check_and_increment(m2)); // machine 2 has fresh quota
    }
}
