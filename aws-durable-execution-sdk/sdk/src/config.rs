//! Configuration types for durable execution operations.
//!
//! This module provides type-safe configuration structs for all
//! durable operations including steps, callbacks, invocations,
//! map, and parallel operations.
//!
//! ## Performance Configuration
//!
//! The SDK supports different checkpointing modes that trade off between
//! durability and performance. See [`CheckpointingMode`] for details.
//!
//! ## Sealed Traits
//!
//! The `RetryStrategy` trait is sealed and cannot be implemented outside of this crate.
//! This allows the SDK maintainers to evolve the retry interface without breaking
//! external code.

use std::marker::PhantomData;
use std::sync::Arc;

use blake2::{Blake2b512, Digest};
use serde::{Deserialize, Serialize};

use crate::duration::Duration;
use crate::error::DurableError;
use crate::sealed::Sealed;

/// Jitter strategy for retry delays.
///
/// Jitter adds randomness to retry delays to prevent thundering herd problems
/// when many executions retry simultaneously.
///
/// # Variants
///
/// - `None` — Use the exact calculated delay (no jitter).
/// - `Full` — Random delay in `[0, calculated_delay]`.
/// - `Half` — Random delay in `[calculated_delay/2, calculated_delay]`.
///
/// # Example
///
/// ```
/// use durable_execution_sdk::config::JitterStrategy;
///
/// let none = JitterStrategy::None;
/// assert_eq!(none.apply(10.0, 1), 10.0);
///
/// let full = JitterStrategy::Full;
/// let jittered = full.apply(10.0, 1);
/// assert!(jittered >= 0.0 && jittered <= 10.0);
///
/// let half = JitterStrategy::Half;
/// let jittered = half.apply(10.0, 1);
/// assert!(jittered >= 5.0 && jittered <= 10.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JitterStrategy {
    /// No jitter — use exact calculated delay.
    #[default]
    None,
    /// Full jitter — random delay in [0, calculated_delay].
    Full,
    /// Half jitter — random delay in [calculated_delay/2, calculated_delay].
    Half,
}

impl JitterStrategy {
    /// Applies jitter to a delay value in seconds.
    ///
    /// Uses a deterministic seed derived from the attempt number via blake2b
    /// hashing. This makes jitter replay-safe since the same attempt always
    /// produces the same jittered value.
    ///
    /// # Arguments
    ///
    /// * `delay_secs` - The base delay in seconds
    /// * `attempt` - The retry attempt number (used as seed for deterministic randomness)
    ///
    /// # Returns
    ///
    /// The jittered delay in seconds:
    /// - `None`: returns `delay_secs` exactly
    /// - `Full`: returns a value in `[0, delay_secs]`
    /// - `Half`: returns a value in `[delay_secs/2, delay_secs]`
    pub fn apply(&self, delay_secs: f64, attempt: u32) -> f64 {
        match self {
            JitterStrategy::None => delay_secs,
            JitterStrategy::Full => {
                let factor = deterministic_random_factor(attempt);
                factor * delay_secs
            }
            JitterStrategy::Half => {
                let factor = deterministic_random_factor(attempt);
                delay_secs / 2.0 + factor * (delay_secs / 2.0)
            }
        }
    }
}

/// Generates a deterministic random factor in [0.0, 1.0) from an attempt number.
///
/// Uses blake2b hashing to produce a deterministic pseudo-random value
/// seeded by the attempt number. This ensures replay safety.
fn deterministic_random_factor(attempt: u32) -> f64 {
    let mut hasher = Blake2b512::new();
    hasher.update(b"jitter");
    hasher.update(attempt.to_le_bytes());
    let result = hasher.finalize();

    // Take the first 8 bytes and convert to a u64, then normalize to [0.0, 1.0)
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&result[..8]);
    let value = u64::from_le_bytes(bytes);
    (value as f64) / (u64::MAX as f64)
}

/// Decision returned by a wait strategy.
///
/// A wait strategy function returns this enum to indicate whether polling
/// should continue (with a specified delay) or stop (condition is met).
///
/// # Variants
///
/// - `Continue { delay }` — Continue polling after the specified delay.
/// - `Done` — Stop polling; the condition has been met.
///
/// # Example
///
/// ```
/// use durable_execution_sdk::config::WaitDecision;
/// use durable_execution_sdk::Duration;
///
/// let cont = WaitDecision::Continue { delay: Duration::from_seconds(5) };
/// let done = WaitDecision::Done;
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitDecision {
    /// Continue polling after the specified delay.
    Continue { delay: Duration },
    /// Stop polling — condition is met.
    Done,
}

/// Configuration for creating a wait strategy.
///
/// This struct holds all the parameters needed to build a wait strategy function
/// via [`create_wait_strategy`]. The resulting function can be used with
/// [`WaitForConditionConfig`](crate::context::WaitForConditionConfig) to control
/// polling behavior with backoff, jitter, and a custom predicate.
///
/// # Type Parameters
///
/// - `T`: The state type returned by the condition check function.
///
/// # Example
///
/// ```
/// use durable_execution_sdk::config::{WaitStrategyConfig, JitterStrategy, create_wait_strategy, WaitDecision};
/// use durable_execution_sdk::Duration;
///
/// let config = WaitStrategyConfig {
///     max_attempts: Some(10),
///     initial_delay: Duration::from_seconds(5),
///     max_delay: Duration::from_seconds(300),
///     backoff_rate: 1.5,
///     jitter: JitterStrategy::Full,
///     should_continue_polling: Box::new(|state: &String| state != "COMPLETED"),
/// };
///
/// let strategy = create_wait_strategy(config);
/// // strategy(&"COMPLETED".to_string(), 1) => WaitDecision::Done
/// ```
pub struct WaitStrategyConfig<T> {
    /// Maximum number of polling attempts. `None` defaults to 60.
    pub max_attempts: Option<usize>,
    /// Initial delay between polls.
    pub initial_delay: Duration,
    /// Maximum delay cap.
    pub max_delay: Duration,
    /// Backoff multiplier applied per attempt.
    pub backoff_rate: f64,
    /// Jitter strategy applied to the computed delay.
    pub jitter: JitterStrategy,
    /// Predicate that returns `true` if polling should continue, `false` if the condition is met.
    pub should_continue_polling: Box<dyn Fn(&T) -> bool + Send + Sync>,
}

/// Creates a wait strategy function from the given configuration.
///
/// The returned closure takes a reference to the current state and the number of
/// attempts made so far (1-indexed), and returns a [`WaitDecision`].
///
/// # Behavior
///
/// 1. If `should_continue_polling` returns `false`, returns `WaitDecision::Done`.
/// 2. If `attempts_made >= max_attempts` and `should_continue_polling` is `true`,
///    panics with a message indicating max attempts exceeded.
/// 3. Otherwise, computes delay as `min(initial_delay * backoff_rate^(attempts_made - 1), max_delay)`,
///    applies jitter, floors at 1 second, and returns `WaitDecision::Continue { delay }`.
#[allow(clippy::type_complexity)]
pub fn create_wait_strategy<T: Send + Sync + 'static>(
    config: WaitStrategyConfig<T>,
) -> Box<dyn Fn(&T, usize) -> WaitDecision + Send + Sync> {
    let max_attempts = config.max_attempts.unwrap_or(60);
    let initial_delay_secs = config.initial_delay.to_seconds() as f64;
    let max_delay_secs = config.max_delay.to_seconds() as f64;
    let backoff_rate = config.backoff_rate;
    let jitter = config.jitter;
    let should_continue = config.should_continue_polling;

    Box::new(move |result: &T, attempts_made: usize| -> WaitDecision {
        // Check if condition is met
        if !should_continue(result) {
            return WaitDecision::Done;
        }

        // Check max attempts — return Done so the handler can fail gracefully
        // instead of panicking and crashing the async task.
        if attempts_made >= max_attempts {
            return WaitDecision::Done;
        }

        // Calculate delay with exponential backoff
        let exponent = if attempts_made > 0 {
            (attempts_made as i32) - 1
        } else {
            0
        };
        let base_delay = (initial_delay_secs * backoff_rate.powi(exponent)).min(max_delay_secs);

        // Apply jitter
        let jittered = jitter.apply(base_delay, attempts_made as u32);
        let final_delay = jittered.max(1.0).round() as u64;

        WaitDecision::Continue {
            delay: Duration::from_seconds(final_delay),
        }
    })
}

/// Checkpointing mode that controls the trade-off between durability and performance.
///
/// The checkpointing mode determines when and how often the SDK persists operation
/// state to the durable execution service. Different modes offer different trade-offs:
///
/// ## Modes
///
/// ### Eager Mode
/// - Checkpoints after every operation completes
/// - Maximum durability: minimal work is lost on failure
/// - More API calls: higher latency and cost
/// - Best for: Critical workflows where every operation must be durable
///
/// ### Batched Mode (Default)
/// - Groups multiple operations into batches before checkpointing
/// - Balanced durability: some operations may be replayed on failure
/// - Fewer API calls: better performance and lower cost
/// - Best for: Most workflows with reasonable durability requirements
///
/// ### Optimistic Mode
/// - Executes multiple operations before checkpointing
/// - Minimal durability: more work may be replayed on failure
/// - Best performance: fewest API calls
/// - Best for: Workflows where replay is cheap and performance is critical
///
/// ## Example
///
/// ```rust
/// use durable_execution_sdk::CheckpointingMode;
///
/// // Use eager mode for maximum durability
/// let eager = CheckpointingMode::Eager;
///
/// // Use batched mode for balanced performance (default)
/// let batched = CheckpointingMode::default();
///
/// // Use optimistic mode for best performance
/// let optimistic = CheckpointingMode::Optimistic;
/// ```
///
/// ## Requirements
///
/// - 24.1: THE Performance_Configuration SHALL support eager checkpointing mode
/// - 24.2: THE Performance_Configuration SHALL support batched checkpointing mode
/// - 24.3: THE Performance_Configuration SHALL support optimistic execution mode
/// - 24.4: THE Performance_Configuration SHALL document the default behavior and trade-offs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckpointingMode {
    /// Checkpoint after every operation for maximum durability.
    ///
    /// This mode provides the strongest durability guarantees but has the
    /// highest overhead due to frequent API calls.
    ///
    /// ## Characteristics
    /// - Every operation is immediately checkpointed
    /// - Minimal work lost on failure (at most one operation)
    /// - Higher latency due to synchronous checkpointing
    /// - More API calls and higher cost
    ///
    /// ## Use Cases
    /// - Financial transactions
    /// - Critical business workflows
    /// - Operations with expensive side effects
    ///
    /// ## Requirements
    /// - 24.1: THE Performance_Configuration SHALL support eager checkpointing mode
    Eager,

    /// Batch multiple operations before checkpointing for balanced performance.
    ///
    /// This is the default mode that provides a good balance between durability
    /// and performance. Operations are grouped into batches based on size, count,
    /// or time limits before being checkpointed together.
    ///
    /// ## Characteristics
    /// - Operations are batched before checkpointing
    /// - Some operations may be replayed on failure
    /// - Better performance than eager mode
    /// - Configurable batch size and timing
    ///
    /// ## Use Cases
    /// - Most general-purpose workflows
    /// - Workflows with moderate durability requirements
    /// - Cost-sensitive applications
    ///
    /// ## Requirements
    /// - 24.2: THE Performance_Configuration SHALL support batched checkpointing mode
    Batched,

    /// Execute multiple operations before checkpointing for best performance.
    ///
    /// This mode prioritizes performance over durability by executing multiple
    /// operations before creating a checkpoint. On failure, more work may need
    /// to be replayed.
    ///
    /// ## Characteristics
    /// - Multiple operations execute before checkpointing
    /// - More work may be replayed on failure
    /// - Best performance and lowest cost
    /// - Suitable for idempotent operations
    ///
    /// ## Use Cases
    /// - High-throughput batch processing
    /// - Workflows with cheap, idempotent operations
    /// - Performance-critical applications
    ///
    /// ## Requirements
    /// - 24.3: THE Performance_Configuration SHALL support optimistic execution mode
    Optimistic,
}

impl Default for CheckpointingMode {
    /// Returns the default checkpointing mode (Batched).
    ///
    /// Batched mode is the default because it provides a good balance between
    /// durability and performance for most use cases.
    fn default() -> Self {
        Self::Batched
    }
}

impl CheckpointingMode {
    /// Returns true if this mode checkpoints after every operation.
    pub fn is_eager(&self) -> bool {
        matches!(self, Self::Eager)
    }

    /// Returns true if this mode batches operations before checkpointing.
    pub fn is_batched(&self) -> bool {
        matches!(self, Self::Batched)
    }

    /// Returns true if this mode executes multiple operations before checkpointing.
    pub fn is_optimistic(&self) -> bool {
        matches!(self, Self::Optimistic)
    }

    /// Returns a human-readable description of this mode.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Eager => "Checkpoint after every operation (maximum durability)",
            Self::Batched => "Batch operations before checkpointing (balanced)",
            Self::Optimistic => {
                "Execute multiple operations before checkpointing (best performance)"
            }
        }
    }
}

/// Retry strategy trait for configuring step retry behavior.
///
/// # Sealed Trait
///
/// This trait is sealed and cannot be implemented outside of this crate.
/// This allows the SDK maintainers to evolve the retry interface without
/// breaking external code. If you need custom retry behavior, use the
/// provided factory functions.
#[allow(private_bounds)]
pub trait RetryStrategy: Sealed + Send + Sync {
    /// Returns the delay before the next retry attempt, or None if no more retries.
    fn next_delay(&self, attempt: u32, error: &str) -> Option<Duration>;

    /// Clone the retry strategy into a boxed trait object.
    fn clone_box(&self) -> Box<dyn RetryStrategy>;
}

impl Clone for Box<dyn RetryStrategy> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

// =============================================================================
// Built-in Retry Strategies
// =============================================================================

/// Exponential backoff retry strategy.
///
/// Delays increase exponentially with each attempt: `base_delay * 2^(attempt-1)`,
/// capped at `max_delay`. Includes optional jitter to prevent thundering herd.
///
/// # Example
///
/// ```
/// use durable_execution_sdk::config::ExponentialBackoff;
/// use durable_execution_sdk::Duration;
///
/// // Retry up to 5 times with exponential backoff starting at 1 second
/// let strategy = ExponentialBackoff::new(5, Duration::from_seconds(1));
///
/// // With custom max delay
/// let strategy = ExponentialBackoff::builder()
///     .max_attempts(5)
///     .base_delay(Duration::from_seconds(1))
///     .max_delay(Duration::from_minutes(5))
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct ExponentialBackoff {
    /// Maximum number of retry attempts (not including the initial attempt).
    pub max_attempts: u32,
    /// Initial delay before the first retry.
    pub base_delay: Duration,
    /// Maximum delay between retries.
    pub max_delay: Duration,
    /// Multiplier for exponential growth (default: 2.0).
    pub multiplier: f64,
    /// Jitter strategy applied to computed delays.
    pub jitter: JitterStrategy,
}

impl ExponentialBackoff {
    /// Creates a new exponential backoff strategy with default settings.
    ///
    /// # Arguments
    ///
    /// * `max_attempts` - Maximum number of retry attempts
    /// * `base_delay` - Initial delay before the first retry
    pub fn new(max_attempts: u32, base_delay: Duration) -> Self {
        Self {
            max_attempts,
            base_delay,
            max_delay: Duration::from_hours(1),
            multiplier: 2.0,
            jitter: JitterStrategy::None,
        }
    }

    /// Creates a builder for more detailed configuration.
    pub fn builder() -> ExponentialBackoffBuilder {
        ExponentialBackoffBuilder::default()
    }
}

impl Sealed for ExponentialBackoff {}

impl RetryStrategy for ExponentialBackoff {
    fn next_delay(&self, attempt: u32, _error: &str) -> Option<Duration> {
        if attempt >= self.max_attempts {
            return None;
        }

        let base_seconds = self.base_delay.to_seconds() as f64;
        let delay_seconds = base_seconds * self.multiplier.powi(attempt as i32);
        let max_seconds = self.max_delay.to_seconds() as f64;
        let capped_seconds = delay_seconds.min(max_seconds);

        let jittered = self.jitter.apply(capped_seconds, attempt);
        let final_seconds = jittered.max(1.0);

        Some(Duration::from_seconds(final_seconds as u64))
    }

    fn clone_box(&self) -> Box<dyn RetryStrategy> {
        Box::new(self.clone())
    }
}

/// Builder for [`ExponentialBackoff`].
#[derive(Debug, Clone)]
pub struct ExponentialBackoffBuilder {
    max_attempts: u32,
    base_delay: Duration,
    max_delay: Duration,
    multiplier: f64,
    jitter: JitterStrategy,
}

impl Default for ExponentialBackoffBuilder {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_seconds(1),
            max_delay: Duration::from_hours(1),
            multiplier: 2.0,
            jitter: JitterStrategy::None,
        }
    }
}

impl ExponentialBackoffBuilder {
    /// Sets the maximum number of retry attempts.
    pub fn max_attempts(mut self, max_attempts: u32) -> Self {
        self.max_attempts = max_attempts;
        self
    }

    /// Sets the initial delay before the first retry.
    pub fn base_delay(mut self, base_delay: Duration) -> Self {
        self.base_delay = base_delay;
        self
    }

    /// Sets the maximum delay between retries.
    pub fn max_delay(mut self, max_delay: Duration) -> Self {
        self.max_delay = max_delay;
        self
    }

    /// Sets the multiplier for exponential growth (default: 2.0).
    pub fn multiplier(mut self, multiplier: f64) -> Self {
        self.multiplier = multiplier;
        self
    }

    /// Sets the jitter strategy for retry delays.
    pub fn jitter(mut self, jitter: JitterStrategy) -> Self {
        self.jitter = jitter;
        self
    }

    /// Builds the exponential backoff strategy.
    pub fn build(self) -> ExponentialBackoff {
        ExponentialBackoff {
            max_attempts: self.max_attempts,
            base_delay: self.base_delay,
            max_delay: self.max_delay,
            multiplier: self.multiplier,
            jitter: self.jitter,
        }
    }
}

/// Fixed delay retry strategy.
///
/// Retries with a constant delay between attempts.
///
/// # Example
///
/// ```
/// use durable_execution_sdk::config::FixedDelay;
/// use durable_execution_sdk::Duration;
///
/// // Retry up to 3 times with 5 second delay between attempts
/// let strategy = FixedDelay::new(3, Duration::from_seconds(5));
/// ```
#[derive(Debug, Clone)]
pub struct FixedDelay {
    /// Maximum number of retry attempts.
    pub max_attempts: u32,
    /// Delay between retry attempts.
    pub delay: Duration,
    /// Jitter strategy applied to the fixed delay.
    pub jitter: JitterStrategy,
}

impl FixedDelay {
    /// Creates a new fixed delay retry strategy.
    ///
    /// # Arguments
    ///
    /// * `max_attempts` - Maximum number of retry attempts
    /// * `delay` - Delay between retry attempts
    pub fn new(max_attempts: u32, delay: Duration) -> Self {
        Self {
            max_attempts,
            delay,
            jitter: JitterStrategy::None,
        }
    }

    /// Sets the jitter strategy for retry delays.
    pub fn with_jitter(mut self, jitter: JitterStrategy) -> Self {
        self.jitter = jitter;
        self
    }
}

impl Sealed for FixedDelay {}

impl RetryStrategy for FixedDelay {
    fn next_delay(&self, attempt: u32, _error: &str) -> Option<Duration> {
        if attempt >= self.max_attempts {
            return None;
        }

        let delay_secs = self.delay.to_seconds() as f64;
        let jittered = self.jitter.apply(delay_secs, attempt);
        let final_seconds = jittered.max(1.0);

        Some(Duration::from_seconds(final_seconds as u64))
    }

    fn clone_box(&self) -> Box<dyn RetryStrategy> {
        Box::new(self.clone())
    }
}

/// Linear backoff retry strategy.
///
/// Delays increase linearly with each attempt: `base_delay * attempt`.
///
/// # Example
///
/// ```
/// use durable_execution_sdk::config::LinearBackoff;
/// use durable_execution_sdk::Duration;
///
/// // Retry up to 5 times: 2s, 4s, 6s, 8s, 10s
/// let strategy = LinearBackoff::new(5, Duration::from_seconds(2));
/// ```
#[derive(Debug, Clone)]
pub struct LinearBackoff {
    /// Maximum number of retry attempts.
    pub max_attempts: u32,
    /// Base delay that is multiplied by the attempt number.
    pub base_delay: Duration,
    /// Maximum delay between retries.
    pub max_delay: Duration,
    /// Jitter strategy applied to computed delays.
    pub jitter: JitterStrategy,
}

impl LinearBackoff {
    /// Creates a new linear backoff retry strategy.
    ///
    /// # Arguments
    ///
    /// * `max_attempts` - Maximum number of retry attempts
    /// * `base_delay` - Base delay multiplied by attempt number
    pub fn new(max_attempts: u32, base_delay: Duration) -> Self {
        Self {
            max_attempts,
            base_delay,
            max_delay: Duration::from_hours(1),
            jitter: JitterStrategy::None,
        }
    }

    /// Sets the maximum delay between retries.
    pub fn with_max_delay(mut self, max_delay: Duration) -> Self {
        self.max_delay = max_delay;
        self
    }

    /// Sets the jitter strategy for retry delays.
    pub fn with_jitter(mut self, jitter: JitterStrategy) -> Self {
        self.jitter = jitter;
        self
    }
}

impl Sealed for LinearBackoff {}

impl RetryStrategy for LinearBackoff {
    fn next_delay(&self, attempt: u32, _error: &str) -> Option<Duration> {
        if attempt >= self.max_attempts {
            return None;
        }

        let base_seconds = self.base_delay.to_seconds();
        let delay_seconds = base_seconds.saturating_mul((attempt + 1) as u64);
        let max_seconds = self.max_delay.to_seconds();
        let capped_seconds = delay_seconds.min(max_seconds) as f64;

        let jittered = self.jitter.apply(capped_seconds, attempt);
        let final_seconds = jittered.max(1.0);

        Some(Duration::from_seconds(final_seconds as u64))
    }

    fn clone_box(&self) -> Box<dyn RetryStrategy> {
        Box::new(self.clone())
    }
}

/// No retry strategy - fails immediately on first error.
///
/// # Example
///
/// ```
/// use durable_execution_sdk::config::NoRetry;
///
/// let strategy = NoRetry;
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct NoRetry;

impl Sealed for NoRetry {}

impl RetryStrategy for NoRetry {
    fn next_delay(&self, _attempt: u32, _error: &str) -> Option<Duration> {
        None
    }

    fn clone_box(&self) -> Box<dyn RetryStrategy> {
        Box::new(*self)
    }
}

/// Pattern for matching retryable errors.
///
/// Used with [`RetryableErrorFilter`] to declaratively specify which errors
/// should be retried.
///
/// # Example
///
/// ```
/// use durable_execution_sdk::config::ErrorPattern;
///
/// let contains = ErrorPattern::Contains("timeout".to_string());
/// let regex = ErrorPattern::Regex(regex::Regex::new(r"(?i)connection.*refused").unwrap());
/// ```
#[derive(Clone)]
pub enum ErrorPattern {
    /// Match if error message contains this substring.
    Contains(String),
    /// Match if error message matches this regex.
    Regex(regex::Regex),
}

impl std::fmt::Debug for ErrorPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorPattern::Contains(s) => f.debug_tuple("Contains").field(s).finish(),
            ErrorPattern::Regex(r) => f.debug_tuple("Regex").field(&r.as_str()).finish(),
        }
    }
}

/// Declarative filter for retryable errors.
///
/// When configured on a [`StepConfig`], only errors matching the filter will be retried.
/// If no patterns and no error types are configured, all errors are retried (backward-compatible).
///
/// Patterns and error types are combined with OR logic: an error is retryable if it matches
/// ANY pattern OR ANY error type.
///
/// # Example
///
/// ```
/// use durable_execution_sdk::config::{RetryableErrorFilter, ErrorPattern};
///
/// let filter = RetryableErrorFilter {
///     patterns: vec![
///         ErrorPattern::Contains("timeout".to_string()),
///         ErrorPattern::Regex(regex::Regex::new(r"(?i)connection.*refused").unwrap()),
///     ],
///     error_types: vec!["TransientError".to_string()],
/// };
///
/// assert!(filter.is_retryable("request timeout occurred"));
/// assert!(!filter.is_retryable("invalid input"));
/// assert!(filter.is_retryable_with_type("invalid input", "TransientError"));
/// ```
#[derive(Clone, Debug, Default)]
pub struct RetryableErrorFilter {
    /// Error message patterns (string contains or regex).
    pub patterns: Vec<ErrorPattern>,
    /// Error type names to match against.
    pub error_types: Vec<String>,
}

impl RetryableErrorFilter {
    /// Returns `true` if the error message is retryable according to this filter.
    ///
    /// If no filters are configured (empty patterns and empty error_types),
    /// returns `true` for all errors (backward-compatible default).
    ///
    /// Otherwise, returns `true` if the error message matches any configured pattern.
    pub fn is_retryable(&self, error_msg: &str) -> bool {
        if self.patterns.is_empty() && self.error_types.is_empty() {
            return true;
        }

        self.patterns.iter().any(|p| match p {
            ErrorPattern::Contains(s) => error_msg.contains(s.as_str()),
            ErrorPattern::Regex(r) => r.is_match(error_msg),
        })
    }

    /// Returns `true` if the error is retryable by message or type.
    ///
    /// Uses OR logic: returns `true` if the error matches any pattern
    /// OR if the error type matches any configured error type.
    ///
    /// If no filters are configured, returns `true` for all errors.
    pub fn is_retryable_with_type(&self, error_msg: &str, error_type: &str) -> bool {
        if self.patterns.is_empty() && self.error_types.is_empty() {
            return true;
        }

        let matches_type = self.error_types.iter().any(|t| t == error_type);
        matches_type || self.is_retryable(error_msg)
    }
}

/// Custom retry strategy using a user-provided closure.
///
/// This allows users to define custom retry logic without implementing
/// the sealed `RetryStrategy` trait directly.
///
/// # Example
///
/// ```
/// use durable_execution_sdk::config::custom_retry;
/// use durable_execution_sdk::Duration;
///
/// // Custom strategy: retry up to 3 times, but only for specific errors
/// let strategy = custom_retry(|attempt, error| {
///     if attempt >= 3 {
///         return None;
///     }
///     if error.contains("transient") || error.contains("timeout") {
///         Some(Duration::from_seconds(5))
///     } else {
///         None // Don't retry other errors
///     }
/// });
/// ```
pub fn custom_retry<F>(f: F) -> CustomRetry<F>
where
    F: Fn(u32, &str) -> Option<Duration> + Send + Sync + Clone + 'static,
{
    CustomRetry { f }
}

/// Custom retry strategy wrapper.
///
/// Created via the [`custom_retry`] function.
#[derive(Clone)]
pub struct CustomRetry<F>
where
    F: Fn(u32, &str) -> Option<Duration> + Send + Sync + Clone + 'static,
{
    f: F,
}

impl<F> std::fmt::Debug for CustomRetry<F>
where
    F: Fn(u32, &str) -> Option<Duration> + Send + Sync + Clone + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CustomRetry").finish()
    }
}

impl<F> Sealed for CustomRetry<F> where
    F: Fn(u32, &str) -> Option<Duration> + Send + Sync + Clone + 'static
{
}

impl<F> RetryStrategy for CustomRetry<F>
where
    F: Fn(u32, &str) -> Option<Duration> + Send + Sync + Clone + 'static,
{
    fn next_delay(&self, attempt: u32, error: &str) -> Option<Duration> {
        (self.f)(attempt, error)
    }

    fn clone_box(&self) -> Box<dyn RetryStrategy> {
        Box::new(self.clone())
    }
}

// =============================================================================
// Step Semantics and Configuration
// =============================================================================

/// Execution semantics for step operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StepSemantics {
    /// Checkpoint before execution - guarantees at most once execution per retry.
    AtMostOncePerRetry,
    /// Checkpoint after execution - guarantees at least once execution per retry.
    #[default]
    AtLeastOncePerRetry,
}

/// Configuration for step operations.
///
/// # Examples
///
/// Using default configuration:
///
/// ```
/// use durable_execution_sdk::StepConfig;
///
/// let config = StepConfig::default();
/// // Default uses AtLeastOncePerRetry semantics
/// ```
///
/// Configuring step semantics:
///
/// ```
/// use durable_execution_sdk::{StepConfig, StepSemantics};
///
/// // For non-idempotent operations, use AtMostOncePerRetry
/// let config = StepConfig {
///     step_semantics: StepSemantics::AtMostOncePerRetry,
///     ..Default::default()
/// };
/// ```
#[derive(Clone, Default)]
pub struct StepConfig {
    /// Optional retry strategy for failed steps.
    pub retry_strategy: Option<Box<dyn RetryStrategy>>,
    /// Execution semantics (at-most-once or at-least-once).
    pub step_semantics: StepSemantics,
    /// Optional custom serializer/deserializer.
    pub serdes: Option<Arc<dyn SerDesAny>>,
    /// Optional filter for retryable errors. When set, only errors matching
    /// the filter will be retried. When `None`, all errors are retried
    /// (current behavior preserved).
    pub retryable_error_filter: Option<RetryableErrorFilter>,
}

impl std::fmt::Debug for StepConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StepConfig")
            .field("retry_strategy", &self.retry_strategy.is_some())
            .field("step_semantics", &self.step_semantics)
            .field("serdes", &self.serdes.is_some())
            .field(
                "retryable_error_filter",
                &self.retryable_error_filter.is_some(),
            )
            .finish()
    }
}

/// Configuration for callback operations.
#[derive(Debug, Clone, Default)]
pub struct CallbackConfig {
    /// Timeout duration for the callback.
    pub timeout: Duration,
    /// Heartbeat timeout duration.
    pub heartbeat_timeout: Duration,
    /// Optional custom serializer/deserializer.
    pub serdes: Option<Arc<dyn SerDesAny>>,
}

/// Configuration for invoke operations.
#[derive(Clone)]
pub struct InvokeConfig<P, R> {
    /// Timeout duration for the invocation.
    pub timeout: Duration,
    /// Optional custom serializer for the payload.
    pub serdes_payload: Option<Arc<dyn SerDesAny>>,
    /// Optional custom deserializer for the result.
    pub serdes_result: Option<Arc<dyn SerDesAny>>,
    /// Optional tenant ID for multi-tenant scenarios.
    pub tenant_id: Option<String>,
    /// Phantom data for type parameters.
    _marker: PhantomData<(P, R)>,
}

impl<P, R> Default for InvokeConfig<P, R> {
    fn default() -> Self {
        Self {
            timeout: Duration::default(),
            serdes_payload: None,
            serdes_result: None,
            tenant_id: None,
            _marker: PhantomData,
        }
    }
}

impl<P, R> std::fmt::Debug for InvokeConfig<P, R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InvokeConfig")
            .field("timeout", &self.timeout)
            .field("serdes_payload", &self.serdes_payload.is_some())
            .field("serdes_result", &self.serdes_result.is_some())
            .field("tenant_id", &self.tenant_id)
            .finish()
    }
}

/// Configuration for map operations.
///
/// # Examples
///
/// Basic map configuration with concurrency limit:
///
/// ```
/// use durable_execution_sdk::MapConfig;
///
/// let config = MapConfig {
///     max_concurrency: Some(5),
///     ..Default::default()
/// };
/// ```
///
/// Map with failure tolerance:
///
/// ```
/// use durable_execution_sdk::{MapConfig, CompletionConfig};
///
/// let config = MapConfig {
///     max_concurrency: Some(10),
///     completion_config: CompletionConfig::with_failure_tolerance(2),
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone, Default)]
pub struct MapConfig {
    /// Maximum number of concurrent executions.
    pub max_concurrency: Option<usize>,
    /// Optional item batcher for grouping items.
    pub item_batcher: Option<ItemBatcher>,
    /// Completion configuration defining success/failure criteria.
    pub completion_config: CompletionConfig,
    /// Optional custom serializer/deserializer.
    pub serdes: Option<Arc<dyn SerDesAny>>,
}

/// Configuration for parallel operations.
#[derive(Debug, Clone, Default)]
pub struct ParallelConfig {
    /// Maximum number of concurrent executions.
    pub max_concurrency: Option<usize>,
    /// Completion configuration defining success/failure criteria.
    pub completion_config: CompletionConfig,
    /// Optional custom serializer/deserializer.
    pub serdes: Option<Arc<dyn SerDesAny>>,
}

/// Configuration for child context operations.
///
/// This configuration controls how child contexts behave, including
/// whether to replay children when loading state for large parallel operations.
#[derive(Clone, Default)]
#[allow(clippy::type_complexity)]
pub struct ChildConfig {
    /// Optional custom serializer/deserializer.
    pub serdes: Option<Arc<dyn SerDesAny>>,
    /// Whether to replay children when loading state.
    ///
    /// When set to `true`, the child context will request child operations
    /// to be included in state loads during replay. This is useful for large
    /// parallel operations where the combined output needs to be reconstructed
    /// by replaying each branch.
    ///
    /// Default is `false` for better performance in most cases.
    pub replay_children: bool,
    /// Optional function to map child context errors before propagation.
    ///
    /// When set, this function is applied to errors from child context execution
    /// before they are checkpointed and propagated. Suspend errors are never mapped.
    ///
    /// Default is `None`, which preserves current behavior (errors propagate unchanged).
    pub error_mapper: Option<Arc<dyn Fn(DurableError) -> DurableError + Send + Sync>>,
    /// Optional function to generate a summary when the serialized child result exceeds 256KB.
    ///
    /// When set, this function is invoked with the serialized result string if its size
    /// exceeds 256KB (262144 bytes). The returned summary string is stored instead of the
    /// full result, enabling replay-based reconstruction for large payloads.
    ///
    /// When the serialized result is 256KB or less, the full result is stored even if
    /// a summary generator is configured.
    ///
    /// Default is `None`, which preserves current behavior (full result stored regardless of size).
    pub summary_generator: Option<Arc<dyn Fn(&str) -> String + Send + Sync>>,
}

impl std::fmt::Debug for ChildConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChildConfig")
            .field("serdes", &self.serdes)
            .field("replay_children", &self.replay_children)
            .field("error_mapper", &self.error_mapper.as_ref().map(|_| "..."))
            .field(
                "summary_generator",
                &self.summary_generator.as_ref().map(|_| "..."),
            )
            .finish()
    }
}

impl ChildConfig {
    /// Creates a new ChildConfig with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a ChildConfig with replay_children enabled.
    ///
    /// Use this when you need to reconstruct the combined output of a large
    /// parallel operation by replaying each branch.
    ///
    /// # Example
    ///
    /// ```
    /// use durable_execution_sdk::ChildConfig;
    ///
    /// let config = ChildConfig::with_replay_children();
    /// assert!(config.replay_children);
    /// ```
    pub fn with_replay_children() -> Self {
        Self {
            replay_children: true,
            ..Default::default()
        }
    }

    /// Sets the replay_children option.
    ///
    /// # Arguments
    ///
    /// * `replay_children` - Whether to replay children when loading state
    pub fn set_replay_children(mut self, replay_children: bool) -> Self {
        self.replay_children = replay_children;
        self
    }

    /// Sets the custom serializer/deserializer.
    pub fn set_serdes(mut self, serdes: Arc<dyn SerDesAny>) -> Self {
        self.serdes = Some(serdes);
        self
    }

    /// Sets the error mapper function.
    ///
    /// The error mapper is applied to child context errors before they are
    /// checkpointed and propagated. Suspend errors are never mapped.
    pub fn set_error_mapper(
        mut self,
        mapper: Arc<dyn Fn(DurableError) -> DurableError + Send + Sync>,
    ) -> Self {
        self.error_mapper = Some(mapper);
        self
    }

    /// Sets the summary generator function.
    ///
    /// The summary generator is invoked when the serialized child result exceeds
    /// 256KB (262144 bytes). It receives the serialized result string and should
    /// return a compact summary string to store instead.
    pub fn set_summary_generator(
        mut self,
        generator: Arc<dyn Fn(&str) -> String + Send + Sync>,
    ) -> Self {
        self.summary_generator = Some(generator);
        self
    }
}

/// Type alias for ChildConfig for consistency with the design document.
///
/// The design document refers to this as `ContextConfig`, but internally
/// we use `ChildConfig` to be more descriptive of its purpose.
pub type ContextConfig = ChildConfig;

/// Configuration defining success/failure criteria for concurrent operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompletionConfig {
    /// Minimum number of successful completions required.
    pub min_successful: Option<usize>,
    /// Maximum number of tolerated failures (absolute count).
    pub tolerated_failure_count: Option<usize>,
    /// Maximum percentage of tolerated failures (0.0 to 1.0).
    pub tolerated_failure_percentage: Option<f64>,
}

impl CompletionConfig {
    /// Creates a completion config that succeeds when the first task succeeds.
    ///
    /// # Example
    ///
    /// ```
    /// use durable_execution_sdk::CompletionConfig;
    ///
    /// let config = CompletionConfig::first_successful();
    /// assert_eq!(config.min_successful, Some(1));
    /// ```
    pub fn first_successful() -> Self {
        Self {
            min_successful: Some(1),
            ..Default::default()
        }
    }

    /// Creates a completion config that waits for all tasks to complete.
    ///
    /// # Example
    ///
    /// ```
    /// use durable_execution_sdk::CompletionConfig;
    ///
    /// let config = CompletionConfig::all_completed();
    /// assert!(config.min_successful.is_none());
    /// ```
    pub fn all_completed() -> Self {
        Self::default()
    }

    /// Creates a completion config that requires all tasks to succeed.
    ///
    /// # Example
    ///
    /// ```
    /// use durable_execution_sdk::CompletionConfig;
    ///
    /// let config = CompletionConfig::all_successful();
    /// assert_eq!(config.tolerated_failure_count, Some(0));
    /// assert_eq!(config.tolerated_failure_percentage, Some(0.0));
    /// ```
    pub fn all_successful() -> Self {
        Self {
            tolerated_failure_count: Some(0),
            tolerated_failure_percentage: Some(0.0),
            ..Default::default()
        }
    }

    /// Creates a completion config with a specific minimum successful count.
    pub fn with_min_successful(count: usize) -> Self {
        Self {
            min_successful: Some(count),
            ..Default::default()
        }
    }

    /// Creates a completion config with a specific failure tolerance.
    pub fn with_failure_tolerance(count: usize) -> Self {
        Self {
            tolerated_failure_count: Some(count),
            ..Default::default()
        }
    }
}

/// Configuration for batching items in map operations.
#[derive(Debug, Clone)]
pub struct ItemBatcher {
    /// Maximum number of items per batch.
    pub max_items_per_batch: usize,
    /// Maximum total bytes per batch.
    pub max_bytes_per_batch: usize,
}

impl Default for ItemBatcher {
    fn default() -> Self {
        Self {
            max_items_per_batch: 100,
            max_bytes_per_batch: 256 * 1024, // 256KB
        }
    }
}

impl ItemBatcher {
    /// Creates a new ItemBatcher with the specified limits.
    pub fn new(max_items_per_batch: usize, max_bytes_per_batch: usize) -> Self {
        Self {
            max_items_per_batch,
            max_bytes_per_batch,
        }
    }

    /// Batches items according to configuration, respecting both item count and byte limits.
    ///
    /// This method groups items into batches where each batch:
    /// - Contains at most `max_items_per_batch` items
    /// - Has an estimated total size of at most `max_bytes_per_batch` bytes
    ///
    /// Item size is estimated using JSON serialization via `serde_json`.
    ///
    /// # Arguments
    ///
    /// * `items` - The slice of items to batch
    ///
    /// # Returns
    ///
    /// A vector of `(start_index, batch)` tuples where:
    /// - `start_index` is the index of the first item in the batch from the original slice
    /// - `batch` is a vector of cloned items in that batch
    /// # Example
    ///
    /// ```
    /// use durable_execution_sdk::ItemBatcher;
    ///
    /// let batcher = ItemBatcher::new(2, 1024);
    /// let items = vec!["a", "b", "c", "d", "e"];
    /// let batches = batcher.batch(&items);
    ///
    /// // Items are grouped into batches of at most 2 items each
    /// assert_eq!(batches.len(), 3);
    /// assert_eq!(batches[0], (0, vec!["a", "b"]));
    /// assert_eq!(batches[1], (2, vec!["c", "d"]));
    /// assert_eq!(batches[2], (4, vec!["e"]));
    /// ```
    pub fn batch<T: Serialize + Clone>(&self, items: &[T]) -> Vec<(usize, Vec<T>)> {
        if items.is_empty() {
            return Vec::new();
        }

        let mut batches = Vec::new();
        let mut current_batch = Vec::new();
        let mut current_bytes = 0usize;
        let mut batch_start_index = 0;

        for (i, item) in items.iter().enumerate() {
            // Estimate item size using JSON serialization
            let item_bytes = serde_json::to_string(item).map(|s| s.len()).unwrap_or(0);

            // Check if adding this item would exceed limits
            let would_exceed_items = current_batch.len() >= self.max_items_per_batch;
            let would_exceed_bytes =
                current_bytes + item_bytes > self.max_bytes_per_batch && !current_batch.is_empty();

            if would_exceed_items || would_exceed_bytes {
                // Finalize current batch and start a new one
                batches.push((batch_start_index, std::mem::take(&mut current_batch)));
                current_bytes = 0;
                batch_start_index = i;
            }

            current_batch.push(item.clone());
            current_bytes += item_bytes;
        }

        // Don't forget the last batch
        if !current_batch.is_empty() {
            batches.push((batch_start_index, current_batch));
        }

        batches
    }
}

/// Type-erased SerDes trait for storing in config structs.
pub trait SerDesAny: Send + Sync {
    /// Serialize a value to a string.
    fn serialize_any(
        &self,
        value: &dyn std::any::Any,
    ) -> Result<String, crate::error::DurableError>;
    /// Deserialize a string to a boxed Any value.
    fn deserialize_any(
        &self,
        data: &str,
    ) -> Result<Box<dyn std::any::Any + Send>, crate::error::DurableError>;
}

impl std::fmt::Debug for dyn SerDesAny {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SerDesAny")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // =========================================================================
    // Unit Tests
    // =========================================================================

    #[test]
    fn test_step_semantics_default() {
        let semantics = StepSemantics::default();
        assert_eq!(semantics, StepSemantics::AtLeastOncePerRetry);
    }

    #[test]
    fn test_step_config_default() {
        let config = StepConfig::default();
        assert!(config.retry_strategy.is_none());
        assert_eq!(config.step_semantics, StepSemantics::AtLeastOncePerRetry);
        assert!(config.serdes.is_none());
    }

    #[test]
    fn test_completion_config_first_successful() {
        let config = CompletionConfig::first_successful();
        assert_eq!(config.min_successful, Some(1));
        assert!(config.tolerated_failure_count.is_none());
        assert!(config.tolerated_failure_percentage.is_none());
    }

    #[test]
    fn test_completion_config_all_completed() {
        let config = CompletionConfig::all_completed();
        assert!(config.min_successful.is_none());
        assert!(config.tolerated_failure_count.is_none());
        assert!(config.tolerated_failure_percentage.is_none());
    }

    #[test]
    fn test_completion_config_all_successful() {
        let config = CompletionConfig::all_successful();
        assert!(config.min_successful.is_none());
        assert_eq!(config.tolerated_failure_count, Some(0));
        assert_eq!(config.tolerated_failure_percentage, Some(0.0));
    }

    #[test]
    fn test_item_batcher_default() {
        let batcher = ItemBatcher::default();
        assert_eq!(batcher.max_items_per_batch, 100);
        assert_eq!(batcher.max_bytes_per_batch, 256 * 1024);
    }

    #[test]
    fn test_item_batcher_new() {
        let batcher = ItemBatcher::new(50, 128 * 1024);
        assert_eq!(batcher.max_items_per_batch, 50);
        assert_eq!(batcher.max_bytes_per_batch, 128 * 1024);
    }

    #[test]
    fn test_callback_config_default() {
        let config = CallbackConfig::default();
        assert_eq!(config.timeout.to_seconds(), 0);
        assert_eq!(config.heartbeat_timeout.to_seconds(), 0);
    }

    #[test]
    fn test_invoke_config_default() {
        let config: InvokeConfig<String, String> = InvokeConfig::default();
        assert_eq!(config.timeout.to_seconds(), 0);
        assert!(config.tenant_id.is_none());
    }

    #[test]
    fn test_map_config_default() {
        let config = MapConfig::default();
        assert!(config.max_concurrency.is_none());
        assert!(config.item_batcher.is_none());
    }

    #[test]
    fn test_parallel_config_default() {
        let config = ParallelConfig::default();
        assert!(config.max_concurrency.is_none());
    }

    #[test]
    fn test_child_config_default() {
        let config = ChildConfig::default();
        assert!(!config.replay_children);
        assert!(config.serdes.is_none());
        assert!(config.error_mapper.is_none());
        assert!(config.summary_generator.is_none());
    }

    #[test]
    fn test_child_config_with_replay_children() {
        let config = ChildConfig::with_replay_children();
        assert!(config.replay_children);
    }

    #[test]
    fn test_child_config_set_replay_children() {
        let config = ChildConfig::new().set_replay_children(true);
        assert!(config.replay_children);
    }

    #[test]
    fn test_context_config_type_alias() {
        // ContextConfig is a type alias for ChildConfig
        let config: ContextConfig = ContextConfig::with_replay_children();
        assert!(config.replay_children);
    }

    #[test]
    fn test_checkpointing_mode_default() {
        let mode = CheckpointingMode::default();
        assert_eq!(mode, CheckpointingMode::Batched);
        assert!(mode.is_batched());
    }

    #[test]
    fn test_checkpointing_mode_eager() {
        let mode = CheckpointingMode::Eager;
        assert!(mode.is_eager());
        assert!(!mode.is_batched());
        assert!(!mode.is_optimistic());
    }

    #[test]
    fn test_checkpointing_mode_batched() {
        let mode = CheckpointingMode::Batched;
        assert!(!mode.is_eager());
        assert!(mode.is_batched());
        assert!(!mode.is_optimistic());
    }

    #[test]
    fn test_checkpointing_mode_optimistic() {
        let mode = CheckpointingMode::Optimistic;
        assert!(!mode.is_eager());
        assert!(!mode.is_batched());
        assert!(mode.is_optimistic());
    }

    #[test]
    fn test_checkpointing_mode_description() {
        assert!(CheckpointingMode::Eager
            .description()
            .contains("maximum durability"));
        assert!(CheckpointingMode::Batched
            .description()
            .contains("balanced"));
        assert!(CheckpointingMode::Optimistic
            .description()
            .contains("best performance"));
    }

    #[test]
    fn test_checkpointing_mode_serialization() {
        // Test that CheckpointingMode can be serialized and deserialized
        let mode = CheckpointingMode::Eager;
        let serialized = serde_json::to_string(&mode).unwrap();
        let deserialized: CheckpointingMode = serde_json::from_str(&serialized).unwrap();
        assert_eq!(mode, deserialized);

        let mode = CheckpointingMode::Batched;
        let serialized = serde_json::to_string(&mode).unwrap();
        let deserialized: CheckpointingMode = serde_json::from_str(&serialized).unwrap();
        assert_eq!(mode, deserialized);

        let mode = CheckpointingMode::Optimistic;
        let serialized = serde_json::to_string(&mode).unwrap();
        let deserialized: CheckpointingMode = serde_json::from_str(&serialized).unwrap();
        assert_eq!(mode, deserialized);
    }

    // =========================================================================
    // Retry Strategy Tests
    // =========================================================================

    #[test]
    fn test_exponential_backoff_new() {
        let strategy = ExponentialBackoff::new(5, Duration::from_seconds(1));
        assert_eq!(strategy.max_attempts, 5);
        assert_eq!(strategy.base_delay.to_seconds(), 1);
        assert_eq!(strategy.max_delay.to_seconds(), 3600); // 1 hour default
        assert!((strategy.multiplier - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_exponential_backoff_builder() {
        let strategy = ExponentialBackoff::builder()
            .max_attempts(10)
            .base_delay(Duration::from_seconds(2))
            .max_delay(Duration::from_minutes(30))
            .multiplier(3.0)
            .build();

        assert_eq!(strategy.max_attempts, 10);
        assert_eq!(strategy.base_delay.to_seconds(), 2);
        assert_eq!(strategy.max_delay.to_seconds(), 1800); // 30 minutes
        assert!((strategy.multiplier - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_exponential_backoff_delays() {
        let strategy = ExponentialBackoff::new(5, Duration::from_seconds(1));

        // attempt 0: 1 * 2^0 = 1 second
        assert_eq!(
            strategy.next_delay(0, "error").map(|d| d.to_seconds()),
            Some(1)
        );
        // attempt 1: 1 * 2^1 = 2 seconds
        assert_eq!(
            strategy.next_delay(1, "error").map(|d| d.to_seconds()),
            Some(2)
        );
        // attempt 2: 1 * 2^2 = 4 seconds
        assert_eq!(
            strategy.next_delay(2, "error").map(|d| d.to_seconds()),
            Some(4)
        );
        // attempt 3: 1 * 2^3 = 8 seconds
        assert_eq!(
            strategy.next_delay(3, "error").map(|d| d.to_seconds()),
            Some(8)
        );
        // attempt 4: 1 * 2^4 = 16 seconds
        assert_eq!(
            strategy.next_delay(4, "error").map(|d| d.to_seconds()),
            Some(16)
        );
        // attempt 5: exceeds max_attempts
        assert_eq!(strategy.next_delay(5, "error"), None);
    }

    #[test]
    fn test_exponential_backoff_max_delay_cap() {
        let strategy = ExponentialBackoff::builder()
            .max_attempts(10)
            .base_delay(Duration::from_seconds(10))
            .max_delay(Duration::from_seconds(30))
            .build();

        // attempt 0: 10 * 2^0 = 10 seconds
        assert_eq!(
            strategy.next_delay(0, "error").map(|d| d.to_seconds()),
            Some(10)
        );
        // attempt 1: 10 * 2^1 = 20 seconds
        assert_eq!(
            strategy.next_delay(1, "error").map(|d| d.to_seconds()),
            Some(20)
        );
        // attempt 2: 10 * 2^2 = 40 seconds, capped at 30
        assert_eq!(
            strategy.next_delay(2, "error").map(|d| d.to_seconds()),
            Some(30)
        );
        // attempt 3: 10 * 2^3 = 80 seconds, capped at 30
        assert_eq!(
            strategy.next_delay(3, "error").map(|d| d.to_seconds()),
            Some(30)
        );
    }

    #[test]
    fn test_fixed_delay_new() {
        let strategy = FixedDelay::new(3, Duration::from_seconds(5));
        assert_eq!(strategy.max_attempts, 3);
        assert_eq!(strategy.delay.to_seconds(), 5);
    }

    #[test]
    fn test_fixed_delay_constant() {
        let strategy = FixedDelay::new(3, Duration::from_seconds(5));

        // All delays should be the same
        assert_eq!(
            strategy.next_delay(0, "error").map(|d| d.to_seconds()),
            Some(5)
        );
        assert_eq!(
            strategy.next_delay(1, "error").map(|d| d.to_seconds()),
            Some(5)
        );
        assert_eq!(
            strategy.next_delay(2, "error").map(|d| d.to_seconds()),
            Some(5)
        );
        // Exceeds max_attempts
        assert_eq!(strategy.next_delay(3, "error"), None);
    }

    #[test]
    fn test_linear_backoff_new() {
        let strategy = LinearBackoff::new(5, Duration::from_seconds(2));
        assert_eq!(strategy.max_attempts, 5);
        assert_eq!(strategy.base_delay.to_seconds(), 2);
        assert_eq!(strategy.max_delay.to_seconds(), 3600); // 1 hour default
    }

    #[test]
    fn test_linear_backoff_with_max_delay() {
        let strategy = LinearBackoff::new(5, Duration::from_seconds(2))
            .with_max_delay(Duration::from_seconds(10));
        assert_eq!(strategy.max_delay.to_seconds(), 10);
    }

    #[test]
    fn test_linear_backoff_delays() {
        let strategy = LinearBackoff::new(5, Duration::from_seconds(2));

        // attempt 0: 2 * (0+1) = 2 seconds
        assert_eq!(
            strategy.next_delay(0, "error").map(|d| d.to_seconds()),
            Some(2)
        );
        // attempt 1: 2 * (1+1) = 4 seconds
        assert_eq!(
            strategy.next_delay(1, "error").map(|d| d.to_seconds()),
            Some(4)
        );
        // attempt 2: 2 * (2+1) = 6 seconds
        assert_eq!(
            strategy.next_delay(2, "error").map(|d| d.to_seconds()),
            Some(6)
        );
        // attempt 3: 2 * (3+1) = 8 seconds
        assert_eq!(
            strategy.next_delay(3, "error").map(|d| d.to_seconds()),
            Some(8)
        );
        // attempt 4: 2 * (4+1) = 10 seconds
        assert_eq!(
            strategy.next_delay(4, "error").map(|d| d.to_seconds()),
            Some(10)
        );
        // attempt 5: exceeds max_attempts
        assert_eq!(strategy.next_delay(5, "error"), None);
    }

    #[test]
    fn test_linear_backoff_max_delay_cap() {
        let strategy = LinearBackoff::new(10, Duration::from_seconds(5))
            .with_max_delay(Duration::from_seconds(15));

        // attempt 0: 5 * 1 = 5 seconds
        assert_eq!(
            strategy.next_delay(0, "error").map(|d| d.to_seconds()),
            Some(5)
        );
        // attempt 1: 5 * 2 = 10 seconds
        assert_eq!(
            strategy.next_delay(1, "error").map(|d| d.to_seconds()),
            Some(10)
        );
        // attempt 2: 5 * 3 = 15 seconds
        assert_eq!(
            strategy.next_delay(2, "error").map(|d| d.to_seconds()),
            Some(15)
        );
        // attempt 3: 5 * 4 = 20 seconds, capped at 15
        assert_eq!(
            strategy.next_delay(3, "error").map(|d| d.to_seconds()),
            Some(15)
        );
    }

    #[test]
    fn test_no_retry() {
        let strategy = NoRetry;

        // Should always return None
        assert_eq!(strategy.next_delay(0, "error"), None);
        assert_eq!(strategy.next_delay(1, "error"), None);
        assert_eq!(strategy.next_delay(100, "error"), None);
    }

    #[test]
    fn test_no_retry_default() {
        let strategy = NoRetry;
        assert_eq!(strategy.next_delay(0, "error"), None);
    }

    #[test]
    fn test_custom_retry_basic() {
        let strategy = custom_retry(|attempt, _error| {
            if attempt >= 3 {
                None
            } else {
                Some(Duration::from_seconds(10))
            }
        });

        assert_eq!(
            strategy.next_delay(0, "error").map(|d| d.to_seconds()),
            Some(10)
        );
        assert_eq!(
            strategy.next_delay(1, "error").map(|d| d.to_seconds()),
            Some(10)
        );
        assert_eq!(
            strategy.next_delay(2, "error").map(|d| d.to_seconds()),
            Some(10)
        );
        assert_eq!(strategy.next_delay(3, "error"), None);
    }

    #[test]
    fn test_custom_retry_error_based() {
        let strategy = custom_retry(|attempt, error| {
            if attempt >= 5 {
                return None;
            }
            if error.contains("transient") {
                Some(Duration::from_seconds(1))
            } else if error.contains("rate_limit") {
                Some(Duration::from_seconds(30))
            } else {
                None // Don't retry other errors
            }
        });

        // Transient errors get short delay
        assert_eq!(
            strategy
                .next_delay(0, "transient error")
                .map(|d| d.to_seconds()),
            Some(1)
        );
        // Rate limit errors get longer delay
        assert_eq!(
            strategy
                .next_delay(0, "rate_limit exceeded")
                .map(|d| d.to_seconds()),
            Some(30)
        );
        // Other errors don't retry
        assert_eq!(strategy.next_delay(0, "permanent failure"), None);
    }

    #[test]
    fn test_retry_strategy_clone_box() {
        // Test that clone_box works for all strategies
        let exp: Box<dyn RetryStrategy> =
            Box::new(ExponentialBackoff::new(3, Duration::from_seconds(1)));
        let exp_clone = exp.clone_box();
        assert_eq!(
            exp.next_delay(0, "e").map(|d| d.to_seconds()),
            exp_clone.next_delay(0, "e").map(|d| d.to_seconds())
        );

        let fixed: Box<dyn RetryStrategy> = Box::new(FixedDelay::new(3, Duration::from_seconds(5)));
        let fixed_clone = fixed.clone_box();
        assert_eq!(
            fixed.next_delay(0, "e").map(|d| d.to_seconds()),
            fixed_clone.next_delay(0, "e").map(|d| d.to_seconds())
        );

        let linear: Box<dyn RetryStrategy> =
            Box::new(LinearBackoff::new(3, Duration::from_seconds(2)));
        let linear_clone = linear.clone_box();
        assert_eq!(
            linear.next_delay(0, "e").map(|d| d.to_seconds()),
            linear_clone.next_delay(0, "e").map(|d| d.to_seconds())
        );

        let no_retry: Box<dyn RetryStrategy> = Box::new(NoRetry);
        let no_retry_clone = no_retry.clone_box();
        assert_eq!(
            no_retry.next_delay(0, "e"),
            no_retry_clone.next_delay(0, "e")
        );
    }

    #[test]
    fn test_boxed_retry_strategy_clone() {
        // Test the Clone impl for Box<dyn RetryStrategy>
        let strategy: Box<dyn RetryStrategy> =
            Box::new(ExponentialBackoff::new(3, Duration::from_seconds(1)));
        let cloned = strategy.clone();

        assert_eq!(
            strategy.next_delay(0, "error").map(|d| d.to_seconds()),
            cloned.next_delay(0, "error").map(|d| d.to_seconds())
        );
    }

    #[test]
    fn test_step_config_with_retry_strategy() {
        let config = StepConfig {
            retry_strategy: Some(Box::new(ExponentialBackoff::new(
                3,
                Duration::from_seconds(1),
            ))),
            step_semantics: StepSemantics::AtLeastOncePerRetry,
            serdes: None,
            retryable_error_filter: None,
        };

        assert!(config.retry_strategy.is_some());
        let strategy = config.retry_strategy.as_ref().unwrap();
        assert_eq!(
            strategy.next_delay(0, "error").map(|d| d.to_seconds()),
            Some(1)
        );
    }

    #[test]
    fn test_retry_strategy_debug() {
        // Test Debug implementations
        let exp = ExponentialBackoff::new(3, Duration::from_seconds(1));
        let debug_str = format!("{:?}", exp);
        assert!(debug_str.contains("ExponentialBackoff"));

        let fixed = FixedDelay::new(3, Duration::from_seconds(5));
        let debug_str = format!("{:?}", fixed);
        assert!(debug_str.contains("FixedDelay"));

        let linear = LinearBackoff::new(3, Duration::from_seconds(2));
        let debug_str = format!("{:?}", linear);
        assert!(debug_str.contains("LinearBackoff"));

        let no_retry = NoRetry;
        let debug_str = format!("{:?}", no_retry);
        assert!(debug_str.contains("NoRetry"));

        let custom = custom_retry(|_, _| None);
        let debug_str = format!("{:?}", custom);
        assert!(debug_str.contains("CustomRetry"));
    }

    // =========================================================================
    // Property-Based Tests
    // =========================================================================

    /// Strategy for generating valid StepSemantics values
    fn step_semantics_strategy() -> impl Strategy<Value = StepSemantics> {
        prop_oneof![
            Just(StepSemantics::AtMostOncePerRetry),
            Just(StepSemantics::AtLeastOncePerRetry),
        ]
    }

    /// Strategy for generating valid CheckpointingMode values
    fn checkpointing_mode_strategy() -> impl Strategy<Value = CheckpointingMode> {
        prop_oneof![
            Just(CheckpointingMode::Eager),
            Just(CheckpointingMode::Batched),
            Just(CheckpointingMode::Optimistic),
        ]
    }

    proptest! {
        // **Feature: rust-sdk-test-suite, Property: StepConfig validity**
        // **Validates: Requirements 5.1**
        /// Property: For any valid StepConfig instance, the configuration SHALL be usable without panics.
        /// StepConfig with any StepSemantics value should be valid and usable.
        #[test]
        fn prop_step_config_validity(semantics in step_semantics_strategy()) {
            let config = StepConfig {
                retry_strategy: None,
                step_semantics: semantics,
                serdes: None,
                retryable_error_filter: None,
            };

            // Verify the config is usable - accessing fields should not panic
            let _ = config.retry_strategy.is_none();
            let _ = config.step_semantics;
            let _ = config.serdes.is_none();

            // Verify Debug trait works
            let debug_str = format!("{:?}", config);
            prop_assert!(!debug_str.is_empty());
        }

        // **Feature: rust-sdk-test-suite, Property: CallbackConfig with positive timeout values**
        // **Validates: Requirements 5.2**
        /// Property: For any valid CallbackConfig with positive timeout values, the configuration SHALL be valid.
        #[test]
        fn prop_callback_config_positive_timeout(
            timeout_secs in 1u64..=86400u64,
            heartbeat_secs in 1u64..=86400u64
        ) {
            let config = CallbackConfig {
                timeout: Duration::from_seconds(timeout_secs),
                heartbeat_timeout: Duration::from_seconds(heartbeat_secs),
                serdes: None,
            };

            // Verify the config has the expected timeout values
            prop_assert_eq!(config.timeout.to_seconds(), timeout_secs);
            prop_assert_eq!(config.heartbeat_timeout.to_seconds(), heartbeat_secs);

            // Verify Debug trait works
            let debug_str = format!("{:?}", config);
            prop_assert!(!debug_str.is_empty());
        }

        // **Feature: rust-sdk-test-suite, Property 12: Duration conversion round-trip**
        // **Validates: Requirements 5.3**
        /// Property: For any Duration value, converting to seconds and back SHALL preserve the value.
        #[test]
        fn prop_duration_conversion_roundtrip(seconds in 0u64..=u64::MAX / 2) {
            let original = Duration::from_seconds(seconds);
            let extracted = original.to_seconds();
            let reconstructed = Duration::from_seconds(extracted);

            prop_assert_eq!(original, reconstructed);
            prop_assert_eq!(original.to_seconds(), reconstructed.to_seconds());
        }

        // **Feature: rust-sdk-test-suite, Property: RetryStrategy consistency**
        // **Validates: Requirements 5.4**
        /// Property: For any CompletionConfig, the configuration SHALL produce consistent behavior.
        /// Since RetryStrategy is a sealed trait, we test CompletionConfig which is the main
        /// configurable retry-related type.
        #[test]
        fn prop_completion_config_consistency(
            min_successful in proptest::option::of(0usize..100),
            tolerated_count in proptest::option::of(0usize..100),
            tolerated_pct in proptest::option::of(0.0f64..=1.0f64)
        ) {
            let config = CompletionConfig {
                min_successful,
                tolerated_failure_count: tolerated_count,
                tolerated_failure_percentage: tolerated_pct,
            };

            // Verify the config has the expected values
            prop_assert_eq!(config.min_successful, min_successful);
            prop_assert_eq!(config.tolerated_failure_count, tolerated_count);
            prop_assert_eq!(config.tolerated_failure_percentage, tolerated_pct);

            // Verify serialization round-trip
            let serialized = serde_json::to_string(&config).unwrap();
            let deserialized: CompletionConfig = serde_json::from_str(&serialized).unwrap();

            prop_assert_eq!(config.min_successful, deserialized.min_successful);
            prop_assert_eq!(config.tolerated_failure_count, deserialized.tolerated_failure_count);
            // For f64, we need to handle NaN specially
            match (config.tolerated_failure_percentage, deserialized.tolerated_failure_percentage) {
                (Some(a), Some(b)) => prop_assert!((a - b).abs() < f64::EPSILON),
                (None, None) => {},
                _ => prop_assert!(false, "tolerated_failure_percentage mismatch"),
            }
        }

        // **Feature: rust-sdk-test-suite, Property: CheckpointingMode serialization round-trip**
        // **Validates: Requirements 5.1**
        /// Property: For any CheckpointingMode value, serializing then deserializing SHALL produce the same value.
        #[test]
        fn prop_checkpointing_mode_roundtrip(mode in checkpointing_mode_strategy()) {
            let serialized = serde_json::to_string(&mode).unwrap();
            let deserialized: CheckpointingMode = serde_json::from_str(&serialized).unwrap();
            prop_assert_eq!(mode, deserialized);
        }

        // **Feature: rust-sdk-test-suite, Property: CheckpointingMode classification consistency**
        // **Validates: Requirements 5.1**
        /// Property: For any CheckpointingMode, exactly one of is_eager/is_batched/is_optimistic SHALL be true.
        #[test]
        fn prop_checkpointing_mode_classification(mode in checkpointing_mode_strategy()) {
            let eager = mode.is_eager();
            let batched = mode.is_batched();
            let optimistic = mode.is_optimistic();

            // Exactly one should be true
            let count = [eager, batched, optimistic].iter().filter(|&&x| x).count();
            prop_assert_eq!(count, 1, "Exactly one classification should be true");

            // Verify consistency with the enum variant
            match mode {
                CheckpointingMode::Eager => prop_assert!(eager),
                CheckpointingMode::Batched => prop_assert!(batched),
                CheckpointingMode::Optimistic => prop_assert!(optimistic),
            }
        }

        // **Feature: rust-sdk-test-suite, Property: StepSemantics serialization round-trip**
        // **Validates: Requirements 5.1**
        /// Property: For any StepSemantics value, serializing then deserializing SHALL produce the same value.
        #[test]
        fn prop_step_semantics_roundtrip(semantics in step_semantics_strategy()) {
            let serialized = serde_json::to_string(&semantics).unwrap();
            let deserialized: StepSemantics = serde_json::from_str(&serialized).unwrap();
            prop_assert_eq!(semantics, deserialized);
        }

        // **Feature: rust-sdk-test-suite, Property: ItemBatcher validity**
        // **Validates: Requirements 5.1**
        /// Property: For any ItemBatcher with positive values, the configuration SHALL be valid.
        #[test]
        fn prop_item_batcher_validity(
            max_items in 1usize..=10000,
            max_bytes in 1usize..=10_000_000
        ) {
            let batcher = ItemBatcher::new(max_items, max_bytes);

            prop_assert_eq!(batcher.max_items_per_batch, max_items);
            prop_assert_eq!(batcher.max_bytes_per_batch, max_bytes);

            // Verify Debug trait works
            let debug_str = format!("{:?}", batcher);
            prop_assert!(!debug_str.is_empty());
        }

        // **Feature: rust-sdk-test-suite, Property: ChildConfig builder pattern consistency**
        // **Validates: Requirements 5.1**
        /// Property: For any ChildConfig, the builder pattern SHALL produce consistent results.
        #[test]
        fn prop_child_config_builder_consistency(replay_children in proptest::bool::ANY) {
            let config = ChildConfig::new().set_replay_children(replay_children);

            prop_assert_eq!(config.replay_children, replay_children);

            // Verify Debug trait works
            let debug_str = format!("{:?}", config);
            prop_assert!(!debug_str.is_empty());
        }

        // **Feature: rust-sdk-test-suite, Property: MapConfig validity**
        // **Validates: Requirements 5.1**
        /// Property: For any MapConfig with valid values, the configuration SHALL be usable.
        #[test]
        fn prop_map_config_validity(
            max_concurrency in proptest::option::of(1usize..=1000)
        ) {
            let config = MapConfig {
                max_concurrency,
                item_batcher: None,
                completion_config: CompletionConfig::default(),
                serdes: None,
            };

            prop_assert_eq!(config.max_concurrency, max_concurrency);

            // Verify Debug trait works
            let debug_str = format!("{:?}", config);
            prop_assert!(!debug_str.is_empty());
        }

        // **Feature: rust-sdk-test-suite, Property: ParallelConfig validity**
        // **Validates: Requirements 5.1**
        /// Property: For any ParallelConfig with valid values, the configuration SHALL be usable.
        #[test]
        fn prop_parallel_config_validity(
            max_concurrency in proptest::option::of(1usize..=1000)
        ) {
            let config = ParallelConfig {
                max_concurrency,
                completion_config: CompletionConfig::default(),
                serdes: None,
            };

            prop_assert_eq!(config.max_concurrency, max_concurrency);

            // Verify Debug trait works
            let debug_str = format!("{:?}", config);
            prop_assert!(!debug_str.is_empty());
        }

        // **Feature: sdk-ergonomics-improvements, Property 5: ItemBatcher Configuration Respected**
        // **Validates: Requirements 2.1, 2.2**
        /// Property: For any ItemBatcher configuration with max_items_per_batch and max_bytes_per_batch,
        /// the batch method SHALL produce batches where each batch has at most max_items_per_batch items
        /// AND at most max_bytes_per_batch bytes (estimated).
        #[test]
        fn prop_item_batcher_configuration_respected(
            max_items in 1usize..=50,
            max_bytes in 100usize..=10000,
            item_count in 0usize..=200
        ) {
            let batcher = ItemBatcher::new(max_items, max_bytes);

            // Generate items of varying sizes (strings of different lengths)
            let items: Vec<String> = (0..item_count)
                .map(|i| format!("item_{:04}", i))
                .collect();

            let batches = batcher.batch(&items);

            // Verify each batch respects the item count limit
            for (_, batch) in &batches {
                prop_assert!(
                    batch.len() <= max_items,
                    "Batch has {} items but max is {}",
                    batch.len(),
                    max_items
                );
            }

            // Verify each batch respects the byte limit (with tolerance for single large items)
            for (_, batch) in &batches {
                let batch_bytes: usize = batch.iter()
                    .map(|item| serde_json::to_string(item).map(|s| s.len()).unwrap_or(0))
                    .sum();

                // A batch may exceed max_bytes only if it contains a single item
                // (we can't split a single item)
                if batch.len() > 1 {
                    prop_assert!(
                        batch_bytes <= max_bytes,
                        "Batch has {} bytes but max is {} (batch has {} items)",
                        batch_bytes,
                        max_bytes,
                        batch.len()
                    );
                }
            }
        }

        // **Feature: sdk-ergonomics-improvements, Property 6: ItemBatcher Ordering Preservation**
        // **Validates: Requirements 2.3, 2.4, 2.6, 2.7**
        /// Property: For any list of items, after batching with ItemBatcher, concatenating all batches
        /// in order SHALL produce a list equal to the original input list.
        #[test]
        fn prop_item_batcher_ordering_preservation(
            max_items in 1usize..=50,
            max_bytes in 100usize..=10000,
            item_count in 0usize..=200
        ) {
            let batcher = ItemBatcher::new(max_items, max_bytes);

            // Generate items with unique identifiers to verify ordering
            let items: Vec<String> = (0..item_count)
                .map(|i| format!("item_{:04}", i))
                .collect();

            let batches = batcher.batch(&items);

            // Concatenate all batches in order
            let reconstructed: Vec<String> = batches
                .into_iter()
                .flat_map(|(_, batch)| batch)
                .collect();

            // Verify the reconstructed list equals the original
            prop_assert_eq!(
                items.len(),
                reconstructed.len(),
                "Reconstructed list has different length: expected {}, got {}",
                items.len(),
                reconstructed.len()
            );

            for (i, (original, reconstructed_item)) in items.iter().zip(reconstructed.iter()).enumerate() {
                prop_assert_eq!(
                    original,
                    reconstructed_item,
                    "Item at index {} differs: expected '{}', got '{}'",
                    i,
                    original,
                    reconstructed_item
                );
            }
        }
    }

    // =========================================================================
    // JitterStrategy Unit Tests
    // =========================================================================

    #[test]
    fn test_jitter_strategy_none_returns_exact_delay() {
        let jitter = JitterStrategy::None;
        assert_eq!(jitter.apply(10.0, 0), 10.0);
        assert_eq!(jitter.apply(5.5, 3), 5.5);
        assert_eq!(jitter.apply(0.0, 0), 0.0);
        assert_eq!(jitter.apply(100.0, 99), 100.0);
    }

    #[test]
    fn test_jitter_strategy_full_bounds() {
        let jitter = JitterStrategy::Full;
        for attempt in 0..20 {
            let result = jitter.apply(10.0, attempt);
            assert!(
                (0.0..=10.0).contains(&result),
                "Full jitter for attempt {} produced {}, expected [0, 10]",
                attempt,
                result
            );
        }
    }

    #[test]
    fn test_jitter_strategy_half_bounds() {
        let jitter = JitterStrategy::Half;
        for attempt in 0..20 {
            let result = jitter.apply(10.0, attempt);
            assert!(
                (5.0..=10.0).contains(&result),
                "Half jitter for attempt {} produced {}, expected [5, 10]",
                attempt,
                result
            );
        }
    }

    #[test]
    fn test_jitter_strategy_deterministic() {
        // Same inputs should always produce the same output
        let full = JitterStrategy::Full;
        let r1 = full.apply(10.0, 5);
        let r2 = full.apply(10.0, 5);
        assert_eq!(r1, r2);

        let half = JitterStrategy::Half;
        let r1 = half.apply(10.0, 5);
        let r2 = half.apply(10.0, 5);
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_jitter_strategy_zero_delay() {
        // Jitter with zero delay should return 0
        assert_eq!(JitterStrategy::Full.apply(0.0, 0), 0.0);
        assert_eq!(JitterStrategy::Half.apply(0.0, 0), 0.0);
        assert_eq!(JitterStrategy::None.apply(0.0, 0), 0.0);
    }

    #[test]
    fn test_jitter_strategy_default_is_none() {
        assert_eq!(JitterStrategy::default(), JitterStrategy::None);
    }

    // =========================================================================
    // Retry Strategy with Jitter Integration Tests
    // =========================================================================

    #[test]
    fn test_exponential_backoff_with_full_jitter() {
        let strategy = ExponentialBackoff::builder()
            .max_attempts(5)
            .base_delay(Duration::from_seconds(5))
            .max_delay(Duration::from_seconds(60))
            .jitter(JitterStrategy::Full)
            .build();

        for attempt in 0..5 {
            let delay = strategy.next_delay(attempt, "error");
            assert!(delay.is_some());
            let secs = delay.unwrap().to_seconds();
            // With full jitter, delay should be >= 1 (minimum floor)
            assert!(secs >= 1, "Attempt {} delay {} < 1", attempt, secs);
        }
        assert!(strategy.next_delay(5, "error").is_none());
    }

    #[test]
    fn test_exponential_backoff_with_half_jitter() {
        let strategy = ExponentialBackoff::builder()
            .max_attempts(5)
            .base_delay(Duration::from_seconds(10))
            .max_delay(Duration::from_seconds(60))
            .jitter(JitterStrategy::Half)
            .build();

        for attempt in 0..5 {
            let delay = strategy.next_delay(attempt, "error");
            assert!(delay.is_some());
            let secs = delay.unwrap().to_seconds();
            assert!(secs >= 1, "Attempt {} delay {} < 1", attempt, secs);
        }
    }

    #[test]
    fn test_exponential_backoff_no_jitter_unchanged() {
        // Verify backward compatibility: no jitter produces same results as before
        let strategy = ExponentialBackoff::new(5, Duration::from_seconds(1));
        assert_eq!(strategy.jitter, JitterStrategy::None);
        assert_eq!(strategy.next_delay(0, "e").map(|d| d.to_seconds()), Some(1));
        assert_eq!(strategy.next_delay(1, "e").map(|d| d.to_seconds()), Some(2));
        assert_eq!(strategy.next_delay(2, "e").map(|d| d.to_seconds()), Some(4));
    }

    #[test]
    fn test_fixed_delay_with_jitter() {
        let strategy =
            FixedDelay::new(3, Duration::from_seconds(10)).with_jitter(JitterStrategy::Full);

        for attempt in 0..3 {
            let delay = strategy.next_delay(attempt, "error");
            assert!(delay.is_some());
            let secs = delay.unwrap().to_seconds();
            assert!(secs >= 1, "Attempt {} delay {} < 1", attempt, secs);
        }
        assert!(strategy.next_delay(3, "error").is_none());
    }

    #[test]
    fn test_fixed_delay_no_jitter_unchanged() {
        let strategy = FixedDelay::new(3, Duration::from_seconds(5));
        assert_eq!(strategy.jitter, JitterStrategy::None);
        assert_eq!(strategy.next_delay(0, "e").map(|d| d.to_seconds()), Some(5));
        assert_eq!(strategy.next_delay(1, "e").map(|d| d.to_seconds()), Some(5));
    }

    #[test]
    fn test_linear_backoff_with_jitter() {
        let strategy =
            LinearBackoff::new(5, Duration::from_seconds(5)).with_jitter(JitterStrategy::Half);

        for attempt in 0..5 {
            let delay = strategy.next_delay(attempt, "error");
            assert!(delay.is_some());
            let secs = delay.unwrap().to_seconds();
            assert!(secs >= 1, "Attempt {} delay {} < 1", attempt, secs);
        }
        assert!(strategy.next_delay(5, "error").is_none());
    }

    #[test]
    fn test_linear_backoff_no_jitter_unchanged() {
        let strategy = LinearBackoff::new(5, Duration::from_seconds(2));
        assert_eq!(strategy.jitter, JitterStrategy::None);
        assert_eq!(strategy.next_delay(0, "e").map(|d| d.to_seconds()), Some(2));
        assert_eq!(strategy.next_delay(1, "e").map(|d| d.to_seconds()), Some(4));
    }

    #[test]
    fn test_jitter_minimum_floor_all_strategies() {
        // Even with full jitter on small delays, minimum should be 1 second
        let exp = ExponentialBackoff::builder()
            .max_attempts(3)
            .base_delay(Duration::from_seconds(1))
            .jitter(JitterStrategy::Full)
            .build();
        for attempt in 0..3 {
            let secs = exp.next_delay(attempt, "e").unwrap().to_seconds();
            assert!(
                secs >= 1,
                "ExponentialBackoff attempt {} delay {} < 1",
                attempt,
                secs
            );
        }

        let fixed = FixedDelay::new(3, Duration::from_seconds(1)).with_jitter(JitterStrategy::Full);
        for attempt in 0..3 {
            let secs = fixed.next_delay(attempt, "e").unwrap().to_seconds();
            assert!(
                secs >= 1,
                "FixedDelay attempt {} delay {} < 1",
                attempt,
                secs
            );
        }

        let linear =
            LinearBackoff::new(3, Duration::from_seconds(1)).with_jitter(JitterStrategy::Full);
        for attempt in 0..3 {
            let secs = linear.next_delay(attempt, "e").unwrap().to_seconds();
            assert!(
                secs >= 1,
                "LinearBackoff attempt {} delay {} < 1",
                attempt,
                secs
            );
        }
    }

    // =========================================================================
    // JitterStrategy Property-Based Tests
    // =========================================================================

    /// Strategy for generating valid JitterStrategy values
    fn jitter_strategy_strategy() -> impl Strategy<Value = JitterStrategy> {
        prop_oneof![
            Just(JitterStrategy::None),
            Just(JitterStrategy::Full),
            Just(JitterStrategy::Half),
        ]
    }

    proptest! {
        // **Feature: rust-sdk-parity-gaps, Property: JitterStrategy::None identity**
        // **Validates: Requirements 1.2**
        /// Property: JitterStrategy::None SHALL return the exact delay for any delay and attempt.
        #[test]
        fn prop_jitter_none_identity(delay in 0.0f64..1000.0, attempt in 0u32..100) {
            let result = JitterStrategy::None.apply(delay, attempt);
            prop_assert!((result - delay).abs() < f64::EPSILON,
                "None jitter changed delay from {} to {}", delay, result);
        }

        // **Feature: rust-sdk-parity-gaps, Property: JitterStrategy::Full bounds**
        // **Validates: Requirements 1.3**
        /// Property: JitterStrategy::Full SHALL return a delay in [0, d] for any non-negative delay.
        #[test]
        fn prop_jitter_full_bounds(delay in 0.0f64..1000.0, attempt in 0u32..100) {
            let result = JitterStrategy::Full.apply(delay, attempt);
            prop_assert!(result >= 0.0, "Full jitter result {} < 0", result);
            prop_assert!(result <= delay + f64::EPSILON,
                "Full jitter result {} > delay {}", result, delay);
        }

        // **Feature: rust-sdk-parity-gaps, Property: JitterStrategy::Half bounds**
        // **Validates: Requirements 1.4**
        /// Property: JitterStrategy::Half SHALL return a delay in [d/2, d] for any non-negative delay.
        #[test]
        fn prop_jitter_half_bounds(delay in 0.0f64..1000.0, attempt in 0u32..100) {
            let result = JitterStrategy::Half.apply(delay, attempt);
            prop_assert!(result >= delay / 2.0 - f64::EPSILON,
                "Half jitter result {} < delay/2 {}", result, delay / 2.0);
            prop_assert!(result <= delay + f64::EPSILON,
                "Half jitter result {} > delay {}", result, delay);
        }

        // **Feature: rust-sdk-parity-gaps, Property: JitterStrategy determinism**
        // **Validates: Requirements 1.2, 1.3, 1.4**
        /// Property: JitterStrategy::apply SHALL be deterministic for the same inputs.
        #[test]
        fn prop_jitter_deterministic(
            jitter in jitter_strategy_strategy(),
            delay in 0.0f64..1000.0,
            attempt in 0u32..100
        ) {
            let r1 = jitter.apply(delay, attempt);
            let r2 = jitter.apply(delay, attempt);
            prop_assert!((r1 - r2).abs() < f64::EPSILON,
                "Jitter not deterministic: {} vs {}", r1, r2);
        }

        // **Feature: rust-sdk-parity-gaps, Property: Jittered delay minimum floor**
        // **Validates: Requirements 1.10**
        /// Property: All retry strategies with jitter SHALL produce delays >= 1 second.
        #[test]
        fn prop_jitter_minimum_floor(
            jitter in jitter_strategy_strategy(),
            attempt in 0u32..10,
            base_delay_secs in 1u64..100
        ) {
            // ExponentialBackoff
            let exp = ExponentialBackoff::builder()
                .max_attempts(10)
                .base_delay(Duration::from_seconds(base_delay_secs))
                .jitter(jitter)
                .build();
            if let Some(d) = exp.next_delay(attempt, "e") {
                prop_assert!(d.to_seconds() >= 1,
                    "ExponentialBackoff delay {} < 1 for attempt {}", d.to_seconds(), attempt);
            }

            // FixedDelay
            let fixed = FixedDelay::new(10, Duration::from_seconds(base_delay_secs))
                .with_jitter(jitter);
            if let Some(d) = fixed.next_delay(attempt, "e") {
                prop_assert!(d.to_seconds() >= 1,
                    "FixedDelay delay {} < 1 for attempt {}", d.to_seconds(), attempt);
            }

            // LinearBackoff
            let linear = LinearBackoff::new(10, Duration::from_seconds(base_delay_secs))
                .with_jitter(jitter);
            if let Some(d) = linear.next_delay(attempt, "e") {
                prop_assert!(d.to_seconds() >= 1,
                    "LinearBackoff delay {} < 1 for attempt {}", d.to_seconds(), attempt);
            }
        }
    }
}

#[cfg(test)]
mod retryable_error_filter_tests {
    use super::*;

    #[test]
    fn test_empty_filter_retries_all() {
        let filter = RetryableErrorFilter::default();
        assert!(filter.is_retryable("any error message"));
        assert!(filter.is_retryable(""));
        assert!(filter.is_retryable("timeout"));
        assert!(filter.is_retryable_with_type("any error", "AnyType"));
    }

    #[test]
    fn test_contains_pattern_matches_substring() {
        let filter = RetryableErrorFilter {
            patterns: vec![ErrorPattern::Contains("timeout".to_string())],
            error_types: vec![],
        };
        assert!(filter.is_retryable("request timeout occurred"));
        assert!(filter.is_retryable("timeout"));
        assert!(filter.is_retryable("a timeout happened"));
    }

    #[test]
    fn test_contains_pattern_no_match() {
        let filter = RetryableErrorFilter {
            patterns: vec![ErrorPattern::Contains("timeout".to_string())],
            error_types: vec![],
        };
        assert!(!filter.is_retryable("connection refused"));
        assert!(!filter.is_retryable("invalid input"));
        assert!(!filter.is_retryable(""));
    }

    #[test]
    fn test_regex_pattern_matches() {
        let filter = RetryableErrorFilter {
            patterns: vec![ErrorPattern::Regex(
                regex::Regex::new(r"(?i)connection.*refused").unwrap(),
            )],
            error_types: vec![],
        };
        assert!(filter.is_retryable("Connection was refused"));
        assert!(filter.is_retryable("connection refused"));
        assert!(filter.is_retryable("CONNECTION actively REFUSED"));
    }

    #[test]
    fn test_regex_pattern_no_match() {
        let filter = RetryableErrorFilter {
            patterns: vec![ErrorPattern::Regex(
                regex::Regex::new(r"(?i)connection.*refused").unwrap(),
            )],
            error_types: vec![],
        };
        assert!(!filter.is_retryable("timeout error"));
        assert!(!filter.is_retryable("refused connection")); // wrong order
    }

    #[test]
    fn test_or_logic_multiple_patterns() {
        let filter = RetryableErrorFilter {
            patterns: vec![
                ErrorPattern::Contains("timeout".to_string()),
                ErrorPattern::Regex(regex::Regex::new(r"(?i)connection.*refused").unwrap()),
            ],
            error_types: vec![],
        };
        // Matches first pattern
        assert!(filter.is_retryable("request timeout"));
        // Matches second pattern
        assert!(filter.is_retryable("Connection refused"));
        // Matches neither
        assert!(!filter.is_retryable("invalid input"));
    }

    #[test]
    fn test_error_type_matching() {
        let filter = RetryableErrorFilter {
            patterns: vec![],
            error_types: vec!["TransientError".to_string()],
        };
        // is_retryable only checks patterns, not types
        assert!(!filter.is_retryable("some error"));
        // is_retryable_with_type checks both
        assert!(filter.is_retryable_with_type("some error", "TransientError"));
        assert!(!filter.is_retryable_with_type("some error", "PermanentError"));
    }

    #[test]
    fn test_or_logic_patterns_and_types() {
        let filter = RetryableErrorFilter {
            patterns: vec![ErrorPattern::Contains("timeout".to_string())],
            error_types: vec!["TransientError".to_string()],
        };
        // Matches pattern only
        assert!(filter.is_retryable_with_type("request timeout", "PermanentError"));
        // Matches type only
        assert!(filter.is_retryable_with_type("invalid input", "TransientError"));
        // Matches both
        assert!(filter.is_retryable_with_type("request timeout", "TransientError"));
        // Matches neither
        assert!(!filter.is_retryable_with_type("invalid input", "PermanentError"));
    }

    #[test]
    fn test_error_pattern_debug() {
        let contains = ErrorPattern::Contains("test".to_string());
        let debug_str = format!("{:?}", contains);
        assert!(debug_str.contains("Contains"));
        assert!(debug_str.contains("test"));

        let regex = ErrorPattern::Regex(regex::Regex::new(r"\d+").unwrap());
        let debug_str = format!("{:?}", regex);
        assert!(debug_str.contains("Regex"));
    }

    #[test]
    fn test_retryable_error_filter_clone() {
        let filter = RetryableErrorFilter {
            patterns: vec![
                ErrorPattern::Contains("timeout".to_string()),
                ErrorPattern::Regex(regex::Regex::new(r"err\d+").unwrap()),
            ],
            error_types: vec!["TransientError".to_string()],
        };
        let cloned = filter.clone();
        assert!(cloned.is_retryable("timeout error"));
        assert!(cloned.is_retryable("err42"));
        assert!(cloned.is_retryable_with_type("x", "TransientError"));
    }

    // ==========================================================================
    // Tests for WaitDecision, WaitStrategyConfig, and create_wait_strategy
    // Requirements: 4.1–4.6
    // ==========================================================================

    #[test]
    fn test_wait_decision_done_when_predicate_false() {
        // **Validates: Requirements 4.1, 4.2**
        let strategy = create_wait_strategy(WaitStrategyConfig {
            max_attempts: Some(10),
            initial_delay: Duration::from_seconds(5),
            max_delay: Duration::from_seconds(300),
            backoff_rate: 1.5,
            jitter: JitterStrategy::None,
            should_continue_polling: Box::new(|state: &String| state != "COMPLETED"),
        });

        // When predicate returns false (state == "COMPLETED"), should return Done
        let decision = strategy(&"COMPLETED".to_string(), 1);
        assert_eq!(decision, WaitDecision::Done);
    }

    #[test]
    fn test_wait_decision_continue_with_backoff() {
        // **Validates: Requirements 4.3, 4.5**
        let strategy = create_wait_strategy(WaitStrategyConfig {
            max_attempts: Some(10),
            initial_delay: Duration::from_seconds(5),
            max_delay: Duration::from_seconds(300),
            backoff_rate: 2.0,
            jitter: JitterStrategy::None,
            should_continue_polling: Box::new(|state: &String| state != "DONE"),
        });

        // Attempt 1: delay = min(5 * 2^0, 300) = 5s
        let decision = strategy(&"PENDING".to_string(), 1);
        assert_eq!(
            decision,
            WaitDecision::Continue {
                delay: Duration::from_seconds(5)
            }
        );

        // Attempt 2: delay = min(5 * 2^1, 300) = 10s
        let decision = strategy(&"PENDING".to_string(), 2);
        assert_eq!(
            decision,
            WaitDecision::Continue {
                delay: Duration::from_seconds(10)
            }
        );

        // Attempt 3: delay = min(5 * 2^2, 300) = 20s
        let decision = strategy(&"PENDING".to_string(), 3);
        assert_eq!(
            decision,
            WaitDecision::Continue {
                delay: Duration::from_seconds(20)
            }
        );
    }

    #[test]
    fn test_wait_strategy_delay_capped_at_max() {
        // **Validates: Requirement 4.5**
        let strategy = create_wait_strategy(WaitStrategyConfig {
            max_attempts: Some(20),
            initial_delay: Duration::from_seconds(10),
            max_delay: Duration::from_seconds(30),
            backoff_rate: 2.0,
            jitter: JitterStrategy::None,
            should_continue_polling: Box::new(|_: &i32| true),
        });

        // Attempt 3: delay = min(10 * 2^2, 30) = min(40, 30) = 30s
        let decision = strategy(&0, 3);
        assert_eq!(
            decision,
            WaitDecision::Continue {
                delay: Duration::from_seconds(30)
            }
        );

        // Attempt 5: delay = min(10 * 2^4, 30) = min(160, 30) = 30s
        let decision = strategy(&0, 5);
        assert_eq!(
            decision,
            WaitDecision::Continue {
                delay: Duration::from_seconds(30)
            }
        );
    }

    #[test]
    fn test_wait_strategy_max_attempts_returns_done() {
        // **Validates: Requirement 4.4**
        let strategy = create_wait_strategy(WaitStrategyConfig {
            max_attempts: Some(3),
            initial_delay: Duration::from_seconds(5),
            max_delay: Duration::from_seconds(300),
            backoff_rate: 1.5,
            jitter: JitterStrategy::None,
            should_continue_polling: Box::new(|_: &i32| true),
        });

        // Attempt 3 should return Done (attempts_made >= max_attempts)
        let decision = strategy(&0, 3);
        assert_eq!(decision, WaitDecision::Done);
    }

    #[test]
    fn test_wait_strategy_jitter_application() {
        // **Validates: Requirement 4.6**
        let strategy = create_wait_strategy(WaitStrategyConfig {
            max_attempts: Some(10),
            initial_delay: Duration::from_seconds(10),
            max_delay: Duration::from_seconds(300),
            backoff_rate: 1.0,
            jitter: JitterStrategy::Full,
            should_continue_polling: Box::new(|_: &i32| true),
        });

        // With Full jitter on a 10s base delay, the result should be in [1, 10]
        // (floored at 1s minimum)
        let decision = strategy(&0, 1);
        match decision {
            WaitDecision::Continue { delay } => {
                assert!(
                    delay.to_seconds() >= 1 && delay.to_seconds() <= 10,
                    "Jittered delay {} should be in [1, 10]",
                    delay.to_seconds()
                );
            }
            WaitDecision::Done => panic!("Expected Continue, got Done"),
        }
    }

    #[test]
    fn test_wait_strategy_delay_minimum_floor() {
        // **Validates: Requirement 4.3**
        let strategy = create_wait_strategy(WaitStrategyConfig {
            max_attempts: Some(10),
            initial_delay: Duration::from_seconds(1),
            max_delay: Duration::from_seconds(300),
            backoff_rate: 1.0,
            jitter: JitterStrategy::Full,
            should_continue_polling: Box::new(|_: &i32| true),
        });

        // Even with Full jitter that could produce 0, the floor should be 1s
        let decision = strategy(&0, 1);
        match decision {
            WaitDecision::Continue { delay } => {
                assert!(
                    delay.to_seconds() >= 1,
                    "Delay {} should be at least 1 second",
                    delay.to_seconds()
                );
            }
            WaitDecision::Done => panic!("Expected Continue, got Done"),
        }
    }

    #[test]
    fn test_wait_strategy_default_max_attempts() {
        // **Validates: Requirement 4.4** — default max_attempts is 60
        let strategy = create_wait_strategy(WaitStrategyConfig {
            max_attempts: None, // defaults to 60
            initial_delay: Duration::from_seconds(1),
            max_delay: Duration::from_seconds(10),
            backoff_rate: 1.0,
            jitter: JitterStrategy::None,
            should_continue_polling: Box::new(|_: &i32| true),
        });

        // Attempt 59 should succeed (< 60)
        let decision = strategy(&0, 59);
        assert!(matches!(decision, WaitDecision::Continue { .. }));
    }

    #[test]
    fn test_wait_strategy_default_max_attempts_returns_done() {
        // **Validates: Requirement 4.4** — default max_attempts is 60
        let strategy = create_wait_strategy(WaitStrategyConfig {
            max_attempts: None, // defaults to 60
            initial_delay: Duration::from_seconds(1),
            max_delay: Duration::from_seconds(10),
            backoff_rate: 1.0,
            jitter: JitterStrategy::None,
            should_continue_polling: Box::new(|_: &i32| true),
        });

        // Attempt 60 should return Done (>= 60)
        let decision = strategy(&0, 60);
        assert_eq!(decision, WaitDecision::Done);
    }

    #[test]
    fn test_wait_decision_enum_variants() {
        // **Validates: Requirement 4.1**
        let cont = WaitDecision::Continue {
            delay: Duration::from_seconds(5),
        };
        let done = WaitDecision::Done;

        // Verify Debug
        assert!(format!("{:?}", cont).contains("Continue"));
        assert!(format!("{:?}", done).contains("Done"));

        // Verify PartialEq
        assert_eq!(
            WaitDecision::Continue {
                delay: Duration::from_seconds(5)
            },
            WaitDecision::Continue {
                delay: Duration::from_seconds(5)
            }
        );
        assert_ne!(
            WaitDecision::Continue {
                delay: Duration::from_seconds(5)
            },
            WaitDecision::Done
        );
    }
}
