//! Parallel Processing Example
//!
//! This example demonstrates how to process multiple items in parallel using
//! the AWS Durable Execution SDK. It shows how to:
//!
//! - Use `ctx.map()` to process collections in parallel
//! - Use `ctx.parallel()` to execute independent operations concurrently
//! - Configure concurrency limits and completion criteria
//! - Handle partial failures with failure tolerance
//!
//! # Running this example
//!
//! This example is designed to run as an AWS Lambda function. To deploy:
//!
//! 1. Build with `cargo lambda build --release --example parallel_processing`
//! 2. Deploy to AWS Lambda with durable execution enabled
//! 3. Invoke with a JSON payload matching the `BatchProcessingEvent` structure
//!
//! # Workflow Overview
//!
//! 1. **Fetch Data Sources**: Parallel fetch from multiple data sources
//! 2. **Process Items**: Map over items with configurable concurrency
//! 3. **Aggregate Results**: Combine results from all processed items
//!
//! Each parallel branch and map item is checkpointed independently,
//! allowing fine-grained resumption after interruptions.

use aws_durable_execution_sdk::{
    durable_execution, CompletionConfig, DurableContext, DurableError, MapConfig,
};
use serde::{Deserialize, Serialize};

/// Input event for batch processing.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BatchProcessingEvent {
    /// Batch identifier
    pub batch_id: String,
    /// Items to process
    pub items: Vec<ProcessingItem>,
    /// Maximum concurrent processing
    pub max_concurrency: Option<usize>,
}

/// An item to be processed.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProcessingItem {
    /// Item identifier
    pub id: String,
    /// Data to process
    pub data: String,
    /// Processing priority (1-10)
    pub priority: u8,
}

/// Result of processing a single item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemResult {
    /// Item identifier
    pub id: String,
    /// Processing status
    pub status: String,
    /// Processed output
    pub output: String,
    /// Processing duration in milliseconds
    pub duration_ms: u64,
}

/// Final result of batch processing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchProcessingResult {
    /// Batch identifier
    pub batch_id: String,
    /// Total items processed
    pub total_processed: usize,
    /// Successfully processed items
    pub successful: usize,
    /// Failed items
    pub failed: usize,
    /// Individual item results
    pub results: Vec<ItemResult>,
}

/// Main batch processing workflow with parallel operations.
///
/// This workflow demonstrates the map pattern for processing collections
/// with controlled parallelism. Each item is processed in its own child
/// context with independent checkpointing.
#[durable_execution]
pub async fn process_batch(
    event: BatchProcessingEvent,
    ctx: DurableContext,
) -> Result<BatchProcessingResult, DurableError> {
    let batch_id = event.batch_id.clone();
    let total_items = event.items.len();

    // =========================================================================
    // Step 1: Validate the batch
    // =========================================================================
    ctx.step_named(
        "validate_batch",
        |_| {
            if event.items.is_empty() {
                return Err("Batch must contain at least one item".into());
            }
            Ok(())
        },
        None,
    )
    .await?;

    // =========================================================================
    // Step 2: Process items in parallel with configurable concurrency
    // =========================================================================
    // Use ctx.map() to process a collection with controlled parallelism.
    // This is ideal for batch processing where you want to limit concurrent
    // operations to avoid overwhelming downstream services.
    let map_config = MapConfig {
        max_concurrency: event.max_concurrency.or(Some(5)), // Default to 5 concurrent
        completion_config: CompletionConfig {
            // Allow up to 10% failures before failing the entire batch
            tolerated_failure_percentage: Some(0.1),
            ..Default::default()
        },
        ..Default::default()
    };

    let item_results = ctx
        .map(
            event.items.clone(),
            |child_ctx: DurableContext, item: ProcessingItem, index: usize| {
                Box::pin(async move {
                    // Each item is processed in its own child context
                    // with independent checkpointing
                    child_ctx
                        .step_named(
                            &format!("process_item_{}", index),
                            |_| {
                                // Simulated item processing
                                let start = std::time::Instant::now();

                                // Process the item (in real code, this would do actual work)
                                let output = format!("processed_{}", item.data.to_uppercase());

                                let duration_ms = start.elapsed().as_millis() as u64;

                                Ok(ItemResult {
                                    id: item.id.clone(),
                                    status: "success".to_string(),
                                    output,
                                    duration_ms,
                                })
                            },
                            None,
                        )
                        .await
                })
            },
            Some(map_config),
        )
        .await?;

    // =========================================================================
    // Step 3: Aggregate results
    // =========================================================================
    let successful_results: Vec<ItemResult> = item_results
        .items
        .iter()
        .filter_map(|item| item.result.clone())
        .collect();

    let successful_count = item_results.succeeded().len();
    let failed_count = item_results.failed().len();

    Ok(BatchProcessingResult {
        batch_id,
        total_processed: total_items,
        successful: successful_count,
        failed: failed_count,
        results: successful_results,
    })
}

/// Example: Processing with minimum successful requirement
///
/// This pattern is useful when you need a quorum of results.
#[durable_execution]
pub async fn quorum_processing(
    event: BatchProcessingEvent,
    ctx: DurableContext,
) -> Result<Vec<ItemResult>, DurableError> {
    let total_items = event.items.len();
    let min_required = total_items.div_ceil(2); // Majority quorum

    let results = ctx
        .map(
            event.items,
            |child_ctx: DurableContext, item: ProcessingItem, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(
                            |_| {
                                Ok(ItemResult {
                                    id: item.id.clone(),
                                    status: "success".to_string(),
                                    output: item.data.clone(),
                                    duration_ms: 0,
                                })
                            },
                            None,
                        )
                        .await
                })
            },
            Some(MapConfig {
                completion_config: CompletionConfig::with_min_successful(min_required),
                ..Default::default()
            }),
        )
        .await?;

    // Return all successful results (at least min_required)
    Ok(results
        .items
        .into_iter()
        .filter_map(|item| item.result)
        .collect())
}

/// Example: Processing with strict success requirement
///
/// This pattern requires all items to succeed.
#[durable_execution]
pub async fn strict_processing(
    event: BatchProcessingEvent,
    ctx: DurableContext,
) -> Result<Vec<ItemResult>, DurableError> {
    let results = ctx
        .map(
            event.items,
            |child_ctx: DurableContext, item: ProcessingItem, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(
                            |_| {
                                Ok(ItemResult {
                                    id: item.id.clone(),
                                    status: "success".to_string(),
                                    output: item.data.to_uppercase(),
                                    duration_ms: 0,
                                })
                            },
                            None,
                        )
                        .await
                })
            },
            Some(MapConfig {
                // Require all items to succeed (zero failure tolerance)
                completion_config: CompletionConfig::all_successful(),
                // Limit concurrency to 3
                max_concurrency: Some(3),
                ..Default::default()
            }),
        )
        .await?;

    // Get all results - will error if any failed
    results
        .get_results()
        .map(|refs| refs.into_iter().cloned().collect())
}

/// Main function for the Lambda runtime.
///
/// This is the entry point when running as a Lambda function.
/// The `#[durable_execution]` macro generates the actual handler.
#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    // Run the Lambda handler
    lambda_runtime::run(lambda_runtime::service_fn(process_batch)).await
}
