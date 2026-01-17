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
    CompletionConfig, DurableContext, DurableError, Duration, MapConfig, OperationType,
};
use aws_durable_execution_sdk_examples::test_helper::{assert_event_signatures, assert_event_signatures_unordered};
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

    // Check event signatures
    assert_event_signatures(operations, "tests/history/parallel_basic.history.json");

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

    // Check event signatures
    assert_event_signatures(
        operations,
        "tests/history/parallel_heterogeneous.history.json",
    );
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

    // Check event signatures
    assert_event_signatures(
        operations,
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

    // Check event signatures
    assert_event_signatures(operations, "tests/history/map_basic.history.json");
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

    // Check event signatures (unordered because concurrency can affect operation order)
    assert_event_signatures_unordered(operations, "tests/history/map_with_concurrency.history.json");
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
    assert_eq!(batch_result.successful, 18, "Should have 18 successful items");

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check event signatures (unordered because concurrency can affect operation order)
    assert_event_signatures_unordered(
        operations,
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
    assert_eq!(quorum_result.required, 3, "Should require 3 (majority of 5)");
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

    // Check event signatures
    assert_event_signatures(operations, "tests/history/map_min_successful.history.json");
}
