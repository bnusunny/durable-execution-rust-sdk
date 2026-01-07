//! Configuration types for durable execution operations.
//!
//! This module provides type-safe configuration structs for all
//! durable operations including steps, callbacks, invocations,
//! map, and parallel operations.

use std::marker::PhantomData;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::duration::Duration;

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
#[derive(Debug, Clone, Default)]
pub struct ChildConfig {
    /// Optional custom serializer/deserializer.
    pub serdes: Option<Arc<dyn SerDesAny>>,
}

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
}
