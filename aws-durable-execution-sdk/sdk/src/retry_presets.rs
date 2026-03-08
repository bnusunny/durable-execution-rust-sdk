//! Pre-configured retry strategies for common use cases.
//!
//! Provides convenience factory functions matching Node.js `retryPresets.default`
//! and `retryPresets.noRetry`.
//!
//! # Example
//!
//! ```
//! use durable_execution_sdk::retry_presets;
//!
//! // Default: 6 attempts, exponential backoff, 5s base, 60s max, full jitter
//! let default = retry_presets::default_retry();
//!
//! // No retry: fails immediately on first error
//! let no = retry_presets::no_retry();
//! ```

use crate::config::{ExponentialBackoff, JitterStrategy, NoRetry};
use crate::duration::Duration;

/// Returns a default retry strategy matching Node.js `retryPresets.default`.
///
/// Configuration:
/// - max_attempts: 6
/// - base_delay: 5 seconds
/// - max_delay: 60 seconds
/// - multiplier: 2.0
/// - jitter: Full
///
/// # Requirements
///
/// - 3.1: default_retry() returns ExponentialBackoff with max_attempts=6,
///   base_delay=5s, max_delay=60s, multiplier=2.0, jitter=Full
pub fn default_retry() -> ExponentialBackoff {
    ExponentialBackoff::builder()
        .max_attempts(6)
        .base_delay(Duration::from_seconds(5))
        .max_delay(Duration::from_seconds(60))
        .multiplier(2.0)
        .jitter(JitterStrategy::Full)
        .build()
}

/// Returns a no-retry strategy that fails immediately on first error.
///
/// This strategy returns `None` for any attempt, meaning no retries
/// will be performed.
///
/// # Requirements
///
/// - 3.2: no_retry() returns a NoRetry strategy that returns None for any attempt
pub fn no_retry() -> NoRetry {
    NoRetry
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RetryStrategy;

    #[test]
    fn test_default_retry_max_attempts() {
        let strategy = default_retry();
        assert_eq!(strategy.max_attempts, 6);
    }

    #[test]
    fn test_default_retry_base_delay() {
        let strategy = default_retry();
        assert_eq!(strategy.base_delay.to_seconds(), 5);
    }

    #[test]
    fn test_default_retry_max_delay() {
        let strategy = default_retry();
        assert_eq!(strategy.max_delay.to_seconds(), 60);
    }

    #[test]
    fn test_default_retry_multiplier() {
        let strategy = default_retry();
        assert!((strategy.multiplier - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_default_retry_jitter() {
        let strategy = default_retry();
        assert_eq!(strategy.jitter, JitterStrategy::Full);
    }

    #[test]
    fn test_default_retry_returns_delay_for_valid_attempts() {
        let strategy = default_retry();
        // Attempts 0..5 should return Some (6 attempts total)
        for attempt in 0..6 {
            assert!(
                strategy.next_delay(attempt, "error").is_some(),
                "Expected Some for attempt {}",
                attempt
            );
        }
    }

    #[test]
    fn test_default_retry_returns_none_after_max_attempts() {
        let strategy = default_retry();
        assert!(strategy.next_delay(6, "error").is_none());
        assert!(strategy.next_delay(7, "error").is_none());
    }

    #[test]
    fn test_default_retry_delays_at_least_one_second() {
        let strategy = default_retry();
        for attempt in 0..6 {
            let delay = strategy.next_delay(attempt, "error").unwrap();
            assert!(
                delay.to_seconds() >= 1,
                "Delay for attempt {} was {} seconds, expected >= 1",
                attempt,
                delay.to_seconds()
            );
        }
    }

    #[test]
    fn test_default_retry_delays_at_most_max_delay() {
        let strategy = default_retry();
        for attempt in 0..6 {
            let delay = strategy.next_delay(attempt, "error").unwrap();
            assert!(
                delay.to_seconds() <= 60,
                "Delay for attempt {} was {} seconds, expected <= 60",
                attempt,
                delay.to_seconds()
            );
        }
    }

    #[test]
    fn test_no_retry_returns_none() {
        let strategy = no_retry();
        assert!(strategy.next_delay(0, "error").is_none());
        assert!(strategy.next_delay(1, "error").is_none());
        assert!(strategy.next_delay(100, "any error").is_none());
    }
}
