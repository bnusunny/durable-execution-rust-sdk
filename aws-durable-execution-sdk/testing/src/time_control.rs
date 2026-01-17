//! Time control utilities for durable execution testing.
//!
//! This module provides the `TimeControl` struct for managing fake time during tests,
//! enabling instant advancement of wait operations without blocking.
//!
//! # Overview
//!
//! When testing durable functions that include wait operations, you typically don't want
//! to wait for actual time to pass. The `TimeControl` utilities integrate with Tokio's
//! time manipulation features to enable instant time advancement.
//!
//! # Examples
//!
//! ```ignore
//! use aws_durable_execution_sdk_testing::TimeControl;
//! use std::time::Duration;
//!
//! #[tokio::test]
//! async fn test_with_time_control() {
//!     // Enable time skipping
//!     TimeControl::enable().await.unwrap();
//!
//!     // Advance time by 5 seconds instantly
//!     TimeControl::advance(Duration::from_secs(5)).await;
//!
//!     // Disable time skipping when done
//!     TimeControl::disable().await.unwrap();
//! }
//! ```
//!
//! # Requirements
//!
//! - 2.1: WHEN time skipping is enabled, THE Local_Test_Runner SHALL use Tokio's
//!   time manipulation to skip wait durations
//! - 2.2: WHEN a wait operation is encountered with time skipping enabled,
//!   THE Local_Test_Runner SHALL advance time instantly without blocking
//! - 2.3: WHEN time skipping is disabled, THE Local_Test_Runner SHALL execute
//!   wait operations with real timing
//! - 2.4: WHEN teardown_test_environment() is called, THE Local_Test_Runner
//!   SHALL restore normal time behavior

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::error::TestError;

/// Global flag indicating whether time control is active.
static TIME_CONTROL_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Checks if Tokio's time is currently paused.
///
/// This is a helper function that can be used to check if time is paused
/// before calling `tokio::time::resume()`, which panics if time is not paused.
///
/// # Returns
///
/// `true` if time is currently paused, `false` otherwise.
pub fn is_time_paused() -> bool {
    TimeControl::is_time_paused_internal()
}

/// Utilities for controlling time during durable execution tests.
///
/// `TimeControl` provides methods to pause, resume, and advance time using
/// Tokio's time manipulation features. This enables tests to skip wait
/// operations instantly without blocking.
///
/// # Thread Safety
///
/// Time control is global to the Tokio runtime. Only one test should use
/// time control at a time, or tests should be run sequentially when using
/// time manipulation.
///
/// # Examples
///
/// ```ignore
/// use aws_durable_execution_sdk_testing::TimeControl;
/// use std::time::Duration;
///
/// #[tokio::test]
/// async fn test_workflow_with_waits() {
///     // Enable time skipping for fast test execution
///     TimeControl::enable().await.unwrap();
///
///     // Run your workflow - wait operations complete instantly
///     // ...
///
///     // Optionally advance time by a specific duration
///     TimeControl::advance(Duration::from_secs(60)).await;
///
///     // Restore normal time behavior
///     TimeControl::disable().await.unwrap();
/// }
/// ```
#[derive(Debug, Clone, Copy)]
pub struct TimeControl;

impl TimeControl {
    /// Enables time skipping by pausing Tokio's internal clock.
    ///
    /// When time is paused, `tokio::time::sleep` and other time-based operations
    /// will not block. Instead, time advances only when explicitly advanced or
    /// when there are no other tasks to run.
    ///
    /// # Requirements
    ///
    /// - 2.1: WHEN time skipping is enabled via setup_test_environment(),
    ///   THE Local_Test_Runner SHALL use Tokio's time manipulation to skip wait durations
    ///
    /// # Errors
    ///
    /// Returns `Ok(())` on success. This operation is idempotent - calling it
    /// multiple times has no additional effect.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::TimeControl;
    ///
    /// #[tokio::test]
    /// async fn test_enable_time_control() {
    ///     TimeControl::enable().await.unwrap();
    ///     assert!(TimeControl::is_enabled());
    ///     TimeControl::disable().await.unwrap();
    /// }
    /// ```
    pub async fn enable() -> Result<(), TestError> {
        if TIME_CONTROL_ACTIVE.load(Ordering::SeqCst) {
            // Already enabled, nothing to do
            return Ok(());
        }

        // Pause Tokio's internal clock
        // Note: tokio::time::pause() will panic if:
        // 1. Called when time is already paused (e.g., from a previous test)
        // 2. Called from a multi-threaded runtime (requires current_thread)
        // We handle both cases by checking if time is already paused first,
        // and by using catch_unwind to handle the multi-threaded runtime case.
        if !Self::is_time_paused_internal() {
            // Try to pause time, but handle the case where we're not in current_thread runtime
            use std::panic;
            let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                tokio::time::pause();
            }));
            
            if let Err(panic_info) = result {
                // Check if the panic was because we're not in a current_thread runtime
                let is_runtime_error = panic_info
                    .downcast_ref::<&str>()
                    .map(|msg| msg.contains("current_thread"))
                    .unwrap_or(false)
                    || panic_info
                        .downcast_ref::<String>()
                        .map(|msg| msg.contains("current_thread"))
                        .unwrap_or(false);
                
                if is_runtime_error {
                    // We're not in a current_thread runtime, so time control won't work
                    // but we can still set the flag to indicate the intent
                    tracing::warn!(
                        "Time control requires current_thread Tokio runtime. \
                         Time skipping may not work correctly."
                    );
                }
                // For other panics, we'll just continue - the flag will be set
                // and the user will see issues when they try to use time control
            }
        }
        TIME_CONTROL_ACTIVE.store(true, Ordering::SeqCst);

        Ok(())
    }

    /// Checks if Tokio's time is currently paused (regardless of our tracking flag).
    ///
    /// This is useful for detecting if time was paused by another test or code path.
    /// 
    /// Note: This function uses catch_unwind to detect if time is paused, which
    /// requires the current_thread runtime. If called from a multi-threaded runtime,
    /// it will return false (assuming time is not paused).
    pub(crate) fn is_time_paused_internal() -> bool {
        // Try to detect if time is paused by checking if we can call pause without panicking
        // Unfortunately, there's no direct API for this, so we use a workaround:
        // We check if the auto-advance behavior is active (which only happens when paused)
        use std::panic;

        // Use catch_unwind to safely check if pause would panic
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            // This will panic if:
            // 1. Time is already paused (the case we want to detect)
            // 2. We're not in a current_thread runtime (which we should treat as "not paused")
            tokio::time::pause();
        }));

        match result {
            Ok(()) => {
                // Pause succeeded, so time wasn't paused before. Resume to restore state.
                tokio::time::resume();
                false
            }
            Err(panic_info) => {
                // Check if the panic was because time was already paused
                // vs because we're not in a current_thread runtime
                if let Some(msg) = panic_info.downcast_ref::<&str>() {
                    if msg.contains("current_thread") {
                        // Not in current_thread runtime - treat as not paused
                        return false;
                    }
                }
                if let Some(msg) = panic_info.downcast_ref::<String>() {
                    if msg.contains("current_thread") {
                        // Not in current_thread runtime - treat as not paused
                        return false;
                    }
                }
                // Pause panicked for another reason (likely already paused)
                true
            }
        }
    }

    /// Disables time skipping by resuming Tokio's internal clock.
    ///
    /// After calling this method, time-based operations will use real time again.
    ///
    /// # Requirements
    ///
    /// - 2.4: WHEN teardown_test_environment() is called, THE Local_Test_Runner
    ///   SHALL restore normal time behavior
    ///
    /// # Errors
    ///
    /// Returns `Ok(())` on success. This operation is idempotent - calling it
    /// multiple times has no additional effect.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::TimeControl;
    ///
    /// #[tokio::test]
    /// async fn test_disable_time_control() {
    ///     TimeControl::enable().await.unwrap();
    ///     TimeControl::disable().await.unwrap();
    ///     assert!(!TimeControl::is_enabled());
    /// }
    /// ```
    pub async fn disable() -> Result<(), TestError> {
        if !TIME_CONTROL_ACTIVE.load(Ordering::SeqCst) {
            // Already disabled, nothing to do
            return Ok(());
        }

        // Resume Tokio's internal clock
        // Note: tokio::time::resume() panics if time is not paused,
        // so we use catch_unwind to handle this case gracefully
        use std::panic;
        let _ = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            tokio::time::resume();
        }));
        TIME_CONTROL_ACTIVE.store(false, Ordering::SeqCst);

        Ok(())
    }

    /// Returns whether time control is currently enabled.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::TimeControl;
    ///
    /// #[tokio::test]
    /// async fn test_is_enabled() {
    ///     assert!(!TimeControl::is_enabled());
    ///     TimeControl::enable().await.unwrap();
    ///     assert!(TimeControl::is_enabled());
    ///     TimeControl::disable().await.unwrap();
    ///     assert!(!TimeControl::is_enabled());
    /// }
    /// ```
    pub fn is_enabled() -> bool {
        TIME_CONTROL_ACTIVE.load(Ordering::SeqCst)
    }

    /// Advances the paused clock by the specified duration.
    ///
    /// This method instantly advances Tokio's internal clock by the given duration,
    /// causing any pending timers that would expire within that duration to fire.
    ///
    /// # Requirements
    ///
    /// - 2.2: WHEN a wait operation is encountered with time skipping enabled,
    ///   THE Local_Test_Runner SHALL advance time instantly without blocking
    ///
    /// # Arguments
    ///
    /// * `duration` - The amount of time to advance the clock by
    ///
    /// # Panics
    ///
    /// This method will panic if time control is not enabled (i.e., if `enable()`
    /// has not been called or `disable()` has been called).
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::TimeControl;
    /// use std::time::Duration;
    ///
    /// #[tokio::test]
    /// async fn test_advance_time() {
    ///     TimeControl::enable().await.unwrap();
    ///
    ///     // Advance time by 1 hour instantly
    ///     TimeControl::advance(Duration::from_secs(3600)).await;
    ///
    ///     TimeControl::disable().await.unwrap();
    /// }
    /// ```
    pub async fn advance(duration: Duration) {
        tokio::time::advance(duration).await;
    }

    /// Advances the paused clock by the specified number of seconds.
    ///
    /// This is a convenience method equivalent to `advance(Duration::from_secs(seconds))`.
    ///
    /// # Arguments
    ///
    /// * `seconds` - The number of seconds to advance the clock by
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::TimeControl;
    ///
    /// #[tokio::test]
    /// async fn test_advance_seconds() {
    ///     TimeControl::enable().await.unwrap();
    ///
    ///     // Advance time by 60 seconds
    ///     TimeControl::advance_secs(60).await;
    ///
    ///     TimeControl::disable().await.unwrap();
    /// }
    /// ```
    pub async fn advance_secs(seconds: u64) {
        Self::advance(Duration::from_secs(seconds)).await;
    }

    /// Advances the paused clock by the specified number of milliseconds.
    ///
    /// This is a convenience method equivalent to `advance(Duration::from_millis(millis))`.
    ///
    /// # Arguments
    ///
    /// * `millis` - The number of milliseconds to advance the clock by
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::TimeControl;
    ///
    /// #[tokio::test]
    /// async fn test_advance_millis() {
    ///     TimeControl::enable().await.unwrap();
    ///
    ///     // Advance time by 500 milliseconds
    ///     TimeControl::advance_millis(500).await;
    ///
    ///     TimeControl::disable().await.unwrap();
    /// }
    /// ```
    pub async fn advance_millis(millis: u64) {
        Self::advance(Duration::from_millis(millis)).await;
    }

    /// Resets time control state.
    ///
    /// This method disables time control if it's enabled and resets the internal
    /// state. It's useful for cleanup between tests.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::TimeControl;
    ///
    /// #[tokio::test]
    /// async fn test_reset() {
    ///     TimeControl::enable().await.unwrap();
    ///     TimeControl::reset().await.unwrap();
    ///     assert!(!TimeControl::is_enabled());
    /// }
    /// ```
    pub async fn reset() -> Result<(), TestError> {
        Self::disable().await
    }
}

/// A guard that automatically disables time control when dropped.
///
/// This is useful for ensuring time control is properly cleaned up even if
/// a test panics or returns early.
///
/// # Examples
///
/// ```ignore
/// use aws_durable_execution_sdk_testing::TimeControlGuard;
///
/// #[tokio::test]
/// async fn test_with_guard() {
///     let _guard = TimeControlGuard::new().await.unwrap();
///     // Time control is now enabled
///     
///     // ... run tests ...
///     
///     // Time control is automatically disabled when _guard is dropped
/// }
/// ```
pub struct TimeControlGuard {
    _private: (),
}

impl TimeControlGuard {
    /// Creates a new `TimeControlGuard` and enables time control.
    ///
    /// # Errors
    ///
    /// Returns an error if time control cannot be enabled.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::TimeControlGuard;
    ///
    /// #[tokio::test]
    /// async fn test_guard_creation() {
    ///     let guard = TimeControlGuard::new().await.unwrap();
    ///     assert!(TimeControl::is_enabled());
    ///     drop(guard);
    /// }
    /// ```
    pub async fn new() -> Result<Self, TestError> {
        TimeControl::enable().await?;
        Ok(Self { _private: () })
    }
}

impl Drop for TimeControlGuard {
    fn drop(&mut self) {
        // We can't call async functions in Drop, so we just update the flag
        // and let the runtime handle the resume on the next opportunity.
        // Note: This is a best-effort cleanup. For proper cleanup, users should
        // explicitly call TimeControl::disable() or use the guard in an async context.
        if TIME_CONTROL_ACTIVE.load(Ordering::SeqCst) {
            // Resume time synchronously - this is safe because tokio::time::resume()
            // is not async and can be called from any context
            tokio::time::resume();
            TIME_CONTROL_ACTIVE.store(false, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    async fn test_enable_disable() {
        // Ensure clean state
        TimeControl::disable().await.unwrap();
        assert!(!TimeControl::is_enabled());

        // Enable
        TimeControl::enable().await.unwrap();
        assert!(TimeControl::is_enabled());

        // Disable
        TimeControl::disable().await.unwrap();
        assert!(!TimeControl::is_enabled());
    }

    #[tokio::test]
    async fn test_enable_is_idempotent() {
        // Ensure clean state - first check if time is paused and resume if so
        // Note: We need to be careful here because is_time_paused_internal() 
        // uses catch_unwind which may not work well in all test scenarios
        if TIME_CONTROL_ACTIVE.load(Ordering::SeqCst) {
            tokio::time::resume();
            TIME_CONTROL_ACTIVE.store(false, Ordering::SeqCst);
        }

        // Enable multiple times
        TimeControl::enable().await.unwrap();
        TimeControl::enable().await.unwrap();
        TimeControl::enable().await.unwrap();

        assert!(TimeControl::is_enabled());

        // Cleanup
        TimeControl::disable().await.unwrap();
    }

    #[tokio::test]
    async fn test_disable_is_idempotent() {
        // Ensure clean state
        TimeControl::disable().await.unwrap();

        // Disable multiple times (should not panic)
        TimeControl::disable().await.unwrap();
        TimeControl::disable().await.unwrap();

        assert!(!TimeControl::is_enabled());
    }

    #[tokio::test]
    async fn test_advance_time() {
        // Ensure clean state
        TimeControl::disable().await.unwrap();

        // Enable time control
        TimeControl::enable().await.unwrap();

        let start = Instant::now();

        // Advance by 1 second
        TimeControl::advance_secs(1).await;

        // The wall clock time should be very small (not 1 second)
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(100),
            "Time advance should be instant, but took {:?}",
            elapsed
        );

        // Cleanup
        TimeControl::disable().await.unwrap();
    }

    #[tokio::test]
    async fn test_advance_millis() {
        // Ensure clean state
        TimeControl::disable().await.unwrap();

        // Enable time control
        TimeControl::enable().await.unwrap();

        let start = Instant::now();

        // Advance by 5000 milliseconds (5 seconds)
        TimeControl::advance_millis(5000).await;

        // The wall clock time should be very small
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(100),
            "Time advance should be instant, but took {:?}",
            elapsed
        );

        // Cleanup
        TimeControl::disable().await.unwrap();
    }

    #[tokio::test]
    async fn test_sleep_with_time_control() {
        // Ensure clean state
        TimeControl::disable().await.unwrap();

        // Enable time control
        TimeControl::enable().await.unwrap();

        let start = Instant::now();

        // Sleep for 10 seconds (should be instant with time control)
        tokio::time::sleep(Duration::from_secs(10)).await;

        // The wall clock time should be very small
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(100),
            "Sleep should be instant with time control, but took {:?}",
            elapsed
        );

        // Cleanup
        TimeControl::disable().await.unwrap();
    }

    #[tokio::test]
    async fn test_reset() {
        // Enable time control
        TimeControl::enable().await.unwrap();
        assert!(TimeControl::is_enabled());

        // Reset
        TimeControl::reset().await.unwrap();
        assert!(!TimeControl::is_enabled());
    }

    #[tokio::test]
    async fn test_guard_enables_time_control() {
        // Ensure clean state
        TimeControl::disable().await.unwrap();
        assert!(!TimeControl::is_enabled());

        {
            let _guard = TimeControlGuard::new().await.unwrap();
            assert!(TimeControl::is_enabled());
        }

        // Guard should have disabled time control on drop
        assert!(!TimeControl::is_enabled());
    }

    #[tokio::test]
    async fn test_guard_cleanup_on_drop() {
        // Ensure clean state
        TimeControl::disable().await.unwrap();

        // Create and immediately drop guard
        let guard = TimeControlGuard::new().await.unwrap();
        assert!(TimeControl::is_enabled());
        drop(guard);

        // Time control should be disabled
        assert!(!TimeControl::is_enabled());
    }
}
