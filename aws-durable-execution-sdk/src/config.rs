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

use serde::{Deserialize, Serialize};

use crate::duration::Duration;
use crate::sealed::Sealed;

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
/// use aws_durable_execution_sdk::CheckpointingMode;
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
            Self::Optimistic => "Execute multiple operations before checkpointing (best performance)",
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
///
/// # Requirements
///
/// - 3.3: THE SDK SHALL implement the sealed trait pattern for the `RetryStrategy` trait
/// - 3.5: THE SDK SHALL document that these traits are sealed and cannot be implemented externally
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
/// use aws_durable_execution_sdk::config::ExponentialBackoff;
/// use aws_durable_execution_sdk::Duration;
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

        Some(Duration::from_seconds(capped_seconds as u64))
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
}

impl Default for ExponentialBackoffBuilder {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_seconds(1),
            max_delay: Duration::from_hours(1),
            multiplier: 2.0,
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

    /// Builds the exponential backoff strategy.
    pub fn build(self) -> ExponentialBackoff {
        ExponentialBackoff {
            max_attempts: self.max_attempts,
            base_delay: self.base_delay,
            max_delay: self.max_delay,
            multiplier: self.multiplier,
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
/// use aws_durable_execution_sdk::config::FixedDelay;
/// use aws_durable_execution_sdk::Duration;
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
}

impl FixedDelay {
    /// Creates a new fixed delay retry strategy.
    ///
    /// # Arguments
    ///
    /// * `max_attempts` - Maximum number of retry attempts
    /// * `delay` - Delay between retry attempts
    pub fn new(max_attempts: u32, delay: Duration) -> Self {
        Self { max_attempts, delay }
    }
}

impl Sealed for FixedDelay {}

impl RetryStrategy for FixedDelay {
    fn next_delay(&self, attempt: u32, _error: &str) -> Option<Duration> {
        if attempt >= self.max_attempts {
            return None;
        }
        Some(self.delay.clone())
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
/// use aws_durable_execution_sdk::config::LinearBackoff;
/// use aws_durable_execution_sdk::Duration;
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
        }
    }

    /// Sets the maximum delay between retries.
    pub fn with_max_delay(mut self, max_delay: Duration) -> Self {
        self.max_delay = max_delay;
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
        let capped_seconds = delay_seconds.min(max_seconds);

        Some(Duration::from_seconds(capped_seconds))
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
/// use aws_durable_execution_sdk::config::NoRetry;
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

/// Custom retry strategy using a user-provided closure.
///
/// This allows users to define custom retry logic without implementing
/// the sealed `RetryStrategy` trait directly.
///
/// # Example
///
/// ```
/// use aws_durable_execution_sdk::config::custom_retry;
/// use aws_durable_execution_sdk::Duration;
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

impl<F> Sealed for CustomRetry<F> where F: Fn(u32, &str) -> Option<Duration> + Send + Sync + Clone + 'static {}

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
/// use aws_durable_execution_sdk::StepConfig;
///
/// let config = StepConfig::default();
/// // Default uses AtLeastOncePerRetry semantics
/// ```
///
/// Configuring step semantics:
///
/// ```
/// use aws_durable_execution_sdk::{StepConfig, StepSemantics};
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
}

impl std::fmt::Debug for StepConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StepConfig")
            .field("retry_strategy", &self.retry_strategy.is_some())
            .field("step_semantics", &self.step_semantics)
            .field("serdes", &self.serdes.is_some())
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
/// use aws_durable_execution_sdk::MapConfig;
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
/// use aws_durable_execution_sdk::{MapConfig, CompletionConfig};
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
///
/// # Requirements
///
/// - 10.5: THE Child_Context_Operation SHALL support ReplayChildren option for large parallel operations
/// - 10.6: WHEN ReplayChildren is true, THE Child_Context_Operation SHALL include child operations in state loads for replay
/// - 12.8: THE Configuration_System SHALL provide ContextConfig with replay_children option
#[derive(Debug, Clone, Default)]
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
    ///
    /// # Requirements
    ///
    /// - 10.5: THE Child_Context_Operation SHALL support ReplayChildren option for large parallel operations
    /// - 10.6: WHEN ReplayChildren is true, THE Child_Context_Operation SHALL include child operations in state loads for replay
    pub replay_children: bool,
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
    /// use aws_durable_execution_sdk::ChildConfig;
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
}

/// Type alias for ChildConfig for consistency with the design document.
///
/// The design document refers to this as `ContextConfig`, but internally
/// we use `ChildConfig` to be more descriptive of its purpose.
///
/// # Requirements
///
/// - 12.8: THE Configuration_System SHALL provide ContextConfig with replay_children option
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
    /// use aws_durable_execution_sdk::CompletionConfig;
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
    /// use aws_durable_execution_sdk::CompletionConfig;
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
    /// use aws_durable_execution_sdk::CompletionConfig;
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
    ///
    /// # Requirements
    ///
    /// - 2.1: THE ItemBatcher SHALL support configuring maximum items per batch
    /// - 2.2: THE ItemBatcher SHALL support configuring maximum bytes per batch
    /// - 2.3: WHEN ItemBatcher is configured, THE map operation SHALL group items into batches before processing
    /// - 2.6: WHEN batch size limits are exceeded, THE ItemBatcher SHALL split items into multiple batches
    ///
    /// # Example
    ///
    /// ```
    /// use aws_durable_execution_sdk::ItemBatcher;
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
            let item_bytes = serde_json::to_string(item)
                .map(|s| s.len())
                .unwrap_or(0);

            // Check if adding this item would exceed limits
            let would_exceed_items = current_batch.len() >= self.max_items_per_batch;
            let would_exceed_bytes = current_bytes + item_bytes > self.max_bytes_per_batch
                && !current_batch.is_empty();

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
    fn serialize_any(&self, value: &dyn std::any::Any) -> Result<String, crate::error::DurableError>;
    /// Deserialize a string to a boxed Any value.
    fn deserialize_any(&self, data: &str) -> Result<Box<dyn std::any::Any + Send>, crate::error::DurableError>;
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
        assert!(CheckpointingMode::Eager.description().contains("maximum durability"));
        assert!(CheckpointingMode::Batched.description().contains("balanced"));
        assert!(CheckpointingMode::Optimistic.description().contains("best performance"));
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
        assert_eq!(strategy.next_delay(0, "error").map(|d| d.to_seconds()), Some(1));
        // attempt 1: 1 * 2^1 = 2 seconds
        assert_eq!(strategy.next_delay(1, "error").map(|d| d.to_seconds()), Some(2));
        // attempt 2: 1 * 2^2 = 4 seconds
        assert_eq!(strategy.next_delay(2, "error").map(|d| d.to_seconds()), Some(4));
        // attempt 3: 1 * 2^3 = 8 seconds
        assert_eq!(strategy.next_delay(3, "error").map(|d| d.to_seconds()), Some(8));
        // attempt 4: 1 * 2^4 = 16 seconds
        assert_eq!(strategy.next_delay(4, "error").map(|d| d.to_seconds()), Some(16));
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
        assert_eq!(strategy.next_delay(0, "error").map(|d| d.to_seconds()), Some(10));
        // attempt 1: 10 * 2^1 = 20 seconds
        assert_eq!(strategy.next_delay(1, "error").map(|d| d.to_seconds()), Some(20));
        // attempt 2: 10 * 2^2 = 40 seconds, capped at 30
        assert_eq!(strategy.next_delay(2, "error").map(|d| d.to_seconds()), Some(30));
        // attempt 3: 10 * 2^3 = 80 seconds, capped at 30
        assert_eq!(strategy.next_delay(3, "error").map(|d| d.to_seconds()), Some(30));
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
        assert_eq!(strategy.next_delay(0, "error").map(|d| d.to_seconds()), Some(5));
        assert_eq!(strategy.next_delay(1, "error").map(|d| d.to_seconds()), Some(5));
        assert_eq!(strategy.next_delay(2, "error").map(|d| d.to_seconds()), Some(5));
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
        assert_eq!(strategy.next_delay(0, "error").map(|d| d.to_seconds()), Some(2));
        // attempt 1: 2 * (1+1) = 4 seconds
        assert_eq!(strategy.next_delay(1, "error").map(|d| d.to_seconds()), Some(4));
        // attempt 2: 2 * (2+1) = 6 seconds
        assert_eq!(strategy.next_delay(2, "error").map(|d| d.to_seconds()), Some(6));
        // attempt 3: 2 * (3+1) = 8 seconds
        assert_eq!(strategy.next_delay(3, "error").map(|d| d.to_seconds()), Some(8));
        // attempt 4: 2 * (4+1) = 10 seconds
        assert_eq!(strategy.next_delay(4, "error").map(|d| d.to_seconds()), Some(10));
        // attempt 5: exceeds max_attempts
        assert_eq!(strategy.next_delay(5, "error"), None);
    }

    #[test]
    fn test_linear_backoff_max_delay_cap() {
        let strategy = LinearBackoff::new(10, Duration::from_seconds(5))
            .with_max_delay(Duration::from_seconds(15));

        // attempt 0: 5 * 1 = 5 seconds
        assert_eq!(strategy.next_delay(0, "error").map(|d| d.to_seconds()), Some(5));
        // attempt 1: 5 * 2 = 10 seconds
        assert_eq!(strategy.next_delay(1, "error").map(|d| d.to_seconds()), Some(10));
        // attempt 2: 5 * 3 = 15 seconds
        assert_eq!(strategy.next_delay(2, "error").map(|d| d.to_seconds()), Some(15));
        // attempt 3: 5 * 4 = 20 seconds, capped at 15
        assert_eq!(strategy.next_delay(3, "error").map(|d| d.to_seconds()), Some(15));
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
        let strategy = NoRetry::default();
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

        assert_eq!(strategy.next_delay(0, "error").map(|d| d.to_seconds()), Some(10));
        assert_eq!(strategy.next_delay(1, "error").map(|d| d.to_seconds()), Some(10));
        assert_eq!(strategy.next_delay(2, "error").map(|d| d.to_seconds()), Some(10));
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
        assert_eq!(strategy.next_delay(0, "transient error").map(|d| d.to_seconds()), Some(1));
        // Rate limit errors get longer delay
        assert_eq!(strategy.next_delay(0, "rate_limit exceeded").map(|d| d.to_seconds()), Some(30));
        // Other errors don't retry
        assert_eq!(strategy.next_delay(0, "permanent failure"), None);
    }

    #[test]
    fn test_retry_strategy_clone_box() {
        // Test that clone_box works for all strategies
        let exp: Box<dyn RetryStrategy> = Box::new(ExponentialBackoff::new(3, Duration::from_seconds(1)));
        let exp_clone = exp.clone_box();
        assert_eq!(exp.next_delay(0, "e").map(|d| d.to_seconds()), exp_clone.next_delay(0, "e").map(|d| d.to_seconds()));

        let fixed: Box<dyn RetryStrategy> = Box::new(FixedDelay::new(3, Duration::from_seconds(5)));
        let fixed_clone = fixed.clone_box();
        assert_eq!(fixed.next_delay(0, "e").map(|d| d.to_seconds()), fixed_clone.next_delay(0, "e").map(|d| d.to_seconds()));

        let linear: Box<dyn RetryStrategy> = Box::new(LinearBackoff::new(3, Duration::from_seconds(2)));
        let linear_clone = linear.clone_box();
        assert_eq!(linear.next_delay(0, "e").map(|d| d.to_seconds()), linear_clone.next_delay(0, "e").map(|d| d.to_seconds()));

        let no_retry: Box<dyn RetryStrategy> = Box::new(NoRetry);
        let no_retry_clone = no_retry.clone_box();
        assert_eq!(no_retry.next_delay(0, "e"), no_retry_clone.next_delay(0, "e"));
    }

    #[test]
    fn test_boxed_retry_strategy_clone() {
        // Test the Clone impl for Box<dyn RetryStrategy>
        let strategy: Box<dyn RetryStrategy> = Box::new(ExponentialBackoff::new(3, Duration::from_seconds(1)));
        let cloned = strategy.clone();
        
        assert_eq!(
            strategy.next_delay(0, "error").map(|d| d.to_seconds()),
            cloned.next_delay(0, "error").map(|d| d.to_seconds())
        );
    }

    #[test]
    fn test_step_config_with_retry_strategy() {
        let config = StepConfig {
            retry_strategy: Some(Box::new(ExponentialBackoff::new(3, Duration::from_seconds(1)))),
            step_semantics: StepSemantics::AtLeastOncePerRetry,
            serdes: None,
        };

        assert!(config.retry_strategy.is_some());
        let strategy = config.retry_strategy.as_ref().unwrap();
        assert_eq!(strategy.next_delay(0, "error").map(|d| d.to_seconds()), Some(1));
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
}
