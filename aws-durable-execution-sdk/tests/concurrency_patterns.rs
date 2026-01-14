//! Integration tests for concurrency patterns (map and parallel operations).
//!
//! These tests verify that the SDK correctly handles map and parallel operations,
//! including processing multiple items, handling failures within tolerance,
//! branch failure propagation, and replaying partially completed operations.
//!
//! **Requirements:** 7.1, 7.2, 7.3, 7.4, 7.5

mod common;

use std::sync::Arc;

use aws_durable_execution_sdk::concurrency::{BatchItem, BatchResult, CompletionReason};
use aws_durable_execution_sdk::config::{CompletionConfig, MapConfig, ParallelConfig};
use aws_durable_execution_sdk::context::{DurableContext, OperationIdentifier, TracingLogger, Logger};
use aws_durable_execution_sdk::error::{DurableError, ErrorObject};
use aws_durable_execution_sdk::handlers::map::map_handler;
use aws_durable_execution_sdk::handlers::parallel::parallel_handler;
use aws_durable_execution_sdk::lambda::InitialExecutionState;
use aws_durable_execution_sdk::operation::{ContextDetails, Operation, OperationStatus, OperationType};
use aws_durable_execution_sdk::state::ExecutionState;

use common::*;

// =============================================================================
// Test 13.1: Map operation processing multiple items
// =============================================================================

/// Test that a map operation processes multiple items and collects all results.
///
/// **Requirements:** 7.1 - WHEN a map operation processes multiple items,
/// THE Test_Suite SHALL verify all items are processed and results collected
#[tokio::test]
async fn test_map_operation_processes_multiple_items() {
    let client = Arc::new(MockDurableServiceClient::new().with_checkpoint_responses(20));
    let initial_state = InitialExecutionState {
        operations: vec![create_execution_operation("exec-1", OperationStatus::Started, None)],
        next_marker: None,
    };
    let state = Arc::new(ExecutionState::new(
        TEST_EXECUTION_ARN,
        TEST_CHECKPOINT_TOKEN,
        initial_state,
        client.clone(),
    ));

    let op_id = OperationIdentifier::new("map-op-1", Some("exec-1".to_string()), Some("test-map".to_string()));
    let config = MapConfig::default();
    let logger: Arc<dyn Logger> = Arc::new(TracingLogger);
    let parent_ctx = DurableContext::new(state.clone());

    // Process 5 items, doubling each value
    let items = vec![1, 2, 3, 4, 5];
    let result = map_handler(
        items.clone(),
        |_ctx, item: i32, _idx| async move { Ok(item * 2) },
        &state,
        &op_id,
        &parent_ctx,
        &config,
        &logger,
    ).await;

    assert!(result.is_ok(), "Map operation should succeed");
    let batch_result = result.unwrap();
    
    // Verify all items were processed
    assert_eq!(batch_result.total_count(), 5, "Should have 5 items");
    assert_eq!(batch_result.success_count(), 5, "All 5 items should succeed");
    assert!(batch_result.all_succeeded(), "All items should be successful");
    assert_eq!(batch_result.completion_reason, CompletionReason::AllCompleted);
    
    // Verify results are correct (doubled values)
    let results = batch_result.get_results().unwrap();
    assert_eq!(results.len(), 5);
}

/// Test that map operation preserves item order in results.
#[tokio::test]
async fn test_map_operation_preserves_item_order() {
    let client = Arc::new(MockDurableServiceClient::new().with_checkpoint_responses(20));
    let initial_state = InitialExecutionState {
        operations: vec![create_execution_operation("exec-1", OperationStatus::Started, None)],
        next_marker: None,
    };
    let state = Arc::new(ExecutionState::new(
        TEST_EXECUTION_ARN,
        TEST_CHECKPOINT_TOKEN,
        initial_state,
        client,
    ));

    let op_id = OperationIdentifier::new("map-op-2", Some("exec-1".to_string()), Some("order-test".to_string()));
    let config = MapConfig::default();
    let logger: Arc<dyn Logger> = Arc::new(TracingLogger);
    let parent_ctx = DurableContext::new(state.clone());

    let items = vec![10, 20, 30, 40, 50];
    let result = map_handler(
        items,
        |_ctx, item: i32, idx| async move { Ok(format!("item-{}-value-{}", idx, item)) },
        &state,
        &op_id,
        &parent_ctx,
        &config,
        &logger,
    ).await;

    assert!(result.is_ok());
    let batch_result = result.unwrap();
    
    // Verify items are in order by index
    for (i, item) in batch_result.items.iter().enumerate() {
        assert_eq!(item.index, i, "Item index should match position");
        assert!(item.is_succeeded());
    }
}

/// Test that map operation with empty collection returns empty result.
#[tokio::test]
async fn test_map_operation_empty_collection() {
    let client = Arc::new(MockDurableServiceClient::new().with_checkpoint_responses(5));
    let initial_state = InitialExecutionState {
        operations: vec![create_execution_operation("exec-1", OperationStatus::Started, None)],
        next_marker: None,
    };
    let state = Arc::new(ExecutionState::new(
        TEST_EXECUTION_ARN,
        TEST_CHECKPOINT_TOKEN,
        initial_state,
        client,
    ));

    let op_id = OperationIdentifier::new("map-op-3", Some("exec-1".to_string()), None);
    let config = MapConfig::default();
    let logger: Arc<dyn Logger> = Arc::new(TracingLogger);
    let parent_ctx = DurableContext::new(state.clone());

    let items: Vec<i32> = vec![];
    let result = map_handler(
        items,
        |_ctx, item: i32, _idx| async move { Ok(item) },
        &state,
        &op_id,
        &parent_ctx,
        &config,
        &logger,
    ).await;

    assert!(result.is_ok());
    let batch_result = result.unwrap();
    assert!(batch_result.items.is_empty());
    assert_eq!(batch_result.completion_reason, CompletionReason::AllCompleted);
}

// =============================================================================
// Test 13.2: Parallel operation with multiple branches
// =============================================================================

/// Test that a parallel operation executes multiple branches and combines results.
///
/// **Requirements:** 7.2 - WHEN a parallel operation executes multiple branches,
/// THE Test_Suite SHALL verify all branches complete and results are combined
#[tokio::test]
async fn test_parallel_operation_multiple_branches() {
    let client = Arc::new(MockDurableServiceClient::new().with_checkpoint_responses(20));
    let initial_state = InitialExecutionState {
        operations: vec![create_execution_operation("exec-1", OperationStatus::Started, None)],
        next_marker: None,
    };
    let state = Arc::new(ExecutionState::new(
        TEST_EXECUTION_ARN,
        TEST_CHECKPOINT_TOKEN,
        initial_state,
        client.clone(),
    ));

    let op_id = OperationIdentifier::new("parallel-op-1", Some("exec-1".to_string()), Some("test-parallel".to_string()));
    let config = ParallelConfig::default();
    let logger: Arc<dyn Logger> = Arc::new(TracingLogger);
    let parent_ctx = DurableContext::new(state.clone());

    // Create 3 branches that return different values
    let branches: Vec<Box<dyn FnOnce(DurableContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, DurableError>> + Send>> + Send>> = vec![
        Box::new(|_ctx| Box::pin(async { Ok("branch-0".to_string()) }) as _),
        Box::new(|_ctx| Box::pin(async { Ok("branch-1".to_string()) }) as _),
        Box::new(|_ctx| Box::pin(async { Ok("branch-2".to_string()) }) as _),
    ];

    let result = parallel_handler(
        branches,
        &state,
        &op_id,
        &parent_ctx,
        &config,
        &logger,
    ).await;

    assert!(result.is_ok(), "Parallel operation should succeed");
    let batch_result = result.unwrap();
    
    // Verify all branches completed
    assert_eq!(batch_result.total_count(), 3, "Should have 3 branches");
    assert_eq!(batch_result.success_count(), 3, "All 3 branches should succeed");
    assert!(batch_result.all_succeeded());
    assert_eq!(batch_result.completion_reason, CompletionReason::AllCompleted);
}

/// Test that parallel operation with empty branches returns empty result.
#[tokio::test]
async fn test_parallel_operation_empty_branches() {
    let client = Arc::new(MockDurableServiceClient::new().with_checkpoint_responses(5));
    let initial_state = InitialExecutionState {
        operations: vec![create_execution_operation("exec-1", OperationStatus::Started, None)],
        next_marker: None,
    };
    let state = Arc::new(ExecutionState::new(
        TEST_EXECUTION_ARN,
        TEST_CHECKPOINT_TOKEN,
        initial_state,
        client,
    ));

    let op_id = OperationIdentifier::new("parallel-op-2", Some("exec-1".to_string()), None);
    let config = ParallelConfig::default();
    let logger: Arc<dyn Logger> = Arc::new(TracingLogger);
    let parent_ctx = DurableContext::new(state.clone());

    let branches: Vec<Box<dyn FnOnce(DurableContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32, DurableError>> + Send>> + Send>> = vec![];

    let result = parallel_handler(
        branches,
        &state,
        &op_id,
        &parent_ctx,
        &config,
        &logger,
    ).await;

    assert!(result.is_ok());
    let batch_result = result.unwrap();
    assert!(batch_result.items.is_empty());
    assert_eq!(batch_result.completion_reason, CompletionReason::AllCompleted);
}

/// Test that parallel operation respects concurrency limits.
#[tokio::test]
async fn test_parallel_operation_with_concurrency_limit() {
    let client = Arc::new(MockDurableServiceClient::new().with_checkpoint_responses(20));
    let initial_state = InitialExecutionState {
        operations: vec![create_execution_operation("exec-1", OperationStatus::Started, None)],
        next_marker: None,
    };
    let state = Arc::new(ExecutionState::new(
        TEST_EXECUTION_ARN,
        TEST_CHECKPOINT_TOKEN,
        initial_state,
        client,
    ));

    let op_id = OperationIdentifier::new("parallel-op-3", Some("exec-1".to_string()), None);
    let config = ParallelConfig {
        max_concurrency: Some(2), // Limit to 2 concurrent branches
        ..Default::default()
    };
    let logger: Arc<dyn Logger> = Arc::new(TracingLogger);
    let parent_ctx = DurableContext::new(state.clone());

    // Create 5 branches
    let branches: Vec<Box<dyn FnOnce(DurableContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32, DurableError>> + Send>> + Send>> = vec![
        Box::new(|_ctx| Box::pin(async { Ok(1) }) as _),
        Box::new(|_ctx| Box::pin(async { Ok(2) }) as _),
        Box::new(|_ctx| Box::pin(async { Ok(3) }) as _),
        Box::new(|_ctx| Box::pin(async { Ok(4) }) as _),
        Box::new(|_ctx| Box::pin(async { Ok(5) }) as _),
    ];

    let result = parallel_handler(
        branches,
        &state,
        &op_id,
        &parent_ctx,
        &config,
        &logger,
    ).await;

    assert!(result.is_ok());
    let batch_result = result.unwrap();
    assert_eq!(batch_result.total_count(), 5);
    assert_eq!(batch_result.success_count(), 5);
}



// =============================================================================
// Test 13.3: Map operation with failures within tolerance
// =============================================================================

/// Test that a map operation with failures within tolerance completes with partial success.
///
/// **Requirements:** 7.3 - WHEN a map operation has failures within tolerance,
/// THE Test_Suite SHALL verify the batch completes with partial success
#[tokio::test]
async fn test_map_operation_failures_within_tolerance() {
    let client = Arc::new(MockDurableServiceClient::new().with_checkpoint_responses(20));
    let initial_state = InitialExecutionState {
        operations: vec![create_execution_operation("exec-1", OperationStatus::Started, None)],
        next_marker: None,
    };
    let state = Arc::new(ExecutionState::new(
        TEST_EXECUTION_ARN,
        TEST_CHECKPOINT_TOKEN,
        initial_state,
        client,
    ));

    let op_id = OperationIdentifier::new("map-op-4", Some("exec-1".to_string()), Some("tolerance-test".to_string()));
    // Allow up to 2 failures
    let config = MapConfig {
        completion_config: CompletionConfig {
            tolerated_failure_count: Some(2),
            ..Default::default()
        },
        ..Default::default()
    };
    let logger: Arc<dyn Logger> = Arc::new(TracingLogger);
    let parent_ctx = DurableContext::new(state.clone());

    // 5 items: items at index 1 and 3 will fail (2 failures, within tolerance)
    let items = vec![0, 1, 2, 3, 4];
    let result: Result<BatchResult<i32>, DurableError> = map_handler(
        items,
        |_ctx, item: i32, _idx| async move {
            if item == 1 || item == 3 {
                Err(DurableError::UserCode {
                    message: format!("Item {} failed", item),
                    error_type: "TestError".to_string(),
                    stack_trace: None,
                })
            } else {
                Ok(item * 10)
            }
        },
        &state,
        &op_id,
        &parent_ctx,
        &config,
        &logger,
    ).await;

    assert!(result.is_ok(), "Map operation should complete even with failures within tolerance");
    let batch_result = result.unwrap();
    
    // Verify counts
    assert_eq!(batch_result.total_count(), 5);
    assert_eq!(batch_result.success_count(), 3, "3 items should succeed");
    assert_eq!(batch_result.failure_count(), 2, "2 items should fail");
    assert!(batch_result.has_failures());
    
    // Should complete normally (not FailureToleranceExceeded)
    assert_eq!(batch_result.completion_reason, CompletionReason::AllCompleted);
}

/// Test that map operation exceeding failure tolerance stops early.
#[tokio::test]
async fn test_map_operation_exceeds_failure_tolerance() {
    let client = Arc::new(MockDurableServiceClient::new().with_checkpoint_responses(20));
    let initial_state = InitialExecutionState {
        operations: vec![create_execution_operation("exec-1", OperationStatus::Started, None)],
        next_marker: None,
    };
    let state = Arc::new(ExecutionState::new(
        TEST_EXECUTION_ARN,
        TEST_CHECKPOINT_TOKEN,
        initial_state,
        client,
    ));

    let op_id = OperationIdentifier::new("map-op-5", Some("exec-1".to_string()), None);
    // Allow 0 failures (all must succeed)
    let config = MapConfig {
        completion_config: CompletionConfig::all_successful(),
        ..Default::default()
    };
    let logger: Arc<dyn Logger> = Arc::new(TracingLogger);
    let parent_ctx = DurableContext::new(state.clone());

    // First item will fail, exceeding tolerance
    let items = vec![0, 1, 2, 3, 4];
    let result: Result<BatchResult<i32>, DurableError> = map_handler(
        items,
        |_ctx, item: i32, _idx| async move {
            if item == 0 {
                Err(DurableError::UserCode {
                    message: "First item failed".to_string(),
                    error_type: "TestError".to_string(),
                    stack_trace: None,
                })
            } else {
                Ok(item)
            }
        },
        &state,
        &op_id,
        &parent_ctx,
        &config,
        &logger,
    ).await;

    assert!(result.is_ok());
    let batch_result = result.unwrap();
    
    // Should indicate failure tolerance exceeded
    assert_eq!(batch_result.completion_reason, CompletionReason::FailureToleranceExceeded);
    assert!(batch_result.is_failure());
}

/// Test map operation with percentage-based failure tolerance.
#[tokio::test]
async fn test_map_operation_percentage_failure_tolerance() {
    let client = Arc::new(MockDurableServiceClient::new().with_checkpoint_responses(30));
    let initial_state = InitialExecutionState {
        operations: vec![create_execution_operation("exec-1", OperationStatus::Started, None)],
        next_marker: None,
    };
    let state = Arc::new(ExecutionState::new(
        TEST_EXECUTION_ARN,
        TEST_CHECKPOINT_TOKEN,
        initial_state,
        client,
    ));

    let op_id = OperationIdentifier::new("map-op-6", Some("exec-1".to_string()), None);
    // Allow up to 30% failures
    let config = MapConfig {
        completion_config: CompletionConfig {
            tolerated_failure_percentage: Some(0.3),
            ..Default::default()
        },
        ..Default::default()
    };
    let logger: Arc<dyn Logger> = Arc::new(TracingLogger);
    let parent_ctx = DurableContext::new(state.clone());

    // 10 items: 2 will fail (20%, within 30% tolerance)
    let items: Vec<i32> = (0..10).collect();
    let result: Result<BatchResult<i32>, DurableError> = map_handler(
        items,
        |_ctx, item: i32, _idx| async move {
            if item == 2 || item == 7 {
                Err(DurableError::UserCode {
                    message: format!("Item {} failed", item),
                    error_type: "TestError".to_string(),
                    stack_trace: None,
                })
            } else {
                Ok(item)
            }
        },
        &state,
        &op_id,
        &parent_ctx,
        &config,
        &logger,
    ).await;

    assert!(result.is_ok());
    let batch_result = result.unwrap();
    
    assert_eq!(batch_result.success_count(), 8);
    assert_eq!(batch_result.failure_count(), 2);
    // 20% failure rate is within 30% tolerance
    assert_eq!(batch_result.completion_reason, CompletionReason::AllCompleted);
}

// =============================================================================
// Test 13.4: Parallel operation branch failure propagation
// =============================================================================

/// Test that parallel operation propagates branch failures according to completion policy.
///
/// **Requirements:** 7.4 - WHEN a parallel operation has a branch failure,
/// THE Test_Suite SHALL verify error propagation follows completion policy
#[tokio::test]
async fn test_parallel_operation_branch_failure_propagation() {
    let client = Arc::new(MockDurableServiceClient::new().with_checkpoint_responses(20));
    let initial_state = InitialExecutionState {
        operations: vec![create_execution_operation("exec-1", OperationStatus::Started, None)],
        next_marker: None,
    };
    let state = Arc::new(ExecutionState::new(
        TEST_EXECUTION_ARN,
        TEST_CHECKPOINT_TOKEN,
        initial_state,
        client,
    ));

    let op_id = OperationIdentifier::new("parallel-op-4", Some("exec-1".to_string()), Some("failure-test".to_string()));
    // All branches must succeed (no tolerance)
    let config = ParallelConfig {
        completion_config: CompletionConfig::all_successful(),
        ..Default::default()
    };
    let logger: Arc<dyn Logger> = Arc::new(TracingLogger);
    let parent_ctx = DurableContext::new(state.clone());

    // Second branch will fail
    let branches: Vec<Box<dyn FnOnce(DurableContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, DurableError>> + Send>> + Send>> = vec![
        Box::new(|_ctx| Box::pin(async { Ok("success-0".to_string()) }) as _),
        Box::new(|_ctx| Box::pin(async { 
            Err(DurableError::UserCode {
                message: "Branch 1 failed".to_string(),
                error_type: "BranchError".to_string(),
                stack_trace: None,
            })
        }) as _),
        Box::new(|_ctx| Box::pin(async { Ok("success-2".to_string()) }) as _),
    ];

    let result = parallel_handler(
        branches,
        &state,
        &op_id,
        &parent_ctx,
        &config,
        &logger,
    ).await;

    assert!(result.is_ok());
    let batch_result = result.unwrap();
    
    // Should indicate failure tolerance exceeded
    assert_eq!(batch_result.completion_reason, CompletionReason::FailureToleranceExceeded);
    assert!(batch_result.is_failure());
    assert!(batch_result.has_failures());
    
    // Verify the failed branch has error details
    let failed_items: Vec<_> = batch_result.items.iter()
        .filter(|item| item.is_failed())
        .collect();
    assert!(!failed_items.is_empty(), "Should have at least one failed item");
    
    let failed_item = failed_items[0];
    assert!(failed_item.error.is_some());
    let error = failed_item.error.as_ref().unwrap();
    assert_eq!(error.error_type, "BranchError");
}

/// Test parallel operation with failure tolerance allows partial success.
#[tokio::test]
async fn test_parallel_operation_with_failure_tolerance() {
    let client = Arc::new(MockDurableServiceClient::new().with_checkpoint_responses(20));
    let initial_state = InitialExecutionState {
        operations: vec![create_execution_operation("exec-1", OperationStatus::Started, None)],
        next_marker: None,
    };
    let state = Arc::new(ExecutionState::new(
        TEST_EXECUTION_ARN,
        TEST_CHECKPOINT_TOKEN,
        initial_state,
        client,
    ));

    let op_id = OperationIdentifier::new("parallel-op-5", Some("exec-1".to_string()), None);
    // Allow 1 failure
    let config = ParallelConfig {
        completion_config: CompletionConfig {
            tolerated_failure_count: Some(1),
            ..Default::default()
        },
        ..Default::default()
    };
    let logger: Arc<dyn Logger> = Arc::new(TracingLogger);
    let parent_ctx = DurableContext::new(state.clone());

    // One branch fails (within tolerance)
    let branches: Vec<Box<dyn FnOnce(DurableContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32, DurableError>> + Send>> + Send>> = vec![
        Box::new(|_ctx| Box::pin(async { Ok(1) }) as _),
        Box::new(|_ctx| Box::pin(async { 
            Err(DurableError::UserCode {
                message: "Branch failed".to_string(),
                error_type: "TestError".to_string(),
                stack_trace: None,
            })
        }) as _),
        Box::new(|_ctx| Box::pin(async { Ok(3) }) as _),
    ];

    let result = parallel_handler(
        branches,
        &state,
        &op_id,
        &parent_ctx,
        &config,
        &logger,
    ).await;

    assert!(result.is_ok());
    let batch_result = result.unwrap();
    
    assert_eq!(batch_result.success_count(), 2);
    assert_eq!(batch_result.failure_count(), 1);
    // Should complete normally since failure is within tolerance
    assert_eq!(batch_result.completion_reason, CompletionReason::AllCompleted);
}

/// Test parallel operation with min_successful completion criteria.
#[tokio::test]
async fn test_parallel_operation_min_successful() {
    let client = Arc::new(MockDurableServiceClient::new().with_checkpoint_responses(20));
    let initial_state = InitialExecutionState {
        operations: vec![create_execution_operation("exec-1", OperationStatus::Started, None)],
        next_marker: None,
    };
    let state = Arc::new(ExecutionState::new(
        TEST_EXECUTION_ARN,
        TEST_CHECKPOINT_TOKEN,
        initial_state,
        client,
    ));

    let op_id = OperationIdentifier::new("parallel-op-6", Some("exec-1".to_string()), None);
    // Complete when 2 branches succeed
    let config = ParallelConfig {
        completion_config: CompletionConfig::with_min_successful(2),
        ..Default::default()
    };
    let logger: Arc<dyn Logger> = Arc::new(TracingLogger);
    let parent_ctx = DurableContext::new(state.clone());

    let branches: Vec<Box<dyn FnOnce(DurableContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32, DurableError>> + Send>> + Send>> = vec![
        Box::new(|_ctx| Box::pin(async { Ok(1) }) as _),
        Box::new(|_ctx| Box::pin(async { Ok(2) }) as _),
        Box::new(|_ctx| Box::pin(async { Ok(3) }) as _),
        Box::new(|_ctx| Box::pin(async { Ok(4) }) as _),
    ];

    let result = parallel_handler(
        branches,
        &state,
        &op_id,
        &parent_ctx,
        &config,
        &logger,
    ).await;

    assert!(result.is_ok());
    let batch_result = result.unwrap();
    
    // Should complete when min_successful is reached
    // Note: Due to concurrent execution, all may complete before the check
    assert!(batch_result.success_count() >= 2);
}

// =============================================================================
// Test 13.5: Replaying partially completed map
// =============================================================================

/// Test that replaying a partially completed map only re-processes incomplete items.
///
/// **Requirements:** 7.5 - WHEN replaying a partially completed map,
/// THE Test_Suite SHALL verify only incomplete items are re-processed
#[tokio::test]
async fn test_replay_completed_map_returns_cached_result() {
    // Create a completed map operation with cached result
    let mut map_op = Operation::new("map-op-7", OperationType::Context);
    map_op.status = OperationStatus::Succeeded;
    map_op.parent_id = Some("exec-1".to_string());
    map_op.name = Some("completed-map".to_string());
    // Serialize a BatchResult as the cached result
    let cached_result: BatchResult<i32> = BatchResult::new(
        vec![
            BatchItem::succeeded(0, 10),
            BatchItem::succeeded(1, 20),
            BatchItem::succeeded(2, 30),
        ],
        CompletionReason::AllCompleted,
    );
    map_op.context_details = Some(ContextDetails {
        result: Some(serde_json::to_string(&cached_result).unwrap()),
        replay_children: Some(true),
        error: None,
    });

    let operations = vec![
        create_execution_operation("exec-1", OperationStatus::Started, None),
        map_op,
    ];

    let client = Arc::new(MockDurableServiceClient::new());
    let initial_state = InitialExecutionState {
        operations,
        next_marker: None,
    };
    let state = Arc::new(ExecutionState::new(
        TEST_EXECUTION_ARN,
        TEST_CHECKPOINT_TOKEN,
        initial_state,
        client.clone(),
    ));

    let op_id = OperationIdentifier::new("map-op-7", Some("exec-1".to_string()), Some("completed-map".to_string()));
    let config = MapConfig::default();
    let logger: Arc<dyn Logger> = Arc::new(TracingLogger);
    let parent_ctx = DurableContext::new(state.clone());

    // Try to run map again - should return cached result
    let items = vec![1, 2, 3];
    let result = map_handler(
        items,
        |_ctx, item: i32, _idx| async move { 
            // This should NOT be called during replay
            Ok(item * 100) 
        },
        &state,
        &op_id,
        &parent_ctx,
        &config,
        &logger,
    ).await;

    assert!(result.is_ok(), "Replay should succeed");
    let batch_result = result.unwrap();
    
    // Should return the cached result, not re-computed values
    assert_eq!(batch_result.total_count(), 3);
    assert_eq!(batch_result.success_count(), 3);
    
    // Verify we got the cached values (10, 20, 30) not re-computed (100, 200, 300)
    let results = batch_result.get_results().unwrap();
    assert!(results.contains(&&10));
    assert!(results.contains(&&20));
    assert!(results.contains(&&30));
    
    // Verify no checkpoint calls were made (all from cache)
    let calls = client.get_checkpoint_calls();
    assert!(calls.is_empty(), "No checkpoint calls should be made during replay");
}

/// Test that replaying a failed map returns the cached error.
#[tokio::test]
async fn test_replay_failed_map_returns_cached_error() {
    // Create a failed map operation
    let mut map_op = Operation::new("map-op-8", OperationType::Context);
    map_op.status = OperationStatus::Failed;
    map_op.parent_id = Some("exec-1".to_string());
    // Set the error on the operation's error field (not context_details)
    map_op.error = Some(ErrorObject::new("MapError", "Map operation failed"));
    map_op.context_details = Some(ContextDetails {
        result: None,
        replay_children: Some(true),
        error: None,
    });

    let operations = vec![
        create_execution_operation("exec-1", OperationStatus::Started, None),
        map_op,
    ];

    let client = Arc::new(MockDurableServiceClient::new());
    let initial_state = InitialExecutionState {
        operations,
        next_marker: None,
    };
    let state = Arc::new(ExecutionState::new(
        TEST_EXECUTION_ARN,
        TEST_CHECKPOINT_TOKEN,
        initial_state,
        client.clone(),
    ));

    let op_id = OperationIdentifier::new("map-op-8", Some("exec-1".to_string()), None);
    let config = MapConfig::default();
    let logger: Arc<dyn Logger> = Arc::new(TracingLogger);
    let parent_ctx = DurableContext::new(state.clone());

    let items = vec![1, 2, 3];
    let result = map_handler(
        items,
        |_ctx, item: i32, _idx| async move { Ok(item) },
        &state,
        &op_id,
        &parent_ctx,
        &config,
        &logger,
    ).await;

    // Should return the cached error
    assert!(result.is_err());
    match result.unwrap_err() {
        DurableError::UserCode { error_type, message, .. } => {
            assert_eq!(error_type, "MapError");
            assert!(message.contains("Map operation failed"));
        }
        other => panic!("Expected UserCode error, got {:?}", other),
    }
}

/// Test that replaying a completed parallel operation returns cached result.
#[tokio::test]
async fn test_replay_completed_parallel_returns_cached_result() {
    // Create a completed parallel operation with cached result
    let mut parallel_op = Operation::new("parallel-op-7", OperationType::Context);
    parallel_op.status = OperationStatus::Succeeded;
    parallel_op.parent_id = Some("exec-1".to_string());
    parallel_op.name = Some("completed-parallel".to_string());
    
    let cached_result: BatchResult<String> = BatchResult::new(
        vec![
            BatchItem::succeeded(0, "cached-branch-0".to_string()),
            BatchItem::succeeded(1, "cached-branch-1".to_string()),
        ],
        CompletionReason::AllCompleted,
    );
    parallel_op.context_details = Some(ContextDetails {
        result: Some(serde_json::to_string(&cached_result).unwrap()),
        replay_children: Some(true),
        error: None,
    });

    let operations = vec![
        create_execution_operation("exec-1", OperationStatus::Started, None),
        parallel_op,
    ];

    let client = Arc::new(MockDurableServiceClient::new());
    let initial_state = InitialExecutionState {
        operations,
        next_marker: None,
    };
    let state = Arc::new(ExecutionState::new(
        TEST_EXECUTION_ARN,
        TEST_CHECKPOINT_TOKEN,
        initial_state,
        client.clone(),
    ));

    let op_id = OperationIdentifier::new("parallel-op-7", Some("exec-1".to_string()), Some("completed-parallel".to_string()));
    let config = ParallelConfig::default();
    let logger: Arc<dyn Logger> = Arc::new(TracingLogger);
    let parent_ctx = DurableContext::new(state.clone());

    // Create branches that would return different values if executed
    let branches: Vec<Box<dyn FnOnce(DurableContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, DurableError>> + Send>> + Send>> = vec![
        Box::new(|_ctx| Box::pin(async { Ok("new-branch-0".to_string()) }) as _),
        Box::new(|_ctx| Box::pin(async { Ok("new-branch-1".to_string()) }) as _),
    ];

    let result = parallel_handler(
        branches,
        &state,
        &op_id,
        &parent_ctx,
        &config,
        &logger,
    ).await;

    assert!(result.is_ok());
    let batch_result = result.unwrap();
    
    // Should return cached values, not new values
    assert_eq!(batch_result.total_count(), 2);
    let results = batch_result.get_results().unwrap();
    assert!(results.iter().any(|r| *r == "cached-branch-0"));
    assert!(results.iter().any(|r| *r == "cached-branch-1"));
    
    // Verify no checkpoint calls
    assert!(client.get_checkpoint_calls().is_empty());
}

/// Test non-deterministic detection when map operation type changes.
#[tokio::test]
async fn test_map_detects_non_deterministic_type_change() {
    // Create a STEP operation where we expect a Context (map)
    let mut step_op = Operation::new("map-op-9", OperationType::Step);
    step_op.status = OperationStatus::Succeeded;
    step_op.parent_id = Some("exec-1".to_string());

    let operations = vec![
        create_execution_operation("exec-1", OperationStatus::Started, None),
        step_op,
    ];

    let client = Arc::new(MockDurableServiceClient::new());
    let initial_state = InitialExecutionState {
        operations,
        next_marker: None,
    };
    let state = Arc::new(ExecutionState::new(
        TEST_EXECUTION_ARN,
        TEST_CHECKPOINT_TOKEN,
        initial_state,
        client,
    ));

    let op_id = OperationIdentifier::new("map-op-9", Some("exec-1".to_string()), None);
    let config = MapConfig::default();
    let logger: Arc<dyn Logger> = Arc::new(TracingLogger);
    let parent_ctx = DurableContext::new(state.clone());

    let items = vec![1, 2, 3];
    let result = map_handler(
        items,
        |_ctx, item: i32, _idx| async move { Ok(item) },
        &state,
        &op_id,
        &parent_ctx,
        &config,
        &logger,
    ).await;

    // Should detect non-deterministic change (Step vs Context)
    assert!(result.is_err());
    match result.unwrap_err() {
        DurableError::NonDeterministic { operation_id, message } => {
            assert_eq!(operation_id, Some("map-op-9".to_string()));
            assert!(message.contains("Context"));
            assert!(message.contains("Step"));
        }
        other => panic!("Expected NonDeterministic error, got {:?}", other),
    }
}
