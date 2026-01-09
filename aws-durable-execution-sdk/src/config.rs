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

use std::marker::PhantomData;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::duration::Duration;

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
pub trait RetryStrategy: Send + Sync {
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
}
