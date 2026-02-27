//! Graceful termination management for Lambda timeout detection.
//!
//! This module provides [`TerminationManager`] which proactively detects
//! approaching Lambda timeouts and signals the runtime to flush checkpoints
//! and return a PENDING response before the Lambda hard-kills the process.
//!
//! # Requirements
//!
//! - 5.1: wait_for_timeout resolves when deadline - safety_margin is reached
//! - 5.2: On timeout, flush batcher and return PENDING
//! - 5.3: Handler completes normally when no timeout
//! - 5.4: Default safety margin is 5 seconds
//! - 5.5: Immediate resolution when remaining < margin

use std::time::{Duration, SystemTime};

/// Default safety margin in milliseconds (5 seconds).
const DEFAULT_SAFETY_MARGIN_MS: u64 = 5000;

/// Manages graceful termination before Lambda timeout.
///
/// Created from a Lambda context, this struct calculates how much time
/// remains before the Lambda deadline and provides an async signal that
/// fires before the hard timeout, giving the runtime time to flush
/// pending checkpoints.
pub struct TerminationManager {
    remaining_time_ms: u64,
    safety_margin_ms: u64,
}

impl TerminationManager {
    /// Creates a new manager from the Lambda context deadline.
    ///
    /// Calculates `remaining_time_ms` from `ctx.deadline() - now`.
    /// The safety margin defaults to 5000ms (5 seconds).
    ///
    /// # Requirements
    ///
    /// - 5.4: Default safety margin is 5 seconds
    pub fn from_lambda_context(ctx: &lambda_runtime::Context) -> Self {
        let deadline = ctx.deadline();
        let now = SystemTime::now();

        let remaining_time_ms = deadline
            .duration_since(now)
            .unwrap_or(Duration::ZERO)
            .as_millis() as u64;

        Self {
            remaining_time_ms,
            safety_margin_ms: DEFAULT_SAFETY_MARGIN_MS,
        }
    }

    /// Returns a future that resolves when the timeout margin is reached.
    ///
    /// If the remaining time is already less than the safety margin,
    /// this resolves immediately.
    ///
    /// # Requirements
    ///
    /// - 5.1: wait_for_timeout resolves when deadline - safety_margin is reached
    /// - 5.5: Immediate resolution when remaining < margin
    pub async fn wait_for_timeout(&self) {
        let effective_ms = self.remaining_ms();
        if effective_ms == 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(effective_ms)).await;
    }

    /// Returns the remaining time before the safety margin in milliseconds.
    ///
    /// Returns 0 if the remaining time is already less than the safety margin.
    pub fn remaining_ms(&self) -> u64 {
        self.remaining_time_ms.saturating_sub(self.safety_margin_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a TerminationManager with specific values for testing.
    fn make_manager(remaining_time_ms: u64, safety_margin_ms: u64) -> TerminationManager {
        TerminationManager {
            remaining_time_ms,
            safety_margin_ms,
        }
    }

    #[test]
    fn test_remaining_ms_normal() {
        let mgr = make_manager(10000, 5000);
        assert_eq!(mgr.remaining_ms(), 5000);
    }

    #[test]
    fn test_remaining_ms_less_than_margin() {
        let mgr = make_manager(3000, 5000);
        assert_eq!(mgr.remaining_ms(), 0);
    }

    #[test]
    fn test_remaining_ms_equal_to_margin() {
        let mgr = make_manager(5000, 5000);
        assert_eq!(mgr.remaining_ms(), 0);
    }

    #[test]
    fn test_remaining_ms_zero_remaining() {
        let mgr = make_manager(0, 5000);
        assert_eq!(mgr.remaining_ms(), 0);
    }

    #[test]
    fn test_default_safety_margin() {
        assert_eq!(DEFAULT_SAFETY_MARGIN_MS, 5000);
    }

    #[tokio::test]
    async fn test_wait_for_timeout_fires_at_deadline_minus_margin() {
        // 100ms remaining after margin
        let mgr = make_manager(5100, 5000);
        assert_eq!(mgr.remaining_ms(), 100);

        let start = tokio::time::Instant::now();
        tokio::time::pause();
        mgr.wait_for_timeout().await;
        let elapsed = start.elapsed();

        // Should have waited ~100ms
        assert!(
            elapsed.as_millis() >= 100,
            "Expected >= 100ms, got {}ms",
            elapsed.as_millis()
        );
    }

    #[tokio::test]
    async fn test_wait_for_timeout_immediate_when_remaining_less_than_margin() {
        let mgr = make_manager(3000, 5000);
        assert_eq!(mgr.remaining_ms(), 0);

        let start = tokio::time::Instant::now();
        tokio::time::pause();
        mgr.wait_for_timeout().await;
        let elapsed = start.elapsed();

        // Should resolve immediately (< 10ms)
        assert!(
            elapsed.as_millis() < 10,
            "Expected immediate resolution, got {}ms",
            elapsed.as_millis()
        );
    }

    #[tokio::test]
    async fn test_wait_for_timeout_immediate_when_zero_remaining() {
        let mgr = make_manager(0, 5000);

        let start = tokio::time::Instant::now();
        tokio::time::pause();
        mgr.wait_for_timeout().await;
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 10,
            "Expected immediate resolution, got {}ms",
            elapsed.as_millis()
        );
    }

    #[test]
    fn test_from_lambda_context_uses_default_margin() {
        use std::time::{Duration, SystemTime};

        // Create a lambda context with a deadline 10 seconds from now
        let mut ctx = lambda_runtime::Context::default();
        let deadline = SystemTime::now() + Duration::from_secs(10);
        ctx.deadline = deadline
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let mgr = TerminationManager::from_lambda_context(&ctx);

        assert_eq!(mgr.safety_margin_ms, DEFAULT_SAFETY_MARGIN_MS);
        // remaining_ms should be approximately 5000 (10000 - 5000)
        // Allow some tolerance for test execution time
        assert!(
            mgr.remaining_ms() >= 4900 && mgr.remaining_ms() <= 5100,
            "Expected ~5000ms, got {}ms",
            mgr.remaining_ms()
        );
    }

    #[test]
    fn test_from_lambda_context_past_deadline() {
        use std::time::{Duration, SystemTime};

        // Create a lambda context with a deadline in the past
        let mut ctx = lambda_runtime::Context::default();
        let deadline = SystemTime::now() - Duration::from_secs(1);
        // For past deadlines, duration_since will fail, so remaining should be 0
        ctx.deadline = deadline
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let mgr = TerminationManager::from_lambda_context(&ctx);

        assert_eq!(mgr.remaining_ms(), 0);
    }

    /// Integration test: verifies that when the termination manager fires before
    /// the handler completes, the select! pattern returns PENDING output.
    /// This simulates the exact behavior of run_durable_handler on timeout.
    #[tokio::test]
    async fn test_pending_output_on_simulated_timeout() {
        use crate::lambda::DurableExecutionInvocationOutput;

        tokio::time::pause();

        // Termination manager with 100ms effective timeout (6100 - 5000 = 1100ms, but
        // we use a small remaining to make the test fast)
        let mgr = make_manager(5050, 5000); // 50ms effective

        // Simulate a handler that takes much longer than the timeout
        let handler_future = async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            "handler_completed"
        };

        let output = tokio::select! {
            result = handler_future => {
                // Handler completed — this should NOT happen
                panic!("Handler should not complete before timeout, got: {}", result);
            }
            _ = mgr.wait_for_timeout() => {
                // Timeout fired — return PENDING
                DurableExecutionInvocationOutput::pending()
            }
        };

        assert!(output.is_pending(), "Expected PENDING output on timeout");
    }

    /// Integration test: verifies that when the handler completes before the
    /// termination manager fires, the handler result is returned normally.
    #[tokio::test]
    async fn test_handler_completes_before_timeout() {
        tokio::time::pause();

        // Termination manager with plenty of time (10s effective)
        let mgr = make_manager(15000, 5000); // 10000ms effective

        // Simulate a handler that completes quickly
        let handler_future = async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            "handler_result"
        };

        let result = tokio::select! {
            result = handler_future => {
                result
            }
            _ = mgr.wait_for_timeout() => {
                panic!("Timeout should not fire before handler completes");
            }
        };

        assert_eq!(result, "handler_result");
    }
}
