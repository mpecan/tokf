use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Per-user sliding-window rate limiter.
///
/// Each user gets `max_per_window` allowed calls within `window`. After the
/// window elapses since the first call in the current window, the counter
/// resets automatically.
pub struct PublishRateLimiter {
    window: Mutex<HashMap<i64, (u32, Instant)>>,
    max_per_window: u32,
    window_duration: Duration,
}

impl PublishRateLimiter {
    pub fn new(max_per_window: u32, window_secs: u64) -> Self {
        Self {
            window: Mutex::new(HashMap::new()),
            max_per_window,
            window_duration: Duration::from_secs(window_secs),
        }
    }

    /// Returns `true` if the call is allowed and increments the counter.
    /// Returns `false` if the user has exceeded their quota for this window.
    // `guard` must outlive `entry` because `entry` borrows from the HashMap behind the lock.
    #[allow(clippy::significant_drop_tightening)]
    pub fn check_and_increment(&self, user_id: i64) -> bool {
        let now = Instant::now();
        let mut guard = self
            .window
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = guard.entry(user_id).or_insert((0, now));
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
}
