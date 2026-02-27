//! Tests for parallel and map examples using LocalDurableTestRunner.
//!
//! These tests verify that the parallel and map examples execute correctly and produce
//! the expected operation history. The tests cover:
//!
//! - parallel/basic: Running multiple operations in parallel using ctx.map()
//! - parallel/heterogeneous: Running different types of operations using child contexts
//! - parallel/first_successful: Complete when the first branch succeeds
//! - map/basic: Processing arrays with concurrent operations
//! - map/with_concurrency: Processing items with controlled concurrency
//! - map/failure_tolerance: Processing items with configurable failure tolerance
//! - map/min_successful: Processing items with a quorum requirement
//!
//! These tests use `#[tokio::test(flavor = "current_thread")]` to ensure each test
//! has its own isolated tokio runtime for time control.

use aws_durable_execution_sdk::{
    CompletionConfig, CompletionReason, DurableContext, DurableError, Duration, MapConfig,
    OperationType,
};
use aws_durable_execution_sdk_examples::test_helper::{
    assert_nodejs_event_signatures, assert_nodejs_event_signatures_unordered,
};
use aws_durable_execution_sdk_testing::{
    ExecutionStatus, LocalDurableTestRunner, TestEnvironmentConfig,
};
use serde::{Deserialize, Serialize};

// ============================================================================
// Parallel Basic Example Handler
// ============================================================================

/// Handler from parallel/basic example (without macro for testing)
async fn parallel_basic_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<Vec<String>, DurableError> {
    let task_ids = vec![1, 2, 3];

    let results = ctx
        .map(
            task_ids,
            |child_ctx: DurableContext, task_id: i32, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(|_| Ok(format!("task {} completed", task_id)), None)
                        .await
                })
            },
            Some(MapConfig {
                max_concurrency: Some(3),
                ..Default::default()
            }),
        )
        .await?;

    results
        .get_results()
        .map(|refs| refs.into_iter().cloned().collect())
}

#[tokio::test(flavor = "current_thread")]
async fn test_parallel_basic() {
    LocalDurableTestRunner::<serde_json::Value, Vec<String>>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(parallel_basic_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully"
    );

    let results = result.get_result().unwrap();
    assert_eq!(results.len(), 3, "Should have 3 results");
    assert!(results.contains(&"task 1 completed".to_string()));
    assert!(results.contains(&"task 2 completed".to_string()));
    assert!(results.contains(&"task 3 completed".to_string()));

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Should have context operations for child contexts and step operations
    let context_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Context)
        .collect();
    assert!(
        !context_ops.is_empty(),
        "Should have context operations for map"
    );

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/parallel_basic.history.json");

    // Note: teardown is not needed since each test has its own tokio runtime
    // and the time state is automatically cleaned up when the runtime is dropped
}

// ============================================================================
// Parallel Heterogeneous Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServiceResults {
    pub inventory: String,
    pub payment: String,
    pub shipping: String,
}

/// Handler from parallel/heterogeneous example (without macro for testing)
async fn parallel_heterogeneous_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<ServiceResults, DurableError> {
    // Run each service check in its own child context
    let inventory = ctx
        .run_in_child_context_named(
            "check_inventory",
            |child_ctx| {
                Box::pin(async move {
                    child_ctx
                        .step(|_| Ok("inventory_available".to_string()), None)
                        .await
                })
            },
            None,
        )
        .await?;

    let payment = ctx
        .run_in_child_context_named(
            "process_payment",
            |child_ctx| {
                Box::pin(async move {
                    // Payment includes a delay
                    child_ctx
                        .wait(Duration::from_seconds(1), Some("payment_delay"))
                        .await?;
                    child_ctx
                        .step(|_| Ok("payment_completed".to_string()), None)
                        .await
                })
            },
            None,
        )
        .await?;

    let shipping = ctx
        .run_in_child_context_named(
            "calculate_shipping",
            |child_ctx| {
                Box::pin(async move {
                    child_ctx
                        .step(|_| Ok("shipping_calculated".to_string()), None)
                        .await
                })
            },
            None,
        )
        .await?;

    Ok(ServiceResults {
        inventory,
        payment,
        shipping,
    })
}

#[tokio::test(flavor = "current_thread")]
async fn test_parallel_heterogeneous() {
    LocalDurableTestRunner::<serde_json::Value, ServiceResults>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(parallel_heterogeneous_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully"
    );

    let service_results = result.get_result().unwrap();
    assert_eq!(service_results.inventory, "inventory_available");
    assert_eq!(service_results.payment, "payment_completed");
    assert_eq!(service_results.shipping, "shipping_calculated");

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Should have context operations for child contexts
    let context_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Context)
        .collect();
    assert_eq!(
        context_ops.len(),
        3,
        "Should have 3 context operations for child contexts"
    );

    // Verify context names
    let context_names: Vec<_> = context_ops
        .iter()
        .filter_map(|op| op.name.as_ref())
        .collect();
    assert!(context_names.contains(&&"check_inventory".to_string()));
    assert!(context_names.contains(&&"process_payment".to_string()));
    assert!(context_names.contains(&&"calculate_shipping".to_string()));

    // Should have a wait operation in the payment context
    let wait_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Wait)
        .collect();
    assert!(!wait_ops.is_empty(), "Should have wait operation");

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/parallel_heterogeneous.history.json");
}

// ============================================================================
// Parallel First Successful Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DataResult {
    pub source: String,
    pub data: String,
}

/// Handler from parallel/first_successful example (without macro for testing)
async fn parallel_first_successful_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<DataResult, DurableError> {
    let sources: Vec<String> = vec![
        "primary".to_string(),
        "secondary".to_string(),
        "cache".to_string(),
    ];

    let results = ctx
        .map(
            sources,
            |child_ctx: DurableContext, source: String, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step_named(
                            &format!("{}_db", source),
                            |_| {
                                Ok(DataResult {
                                    source: source.clone(),
                                    data: format!("data_from_{}", source),
                                })
                            },
                            None,
                        )
                        .await
                })
            },
            Some(MapConfig {
                completion_config: CompletionConfig::first_successful(),
                ..Default::default()
            }),
        )
        .await?;

    // Get the first successful result
    let first_result = results
        .succeeded()
        .first()
        .and_then(|item| item.result.clone())
        .ok_or_else(|| DurableError::execution("All data sources failed"))?;

    Ok(first_result)
}

#[tokio::test(flavor = "current_thread")]
async fn test_parallel_first_successful() {
    LocalDurableTestRunner::<serde_json::Value, DataResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(parallel_first_successful_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully"
    );

    let data_result = result.get_result().unwrap();
    // The first successful result should be from one of the sources
    assert!(
        ["primary", "secondary", "cache"].contains(&data_result.source.as_str()),
        "Source should be one of the expected sources"
    );
    assert!(
        data_result.data.starts_with("data_from_"),
        "Data should have expected format"
    );

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(
        &result,
        "tests/history/parallel_first_successful.history.json",
    );
}

// ============================================================================
// Map Basic Example Handler
// ============================================================================

/// Handler from map/basic example (without macro for testing)
async fn map_basic_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<Vec<i32>, DurableError> {
    let items = vec![1, 2, 3, 4, 5];

    let results = ctx
        .map(
            items,
            |child_ctx, item: i32, _index: usize| {
                Box::pin(async move { child_ctx.step(|_| Ok(item * 2), None).await })
            },
            None,
        )
        .await?;

    // Get all successful results
    results
        .get_results()
        .map(|refs| refs.into_iter().cloned().collect())
}

#[tokio::test(flavor = "current_thread")]
async fn test_map_basic() {
    LocalDurableTestRunner::<serde_json::Value, Vec<i32>>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(map_basic_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully"
    );

    let results = result.get_result().unwrap();
    assert_eq!(results.len(), 5, "Should have 5 results");
    // Results should be doubled values
    assert!(results.contains(&2));
    assert!(results.contains(&4));
    assert!(results.contains(&6));
    assert!(results.contains(&8));
    assert!(results.contains(&10));

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/map_basic.history.json");
}

// ============================================================================
// Map With Concurrency Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessedItem {
    pub id: String,
    pub result: String,
}

/// Handler from map/with_concurrency example (without macro for testing)
async fn map_with_concurrency_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<Vec<ProcessedItem>, DurableError> {
    let items: Vec<String> = vec![
        "item-1".to_string(),
        "item-2".to_string(),
        "item-3".to_string(),
        "item-4".to_string(),
        "item-5".to_string(),
        "item-6".to_string(),
        "item-7".to_string(),
        "item-8".to_string(),
        "item-9".to_string(),
        "item-10".to_string(),
    ];

    // Configure map with concurrency limit
    let config = MapConfig {
        max_concurrency: Some(3), // Process at most 3 items concurrently
        ..Default::default()
    };

    let results = ctx
        .map(
            items,
            |child_ctx, item: String, index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step_named(
                            &format!("process_{}", index),
                            |_| {
                                Ok(ProcessedItem {
                                    id: item.clone(),
                                    result: format!("processed_{}", item),
                                })
                            },
                            None,
                        )
                        .await
                })
            },
            Some(config),
        )
        .await?;

    results
        .get_results()
        .map(|refs| refs.into_iter().cloned().collect())
}

#[tokio::test(flavor = "current_thread")]
async fn test_map_with_concurrency() {
    LocalDurableTestRunner::<serde_json::Value, Vec<ProcessedItem>>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(map_with_concurrency_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully"
    );

    let results = result.get_result().unwrap();
    assert_eq!(results.len(), 10, "Should have 10 results");

    // Verify all items were processed
    for i in 1..=10 {
        let expected_id = format!("item-{}", i);
        let expected_result = format!("processed_item-{}", i);
        assert!(
            results
                .iter()
                .any(|r| r.id == expected_id && r.result == expected_result),
            "Should have processed item-{}",
            i
        );
    }

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check event signatures (Node.js-compatible format, unordered because concurrency can affect operation order)
    assert_nodejs_event_signatures_unordered(
        &result,
        "tests/history/map_with_concurrency.history.json",
    );
}

// ============================================================================
// Map Failure Tolerance Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BatchResult {
    pub total: usize,
    pub successful: usize,
    pub failed: usize,
}

/// Handler from map/failure_tolerance example (without macro for testing)
async fn map_failure_tolerance_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<BatchResult, DurableError> {
    let items: Vec<i32> = (1..=20).collect();
    let total = items.len();

    // Configure map with failure tolerance
    let config = MapConfig {
        max_concurrency: Some(5),
        completion_config: CompletionConfig {
            // Allow up to 10% failures
            tolerated_failure_percentage: Some(0.1),
            ..Default::default()
        },
        ..Default::default()
    };

    let results = ctx
        .map(
            items,
            |child_ctx, item: i32, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(
                            |_| {
                                // Simulate some items failing
                                if item % 7 == 0 {
                                    Err("Simulated failure".into())
                                } else {
                                    Ok(item * 2)
                                }
                            },
                            None,
                        )
                        .await
                })
            },
            Some(config),
        )
        .await?;

    Ok(BatchResult {
        total,
        successful: results.succeeded().len(),
        failed: results.failed().len(),
    })
}

#[tokio::test(flavor = "current_thread")]
async fn test_map_failure_tolerance() {
    LocalDurableTestRunner::<serde_json::Value, BatchResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(map_failure_tolerance_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully with failure tolerance"
    );

    let batch_result = result.get_result().unwrap();
    assert_eq!(batch_result.total, 20, "Should have 20 total items");
    // Items 7 and 14 should fail (divisible by 7)
    assert_eq!(batch_result.failed, 2, "Should have 2 failed items");
    assert_eq!(
        batch_result.successful, 18,
        "Should have 18 successful items"
    );

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check event signatures (Node.js-compatible format, unordered because concurrency can affect operation order)
    assert_nodejs_event_signatures_unordered(
        &result,
        "tests/history/map_failure_tolerance.history.json",
    );
}

// ============================================================================
// Map Min Successful Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuorumResult {
    pub required: usize,
    pub achieved: usize,
    pub results: Vec<String>,
}

/// Handler from map/min_successful example (without macro for testing)
async fn map_min_successful_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<QuorumResult, DurableError> {
    let items: Vec<String> = vec![
        "node-1".to_string(),
        "node-2".to_string(),
        "node-3".to_string(),
        "node-4".to_string(),
        "node-5".to_string(),
    ];
    let total = items.len();
    let min_required = total.div_ceil(2); // Majority quorum

    // Configure map with minimum successful requirement
    let config = MapConfig {
        completion_config: CompletionConfig::with_min_successful(min_required),
        ..Default::default()
    };

    let results = ctx
        .map(
            items,
            |child_ctx, item: String, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(|_| Ok(format!("response_from_{}", item)), None)
                        .await
                })
            },
            Some(config),
        )
        .await?;

    let successful_results: Vec<String> = results
        .items
        .iter()
        .filter_map(|item| item.result.clone())
        .collect();

    Ok(QuorumResult {
        required: min_required,
        achieved: successful_results.len(),
        results: successful_results,
    })
}

#[tokio::test(flavor = "current_thread")]
async fn test_map_min_successful() {
    LocalDurableTestRunner::<serde_json::Value, QuorumResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(map_min_successful_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully"
    );

    let quorum_result = result.get_result().unwrap();
    assert_eq!(
        quorum_result.required, 3,
        "Should require 3 (majority of 5)"
    );
    assert!(
        quorum_result.achieved >= quorum_result.required,
        "Should achieve at least the required quorum"
    );
    assert!(
        quorum_result.results.len() >= 3,
        "Should have at least 3 results"
    );

    // Verify all results have expected format
    for result_str in &quorum_result.results {
        assert!(
            result_str.starts_with("response_from_node-"),
            "Result should have expected format"
        );
    }

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/map_min_successful.history.json");
}

// ============================================================================
// Parallel Empty Array Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmptyParallelResult {
    pub total_branches: usize,
    pub message: String,
}

/// Handler from parallel/empty_array example (without macro for testing)
async fn parallel_empty_array_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<EmptyParallelResult, DurableError> {
    let results = ctx
        .map(
            Vec::<i32>::new(),
            |child_ctx: DurableContext, _item: i32, _index: usize| {
                Box::pin(async move { child_ctx.step(|_| Ok("".to_string()), None).await })
            },
            None,
        )
        .await?;

    Ok(EmptyParallelResult {
        total_branches: results.items.len(),
        message: "Empty parallel completed successfully".to_string(),
    })
}

#[tokio::test(flavor = "current_thread")]
async fn test_parallel_empty_array() {
    LocalDurableTestRunner::<serde_json::Value, EmptyParallelResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(parallel_empty_array_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully with empty input"
    );

    let output = result.get_result().unwrap();
    assert_eq!(
        output.total_branches, 0,
        "Should have 0 branches for empty input"
    );
    assert_eq!(output.message, "Empty parallel completed successfully");

    // Check event signatures against history file
    assert_nodejs_event_signatures(&result, "tests/history/parallel_empty_array.history.json");
}

// ============================================================================
// Parallel With Wait Example Handler
// ============================================================================

/// Handler from parallel/with_wait example (without macro for testing)
async fn parallel_with_wait_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<Vec<String>, DurableError> {
    let results = ctx
        .map(
            vec![1, 2, 3],
            |child_ctx: DurableContext, item: i32, _index: usize| {
                Box::pin(async move {
                    // Wait for item seconds
                    child_ctx
                        .wait(
                            Duration::from_seconds(item as u64),
                            Some(&format!("wait_{}", item)),
                        )
                        .await?;
                    // Then do a step that returns a result
                    child_ctx
                        .step(|_| Ok(format!("branch {} done", item)), None)
                        .await
                })
            },
            Some(MapConfig {
                max_concurrency: Some(3),
                ..Default::default()
            }),
        )
        .await?;

    // If the map operation is suspended (waiting for waits to complete),
    // propagate the suspension so the orchestrator can re-invoke
    if results.completion_reason == CompletionReason::Suspended {
        return Err(DurableError::suspend());
    }

    results
        .get_results()
        .map(|refs| refs.into_iter().cloned().collect())
}

#[tokio::test(flavor = "current_thread")]
async fn test_parallel_with_wait() {
    LocalDurableTestRunner::<serde_json::Value, Vec<String>>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(parallel_with_wait_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully"
    );

    let output = result.get_result().unwrap();
    assert_eq!(output.len(), 3, "Should have 3 branch results");

    // Verify all branches completed (order may vary due to parallel execution)
    let mut sorted_output = output.clone();
    sorted_output.sort();
    assert_eq!(
        sorted_output,
        vec![
            "branch 1 done".to_string(),
            "branch 2 done".to_string(),
            "branch 3 done".to_string(),
        ]
    );

    // Check event signatures against history file (unordered for parallel ops)
    assert_nodejs_event_signatures_unordered(
        &result,
        "tests/history/parallel_with_wait.history.json",
    );
}

// ============================================================================
// Parallel Error Preservation Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ErrorPreservationResult {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub error_messages: Vec<String>,
}

/// Handler from parallel/error_preservation example (without macro for testing)
async fn parallel_error_preservation_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<ErrorPreservationResult, DurableError> {
    let config = MapConfig {
        max_concurrency: Some(5),
        completion_config: CompletionConfig {
            tolerated_failure_count: Some(3),
            ..Default::default()
        },
        ..Default::default()
    };

    let results = ctx
        .map(
            vec![1, 2, 3, 4, 5],
            |child_ctx: DurableContext, item: i32, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(
                            |_| {
                                if item % 2 == 0 {
                                    Err(format!("Item {} failed: even numbers not allowed", item)
                                        .into())
                                } else {
                                    Ok(format!("item {} ok", item))
                                }
                            },
                            None,
                        )
                        .await
                })
            },
            Some(config),
        )
        .await?;

    let failed = results.failed();
    let succeeded = results.succeeded();

    let error_messages: Vec<String> = failed
        .iter()
        .map(|item| {
            item.get_error()
                .map(|e| e.error_message.clone())
                .unwrap_or_else(|| "unknown error".to_string())
        })
        .collect();

    Ok(ErrorPreservationResult {
        total: results.total_count(),
        succeeded: succeeded.len(),
        failed: failed.len(),
        error_messages,
    })
}

#[tokio::test(flavor = "current_thread")]
async fn test_parallel_error_preservation() {
    LocalDurableTestRunner::<serde_json::Value, ErrorPreservationResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(parallel_error_preservation_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should succeed with tolerated failures"
    );

    let output = result.get_result().unwrap();
    assert_eq!(output.total, 5, "Should have 5 total items");
    assert_eq!(
        output.succeeded, 3,
        "Should have 3 succeeded items (1, 3, 5)"
    );
    assert_eq!(output.failed, 2, "Should have 2 failed items (2, 4)");
    assert_eq!(
        output.error_messages.len(),
        2,
        "Should have 2 error messages"
    );

    // Verify error messages are preserved
    for msg in &output.error_messages {
        assert!(
            msg.contains("even numbers not allowed"),
            "Error message should contain the original error text, got: {}",
            msg
        );
    }

    // Check event signatures against history file (unordered for parallel ops)
    assert_nodejs_event_signatures_unordered(
        &result,
        "tests/history/parallel_error_preservation.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, ErrorPreservationResult>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Parallel Min Successful Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MinSuccessfulResult {
    pub required: usize,
    pub completed: usize,
    pub results: Vec<String>,
}

/// Handler from parallel/min_successful example (without macro for testing)
async fn parallel_min_successful_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<MinSuccessfulResult, DurableError> {
    let config = MapConfig {
        max_concurrency: Some(5),
        completion_config: CompletionConfig::with_min_successful(2),
        ..Default::default()
    };

    let results = ctx
        .map(
            vec![
                "task-a".to_string(),
                "task-b".to_string(),
                "task-c".to_string(),
                "task-d".to_string(),
                "task-e".to_string(),
            ],
            |child_ctx: DurableContext, item: String, _index: usize| {
                Box::pin(
                    async move { child_ctx.step(|_| Ok(format!("{} done", item)), None).await },
                )
            },
            Some(config),
        )
        .await?;

    let succeeded = results.succeeded();
    let successful_results: Vec<String> = succeeded
        .iter()
        .filter_map(|item| item.result.clone())
        .collect();

    Ok(MinSuccessfulResult {
        required: 2,
        completed: successful_results.len(),
        results: successful_results,
    })
}

#[tokio::test(flavor = "current_thread")]
async fn test_parallel_min_successful() {
    LocalDurableTestRunner::<serde_json::Value, MinSuccessfulResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(parallel_min_successful_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully"
    );

    let output = result.get_result().unwrap();
    assert_eq!(output.required, 2, "Should require 2 successful results");
    assert!(
        output.completed >= 2,
        "Should have at least 2 completed results"
    );
    assert!(
        output.results.len() >= 2,
        "Should have at least 2 result strings"
    );

    // Verify result format
    for result_str in &output.results {
        assert!(
            result_str.ends_with(" done"),
            "Result should end with ' done', got: {}",
            result_str
        );
    }

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check event signatures against history file (unordered for parallel ops)
    assert_nodejs_event_signatures_unordered(
        &result,
        "tests/history/parallel_min_successful.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, MinSuccessfulResult>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Parallel Min Successful with Callback Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinSuccessfulCallbackResult {
    pub required: usize,
    pub results: Vec<String>,
}

/// Handler from parallel/min_successful_callback example (without macro for testing).
///
/// Demonstrates `CompletionConfig::with_min_successful(1)` combined with callback
/// operations in each parallel branch. Since callbacks suspend execution, the
/// overall execution remains Running.
async fn parallel_min_successful_callback_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<MinSuccessfulCallbackResult, DurableError> {
    let config = MapConfig {
        max_concurrency: Some(3),
        completion_config: CompletionConfig::with_min_successful(1),
        ..Default::default()
    };

    let results = ctx
        .map(
            vec![
                "service-a".to_string(),
                "service-b".to_string(),
                "service-c".to_string(),
            ],
            |child_ctx: DurableContext, item: String, _index: usize| {
                Box::pin(async move {
                    let callback = child_ctx
                        .create_callback_named::<String>(&format!("{}_callback", item), None)
                        .await?;
                    println!("Callback for {}: {}", item, callback.callback_id);
                    callback.result().await
                })
            },
            Some(config),
        )
        .await?;

    let succeeded = results.succeeded();
    let successful_results: Vec<String> = succeeded
        .iter()
        .filter_map(|item| item.result.clone())
        .collect();

    Ok(MinSuccessfulCallbackResult {
        required: 1,
        results: successful_results,
    })
}

#[tokio::test(flavor = "current_thread")]
async fn test_parallel_min_successful_callback() {
    LocalDurableTestRunner::<serde_json::Value, MinSuccessfulCallbackResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(parallel_min_successful_callback_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // The map with min_successful(1) completes the map operation itself even though
    // individual callbacks are still pending. The execution succeeds because the map
    // returns a BatchResult (with no succeeded items since callbacks are pending).
    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should succeed — map returns BatchResult even with pending callbacks"
    );

    // Verify operations — should have callback operations from the parallel branches
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check that callback operations were created
    let callback_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Callback)
        .collect();
    assert!(
        !callback_ops.is_empty(),
        "Should have callback operations from parallel branches"
    );

    // Check event signatures against history file (unordered for parallel ops)
    assert_nodejs_event_signatures_unordered(
        &result,
        "tests/history/parallel_min_successful_callback.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, MinSuccessfulCallbackResult>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Parallel Tolerated Failure Count Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToleratedFailureResult {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
}

/// Handler from parallel/tolerated_failure_count example (without macro for testing).
///
/// Runs 6 items through `ctx.map()` where items 3 and 6 fail. Configures
/// `tolerated_failure_count: Some(2)` so the overall operation succeeds
/// because exactly 2 items fail, within the tolerance threshold.
async fn parallel_tolerated_failure_count_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<ToleratedFailureResult, DurableError> {
    let config = MapConfig {
        max_concurrency: Some(6),
        completion_config: CompletionConfig {
            tolerated_failure_count: Some(2),
            ..Default::default()
        },
        ..Default::default()
    };

    let results = ctx
        .map(
            vec![1, 2, 3, 4, 5, 6],
            |child_ctx: DurableContext, item: i32, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(
                            |_| {
                                if item == 3 || item == 6 {
                                    Err(format!("Item {} failed", item).into())
                                } else {
                                    Ok(format!("item {} ok", item))
                                }
                            },
                            None,
                        )
                        .await
                })
            },
            Some(config),
        )
        .await?;

    Ok(ToleratedFailureResult {
        total: results.total_count(),
        succeeded: results.succeeded().len(),
        failed: results.failed().len(),
    })
}

#[tokio::test(flavor = "current_thread")]
async fn test_parallel_tolerated_failure_count() {
    LocalDurableTestRunner::<serde_json::Value, ToleratedFailureResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(parallel_tolerated_failure_count_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should succeed — failures are within tolerance"
    );

    let output = result.get_result().unwrap();
    assert_eq!(output.total, 6, "Should have 6 total items");
    assert_eq!(
        output.succeeded, 4,
        "Should have 4 succeeded items (1, 2, 4, 5)"
    );
    assert_eq!(output.failed, 2, "Should have 2 failed items (3, 6)");

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check event signatures against history file (unordered for parallel ops)
    assert_nodejs_event_signatures_unordered(
        &result,
        "tests/history/parallel_tolerated_failure_count.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, ToleratedFailureResult>::teardown_test_environment(
    )
    .await
    .unwrap();
}

// ============================================================================
// Parallel Tolerated Failure Percentage Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToleratedFailurePctResult {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
}

/// Handler from parallel/tolerated_failure_pct example (without macro for testing).
///
/// Runs 10 items through `ctx.map()` where items 3 and 7 fail (20% failure rate).
/// Configures `tolerated_failure_percentage: Some(0.25)` so the overall operation
/// succeeds because 20% < 25% tolerance.
async fn parallel_tolerated_failure_pct_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<ToleratedFailurePctResult, DurableError> {
    let config = MapConfig {
        max_concurrency: Some(10),
        completion_config: CompletionConfig {
            tolerated_failure_percentage: Some(0.25),
            ..Default::default()
        },
        ..Default::default()
    };

    let results = ctx
        .map(
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            |child_ctx: DurableContext, item: i32, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(
                            |_| {
                                if item == 3 || item == 7 {
                                    Err(format!("Item {} failed", item).into())
                                } else {
                                    Ok(format!("item {} ok", item))
                                }
                            },
                            None,
                        )
                        .await
                })
            },
            Some(config),
        )
        .await?;

    Ok(ToleratedFailurePctResult {
        total: results.total_count(),
        succeeded: results.succeeded().len(),
        failed: results.failed().len(),
    })
}

#[tokio::test(flavor = "current_thread")]
async fn test_parallel_tolerated_failure_pct() {
    LocalDurableTestRunner::<serde_json::Value, ToleratedFailurePctResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(parallel_tolerated_failure_pct_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should succeed — 20% failures within 25% tolerance"
    );

    let output = result.get_result().unwrap();
    assert_eq!(output.total, 10, "Should have 10 total items");
    assert_eq!(output.succeeded, 8, "Should have 8 succeeded items");
    assert_eq!(output.failed, 2, "Should have 2 failed items (3, 7)");

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check event signatures against history file (unordered for parallel ops)
    assert_nodejs_event_signatures_unordered(
        &result,
        "tests/history/parallel_tolerated_failure_pct.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, ToleratedFailurePctResult>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Parallel Failure Threshold Exceeded Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FailureThresholdResult {
    pub threshold_exceeded: bool,
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub error_message: Option<String>,
}

/// Handler from parallel/failure_threshold_exceeded example (without macro for testing).
///
/// Runs 5 items through `ctx.map()` where items 1, 3, and 5 fail (3 failures).
/// Configures `tolerated_failure_count: Some(1)` so the operation completes
/// early when the second failure is detected (exceeding the 1-failure tolerance).
/// The handler catches the threshold exceeded condition and returns a summary.
async fn parallel_failure_threshold_exceeded_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<FailureThresholdResult, DurableError> {
    let config = MapConfig {
        max_concurrency: Some(5),
        completion_config: CompletionConfig {
            tolerated_failure_count: Some(1),
            ..Default::default()
        },
        ..Default::default()
    };

    let results = ctx
        .map(
            vec![1, 2, 3, 4, 5],
            |child_ctx: DurableContext, item: i32, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(
                            |_| {
                                if item % 2 == 1 {
                                    Err(format!("Item {} failed", item).into())
                                } else {
                                    Ok(format!("item {} ok", item))
                                }
                            },
                            None,
                        )
                        .await
                })
            },
            Some(config),
        )
        .await?;

    let threshold_exceeded = results.is_failure();
    let error_message = if threshold_exceeded {
        results.get_results().err().map(|e| e.to_string())
    } else {
        None
    };

    Ok(FailureThresholdResult {
        threshold_exceeded,
        total: results.total_count(),
        succeeded: results.succeeded().len(),
        failed: results.failed().len(),
        error_message,
    })
}

#[tokio::test(flavor = "current_thread")]
async fn test_parallel_failure_threshold_exceeded() {
    LocalDurableTestRunner::<serde_json::Value, FailureThresholdResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(parallel_failure_threshold_exceeded_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should succeed — handler catches the threshold exceeded condition"
    );

    let output = result.get_result().unwrap();
    assert!(output.threshold_exceeded, "Threshold should be exceeded");
    assert_eq!(output.total, 5, "Should have 5 total items");
    assert!(
        output.failed >= 2,
        "Should have at least 2 failed items (threshold exceeded after 2nd failure)"
    );
    assert!(
        output.error_message.is_some(),
        "Should have an error message"
    );

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check event signatures against history file (unordered for parallel ops)
    assert_nodejs_event_signatures_unordered(
        &result,
        "tests/history/parallel_failure_threshold_exceeded.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, FailureThresholdResult>::teardown_test_environment(
    )
    .await
    .unwrap();
}

// ============================================================================
// Map Empty Array Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MapEmptyResult {
    pub total_items: usize,
    pub message: String,
}

/// Handler from map/empty_array example (without macro for testing)
async fn map_empty_array_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<MapEmptyResult, DurableError> {
    let results = ctx
        .map(
            Vec::<i32>::new(),
            |child_ctx: DurableContext, _item: i32, _index: usize| {
                Box::pin(async move { child_ctx.step(|_| Ok("".to_string()), None).await })
            },
            None,
        )
        .await?;

    Ok(MapEmptyResult {
        total_items: results.items.len(),
        message: "Empty map completed successfully".to_string(),
    })
}

#[tokio::test(flavor = "current_thread")]
async fn test_map_empty_array() {
    LocalDurableTestRunner::<serde_json::Value, MapEmptyResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(map_empty_array_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully with empty input"
    );

    let output = result.get_result().unwrap();
    assert_eq!(output.total_items, 0, "Should have 0 items for empty input");
    assert_eq!(output.message, "Empty map completed successfully");

    // Check event signatures against history file
    assert_nodejs_event_signatures(&result, "tests/history/map_empty_array.history.json");

    LocalDurableTestRunner::<serde_json::Value, MapEmptyResult>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Map Error Preservation Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MapErrorPreservationResult {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub error_messages: Vec<String>,
}

/// Handler from map/error_preservation example (without macro for testing)
async fn map_error_preservation_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<MapErrorPreservationResult, DurableError> {
    let config = MapConfig {
        max_concurrency: Some(8),
        completion_config: CompletionConfig {
            tolerated_failure_count: Some(3),
            ..Default::default()
        },
        ..Default::default()
    };

    let results = ctx
        .map(
            vec![1, 2, 3, 4, 5, 6, 7, 8],
            |child_ctx: DurableContext, item: i32, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(
                            |_| {
                                if item == 2 || item == 5 || item == 8 {
                                    Err(format!("Item {} processing failed", item).into())
                                } else {
                                    Ok(format!("item {} ok", item))
                                }
                            },
                            None,
                        )
                        .await
                })
            },
            Some(config),
        )
        .await?;

    let failed = results.failed();
    let succeeded = results.succeeded();

    let error_messages: Vec<String> = failed
        .iter()
        .map(|item| {
            item.get_error()
                .map(|e| e.error_message.clone())
                .unwrap_or_else(|| "unknown error".to_string())
        })
        .collect();

    Ok(MapErrorPreservationResult {
        total: results.total_count(),
        succeeded: succeeded.len(),
        failed: failed.len(),
        error_messages,
    })
}

#[tokio::test(flavor = "current_thread")]
async fn test_map_error_preservation() {
    LocalDurableTestRunner::<serde_json::Value, MapErrorPreservationResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(map_error_preservation_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should succeed with tolerated failures"
    );

    let output = result.get_result().unwrap();
    assert_eq!(output.total, 8, "Should have 8 total items");
    assert_eq!(output.succeeded, 5, "Should have 5 succeeded items");
    assert_eq!(output.failed, 3, "Should have 3 failed items (2, 5, 8)");
    assert_eq!(
        output.error_messages.len(),
        3,
        "Should have 3 error messages"
    );

    // Verify error messages are preserved
    for msg in &output.error_messages {
        assert!(
            msg.contains("processing failed"),
            "Error message should contain the original error text, got: {}",
            msg
        );
    }

    // Check event signatures against history file (unordered for concurrent map ops)
    assert_nodejs_event_signatures_unordered(
        &result,
        "tests/history/map_error_preservation.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, MapErrorPreservationResult>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Map High Concurrency Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HighConcurrencyResult {
    pub total: usize,
    pub results: Vec<String>,
}

/// Handler from map/high_concurrency example (without macro for testing)
async fn map_high_concurrency_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<HighConcurrencyResult, DurableError> {
    let items: Vec<i32> = (1..=15).collect();

    let config = MapConfig {
        max_concurrency: Some(10),
        ..Default::default()
    };

    let results = ctx
        .map(
            items,
            |child_ctx: DurableContext, item: i32, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(|_| Ok(format!("processed_{}", item)), None)
                        .await
                })
            },
            Some(config),
        )
        .await?;

    let result_strings: Vec<String> = results
        .succeeded()
        .iter()
        .filter_map(|item| item.result.clone())
        .collect();

    Ok(HighConcurrencyResult {
        total: results.total_count(),
        results: result_strings,
    })
}

#[tokio::test(flavor = "current_thread")]
async fn test_map_high_concurrency() {
    LocalDurableTestRunner::<serde_json::Value, HighConcurrencyResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(map_high_concurrency_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully"
    );

    let output = result.get_result().unwrap();
    assert_eq!(output.total, 15, "Should have 15 total items");
    assert_eq!(output.results.len(), 15, "Should have 15 results");

    // Verify all items were processed
    for i in 1..=15 {
        let expected = format!("processed_{}", i);
        assert!(
            output.results.contains(&expected),
            "Should contain {}",
            expected
        );
    }

    // Check event signatures against history file (unordered for concurrent map ops)
    assert_nodejs_event_signatures_unordered(
        &result,
        "tests/history/map_high_concurrency.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, HighConcurrencyResult>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Map Large Scale Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LargeScaleResult {
    pub total: usize,
    pub results: Vec<i32>,
}

/// Handler from map/large_scale example (without macro for testing)
async fn map_large_scale_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<LargeScaleResult, DurableError> {
    let items: Vec<i32> = (1..=50).collect();

    let config = MapConfig {
        max_concurrency: Some(10),
        ..Default::default()
    };

    let results = ctx
        .map(
            items,
            |child_ctx: DurableContext, item: i32, _index: usize| {
                Box::pin(async move { child_ctx.step(|_| Ok(item * 2), None).await })
            },
            Some(config),
        )
        .await?;

    let result_values: Vec<i32> = results
        .succeeded()
        .iter()
        .filter_map(|item| item.result.clone())
        .collect();

    Ok(LargeScaleResult {
        total: results.total_count(),
        results: result_values,
    })
}

#[tokio::test(flavor = "current_thread")]
async fn test_map_large_scale() {
    LocalDurableTestRunner::<serde_json::Value, LargeScaleResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(map_large_scale_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully"
    );

    let output = result.get_result().unwrap();
    assert_eq!(output.total, 50, "Should have 50 total items");
    assert_eq!(output.results.len(), 50, "Should have 50 results");

    // Verify all items were doubled
    for i in 1..=50 {
        assert!(
            output.results.contains(&(i * 2)),
            "Should contain {} (doubled from {})",
            i * 2,
            i
        );
    }

    // Check event signatures against history file (unordered for concurrent map ops)
    assert_nodejs_event_signatures_unordered(&result, "tests/history/map_large_scale.history.json");

    LocalDurableTestRunner::<serde_json::Value, LargeScaleResult>::teardown_test_environment()
        .await
        .unwrap();
}
