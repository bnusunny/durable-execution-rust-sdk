//! Integration tests for replay scenarios.
//!
//! These tests verify that the SDK correctly handles multi-step workflow replays,
//! including returning cached results, suspending at pending operations, handling
//! mixed operation types, detecting non-deterministic changes, and preserving
//! parent-child relationships in nested contexts.
//!
//! **Requirements:** 6.1, 6.2, 6.3, 6.4, 6.5

mod common;

use std::sync::Arc;

use aws_durable_execution_sdk::error::DurableError;
use aws_durable_execution_sdk::handlers::replay::{check_replay, ReplayResult};
use aws_durable_execution_sdk::lambda::InitialExecutionState;
use aws_durable_execution_sdk::operation::{Operation, OperationStatus, OperationType};
use aws_durable_execution_sdk::state::ExecutionState;

use common::*;

// =============================================================================
// Test 12.1: Replaying workflow with multiple completed steps
// =============================================================================

/// Test that replaying a workflow with multiple completed steps returns cached
/// results without re-execution.
///
/// **Requirements:** 6.1 - WHEN replaying a workflow with multiple completed steps,
/// THE Test_Suite SHALL verify all steps return cached results without re-execution
#[tokio::test]
async fn test_replay_multiple_completed_steps_returns_cached_results() {
    // Create operations representing a workflow with 3 completed steps
    let operations = vec![
        create_execution_operation("exec-1", OperationStatus::Started, Some("{\"input\":\"test\"}")),
        create_completed_step("step-1", "\"result-1\""),
        create_completed_step("step-2", "\"result-2\""),
        create_completed_step("step-3", "\"result-3\""),
    ];

    // Create mock client that should NOT be called (since we're replaying)
    let client = Arc::new(MockDurableServiceClient::new());

    // Create execution state with the completed operations
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

    // Verify state is in replay mode
    assert!(state.is_replay(), "State should be in replay mode");

    // Check replay for each step - should return cached results
    let checkpoint1 = state.get_checkpoint_result("step-1").await;
    let result1: Result<ReplayResult<String>, DurableError> = check_replay(
        &checkpoint1,
        OperationType::Step,
        "step-1",
        TEST_EXECUTION_ARN,
    );
    assert!(result1.is_ok());
    match result1.unwrap() {
        ReplayResult::Replayed(value) => assert_eq!(value, "result-1"),
        _ => panic!("Expected Replayed result for step-1"),
    }

    let checkpoint2 = state.get_checkpoint_result("step-2").await;
    let result2: Result<ReplayResult<String>, DurableError> = check_replay(
        &checkpoint2,
        OperationType::Step,
        "step-2",
        TEST_EXECUTION_ARN,
    );
    assert!(result2.is_ok());
    match result2.unwrap() {
        ReplayResult::Replayed(value) => assert_eq!(value, "result-2"),
        _ => panic!("Expected Replayed result for step-2"),
    }

    let checkpoint3 = state.get_checkpoint_result("step-3").await;
    let result3: Result<ReplayResult<String>, DurableError> = check_replay(
        &checkpoint3,
        OperationType::Step,
        "step-3",
        TEST_EXECUTION_ARN,
    );
    assert!(result3.is_ok());
    match result3.unwrap() {
        ReplayResult::Replayed(value) => assert_eq!(value, "result-3"),
        _ => panic!("Expected Replayed result for step-3"),
    }

    // Verify no checkpoint calls were made (all results came from cache)
    let calls = client.get_checkpoint_calls();
    assert!(calls.is_empty(), "No checkpoint calls should be made during replay");
}

/// Test that replaying preserves the order of results.
#[tokio::test]
async fn test_replay_preserves_result_order() {
    // Create operations with numeric results to verify order
    let operations = vec![
        create_execution_operation("exec-1", OperationStatus::Started, None),
        create_completed_step("step-0", "0"),
        create_completed_step("step-1", "1"),
        create_completed_step("step-2", "2"),
        create_completed_step("step-3", "3"),
        create_completed_step("step-4", "4"),
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

    // Verify each step returns the correct result in order
    for i in 0..5 {
        let op_id = format!("step-{}", i);
        let checkpoint = state.get_checkpoint_result(&op_id).await;
        let result: Result<ReplayResult<i32>, DurableError> = check_replay(
            &checkpoint,
            OperationType::Step,
            &op_id,
            TEST_EXECUTION_ARN,
        );
        assert!(result.is_ok());
        match result.unwrap() {
            ReplayResult::Replayed(value) => assert_eq!(value, i as i32, "Step {} should return {}", i, i),
            _ => panic!("Expected Replayed result for step-{}", i),
        }
    }
}

// =============================================================================
// Test 12.2: Replaying workflow with pending operation (suspension)
// =============================================================================

/// Test that replaying a workflow with a pending operation returns InProgress
/// to signal suspension.
///
/// **Requirements:** 6.2 - WHEN replaying a workflow with a pending operation,
/// THE Test_Suite SHALL verify execution suspends at the pending operation
#[tokio::test]
async fn test_replay_pending_step_returns_in_progress() {
    // Create operations with a pending step (waiting for retry)
    let operations = vec![
        create_execution_operation("exec-1", OperationStatus::Started, None),
        create_completed_step("step-1", "\"done\""),
        create_pending_step("step-2", 1, 9999999999000), // Pending with future timestamp
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

    // First step should return cached result
    let checkpoint1 = state.get_checkpoint_result("step-1").await;
    let result1: Result<ReplayResult<String>, DurableError> = check_replay(
        &checkpoint1,
        OperationType::Step,
        "step-1",
        TEST_EXECUTION_ARN,
    );
    assert!(result1.is_ok());
    assert!(matches!(result1.unwrap(), ReplayResult::Replayed(_)));

    // Pending step should return InProgress (not NotFound, not Replayed)
    let checkpoint2 = state.get_checkpoint_result("step-2").await;
    assert!(checkpoint2.is_existent(), "Pending operation should exist");
    assert!(checkpoint2.is_pending(), "Operation should be in pending status");
    
    // The check_replay function returns InProgress for Started status
    // For Pending status, we need to check the status directly
    let status = checkpoint2.status();
    assert_eq!(status, Some(OperationStatus::Pending), "Status should be Pending");
}

/// Test that replaying a workflow with a pending WAIT operation signals suspension.
#[tokio::test]
async fn test_replay_pending_wait_signals_suspension() {
    let operations = vec![
        create_execution_operation("exec-1", OperationStatus::Started, None),
        create_completed_step("step-1", "\"done\""),
        create_pending_wait("wait-1", 9999999999000), // Wait scheduled for future
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

    // Wait operation should be in Started status (not completed)
    let checkpoint = state.get_checkpoint_result("wait-1").await;
    assert!(checkpoint.is_existent(), "Wait operation should exist");
    
    // Check that it's not terminal (still in progress)
    assert!(!checkpoint.is_terminal(), "Pending wait should not be terminal");
    assert!(!checkpoint.is_succeeded(), "Pending wait should not be succeeded");
}

/// Test that replaying a workflow with a pending CALLBACK operation signals suspension.
#[tokio::test]
async fn test_replay_pending_callback_signals_suspension() {
    let operations = vec![
        create_execution_operation("exec-1", OperationStatus::Started, None),
        create_pending_callback("callback-1", "cb-id-123"),
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

    // Callback operation should be in Started status (waiting for external signal)
    let checkpoint = state.get_checkpoint_result("callback-1").await;
    assert!(checkpoint.is_existent(), "Callback operation should exist");
    assert!(!checkpoint.is_terminal(), "Pending callback should not be terminal");
    
    // Verify callback_id is available
    let op = checkpoint.operation().unwrap();
    assert!(op.callback_details.is_some());
    assert_eq!(
        op.callback_details.as_ref().unwrap().callback_id,
        Some("cb-id-123".to_string())
    );
}

// =============================================================================
// Test 12.3: Replaying workflow with mixed operation types
// =============================================================================

/// Test that replaying a workflow with mixed operation types handles each correctly.
///
/// **Requirements:** 6.3 - WHEN replaying a workflow with mixed operation types,
/// THE Test_Suite SHALL verify each operation type is handled correctly
#[tokio::test]
async fn test_replay_mixed_operation_types() {
    let operations = vec![
        create_execution_operation("exec-1", OperationStatus::Started, Some("{\"order_id\":\"123\"}")),
        create_completed_step("step-1", "\"validated\""),
        create_completed_wait("wait-1"),
        create_completed_callback("callback-1", "cb-123", "\"approved\""),
        create_completed_invoke("invoke-1", "\"processed\""),
        create_completed_step("step-2", "\"finalized\""),
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

    // Verify STEP operations return correct results
    let step1_checkpoint = state.get_checkpoint_result("step-1").await;
    let step1_result: Result<ReplayResult<String>, DurableError> = check_replay(
        &step1_checkpoint,
        OperationType::Step,
        "step-1",
        TEST_EXECUTION_ARN,
    );
    assert!(matches!(step1_result, Ok(ReplayResult::Replayed(ref v)) if v == "validated"));

    // Verify WAIT operation is completed
    let wait_checkpoint = state.get_checkpoint_result("wait-1").await;
    assert!(wait_checkpoint.is_succeeded(), "Wait should be succeeded");
    assert_eq!(wait_checkpoint.operation_type(), Some(OperationType::Wait));

    // Verify CALLBACK operation returns correct result
    let callback_checkpoint = state.get_checkpoint_result("callback-1").await;
    assert!(callback_checkpoint.is_succeeded(), "Callback should be succeeded");
    assert_eq!(callback_checkpoint.operation_type(), Some(OperationType::Callback));
    let callback_op = callback_checkpoint.operation().unwrap();
    assert_eq!(
        callback_op.callback_details.as_ref().unwrap().result,
        Some("\"approved\"".to_string())
    );

    // Verify INVOKE operation returns correct result
    let invoke_checkpoint = state.get_checkpoint_result("invoke-1").await;
    let invoke_result: Result<ReplayResult<String>, DurableError> = check_replay(
        &invoke_checkpoint,
        OperationType::Invoke,
        "invoke-1",
        TEST_EXECUTION_ARN,
    );
    assert!(matches!(invoke_result, Ok(ReplayResult::Replayed(ref v)) if v == "processed"));

    // Verify second STEP operation
    let step2_checkpoint = state.get_checkpoint_result("step-2").await;
    let step2_result: Result<ReplayResult<String>, DurableError> = check_replay(
        &step2_checkpoint,
        OperationType::Step,
        "step-2",
        TEST_EXECUTION_ARN,
    );
    assert!(matches!(step2_result, Ok(ReplayResult::Replayed(ref v)) if v == "finalized"));

    // Verify no checkpoint calls were made
    assert!(client.get_checkpoint_calls().is_empty());
}

/// Test that each operation type is correctly identified during replay.
#[tokio::test]
async fn test_replay_identifies_operation_types_correctly() {
    let operations = vec![
        create_execution_operation("exec-1", OperationStatus::Started, None),
        create_completed_step("op-step", "1"),
        create_completed_wait("op-wait"),
        create_completed_callback("op-callback", "cb-1", "\"ok\""),
        create_completed_invoke("op-invoke", "\"done\""),
        create_context_operation("op-context", OperationStatus::Succeeded, None),
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

    // Verify each operation type
    let step_cp = state.get_checkpoint_result("op-step").await;
    assert_eq!(step_cp.operation_type(), Some(OperationType::Step));

    let wait_cp = state.get_checkpoint_result("op-wait").await;
    assert_eq!(wait_cp.operation_type(), Some(OperationType::Wait));

    let callback_cp = state.get_checkpoint_result("op-callback").await;
    assert_eq!(callback_cp.operation_type(), Some(OperationType::Callback));

    let invoke_cp = state.get_checkpoint_result("op-invoke").await;
    assert_eq!(invoke_cp.operation_type(), Some(OperationType::Invoke));

    let context_cp = state.get_checkpoint_result("op-context").await;
    assert_eq!(context_cp.operation_type(), Some(OperationType::Context));
}


// =============================================================================
// Test 12.4: Non-deterministic change detection
// =============================================================================

/// Test that replaying with a different operation type raises NonDeterministic error.
///
/// **Requirements:** 6.4 - WHEN a replay encounters a non-deterministic change,
/// THE Test_Suite SHALL verify NonDeterministic error is raised
#[tokio::test]
async fn test_replay_detects_operation_type_mismatch() {
    // Create a workflow with a STEP operation
    let operations = vec![
        create_execution_operation("exec-1", OperationStatus::Started, None),
        create_completed_step("op-1", "\"result\""),
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

    // Try to replay as WAIT instead of STEP - should detect non-determinism
    let checkpoint = state.get_checkpoint_result("op-1").await;
    let result: Result<ReplayResult<String>, DurableError> = check_replay(
        &checkpoint,
        OperationType::Wait, // Wrong type! Checkpointed as STEP
        "op-1",
        TEST_EXECUTION_ARN,
    );

    assert!(result.is_err(), "Should return error for type mismatch");
    match result.unwrap_err() {
        DurableError::NonDeterministic { operation_id, message } => {
            assert_eq!(operation_id, Some("op-1".to_string()));
            assert!(message.contains("Wait"), "Message should mention expected type");
            assert!(message.contains("Step"), "Message should mention found type");
        }
        other => panic!("Expected NonDeterministic error, got {:?}", other),
    }
}

/// Test that all operation type mismatches are detected.
#[tokio::test]
async fn test_replay_detects_all_type_mismatches() {
    let test_cases = vec![
        (OperationType::Step, OperationType::Wait),
        (OperationType::Step, OperationType::Callback),
        (OperationType::Step, OperationType::Invoke),
        (OperationType::Wait, OperationType::Step),
        (OperationType::Wait, OperationType::Callback),
        (OperationType::Callback, OperationType::Step),
        (OperationType::Callback, OperationType::Wait),
        (OperationType::Invoke, OperationType::Step),
        (OperationType::Invoke, OperationType::Callback),
    ];

    for (checkpointed_type, expected_type) in test_cases {
        // Create operation with checkpointed_type
        let mut op = Operation::new("test-op", checkpointed_type);
        op.status = OperationStatus::Succeeded;
        op.result = Some("\"test\"".to_string());

        let operations = vec![
            create_execution_operation("exec-1", OperationStatus::Started, None),
            op,
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

        // Try to replay with expected_type (different from checkpointed)
        let checkpoint = state.get_checkpoint_result("test-op").await;
        let result: Result<ReplayResult<String>, DurableError> = check_replay(
            &checkpoint,
            expected_type,
            "test-op",
            TEST_EXECUTION_ARN,
        );

        assert!(
            result.is_err(),
            "Should detect mismatch: checkpointed {:?}, expected {:?}",
            checkpointed_type,
            expected_type
        );
        assert!(
            matches!(result.unwrap_err(), DurableError::NonDeterministic { .. }),
            "Should be NonDeterministic error for {:?} vs {:?}",
            checkpointed_type,
            expected_type
        );
    }
}

/// Test that matching operation types do not raise errors.
#[tokio::test]
async fn test_replay_matching_types_succeed() {
    let operation_types = vec![
        OperationType::Step,
        OperationType::Wait,
        OperationType::Callback,
        OperationType::Invoke,
        OperationType::Context,
    ];

    for op_type in operation_types {
        let mut op = Operation::new("test-op", op_type);
        op.status = OperationStatus::Succeeded;
        op.result = Some("\"test\"".to_string());

        let operations = vec![
            create_execution_operation("exec-1", OperationStatus::Started, None),
            op,
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

        // Replay with matching type should succeed
        let checkpoint = state.get_checkpoint_result("test-op").await;
        let result: Result<ReplayResult<String>, DurableError> = check_replay(
            &checkpoint,
            op_type, // Same as checkpointed
            "test-op",
            TEST_EXECUTION_ARN,
        );

        assert!(
            result.is_ok(),
            "Matching type {:?} should not raise error",
            op_type
        );
        assert!(
            !matches!(result.as_ref().unwrap(), ReplayResult::NotFound),
            "Should find the operation for type {:?}",
            op_type
        );
    }
}

/// Test that non-determinism is detected even for failed operations.
#[tokio::test]
async fn test_replay_detects_type_mismatch_for_failed_operations() {
    // Create a failed STEP operation
    let operations = vec![
        create_execution_operation("exec-1", OperationStatus::Started, None),
        create_failed_step("op-1", "TestError", "Something failed"),
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

    // Try to replay as WAIT - should detect non-determinism BEFORE returning the error
    let checkpoint = state.get_checkpoint_result("op-1").await;
    let result: Result<ReplayResult<String>, DurableError> = check_replay(
        &checkpoint,
        OperationType::Wait, // Wrong type!
        "op-1",
        TEST_EXECUTION_ARN,
    );

    // Should be NonDeterministic, not UserCode error
    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), DurableError::NonDeterministic { .. }),
        "Type mismatch should be detected before returning stored error"
    );
}

// =============================================================================
// Test 12.5: Replaying workflow with nested contexts
// =============================================================================

/// Test that replaying preserves parent-child relationships in nested contexts.
///
/// **Requirements:** 6.5 - WHEN replaying a workflow with nested contexts,
/// THE Test_Suite SHALL verify parent-child relationships are preserved
#[tokio::test]
async fn test_replay_preserves_parent_child_relationships() {
    // Create a workflow with nested context structure:
    // exec-1 (root)
    //   └── context-1 (child of exec-1)
    //         ├── step-1 (child of context-1)
    //         └── step-2 (child of context-1)
    let mut context_op = create_context_operation("context-1", OperationStatus::Succeeded, None);
    context_op.parent_id = Some("exec-1".to_string());

    let mut step1 = create_completed_step("step-1", "\"result-1\"");
    step1.parent_id = Some("context-1".to_string());

    let mut step2 = create_completed_step("step-2", "\"result-2\"");
    step2.parent_id = Some("context-1".to_string());

    let operations = vec![
        create_execution_operation("exec-1", OperationStatus::Started, None),
        context_op,
        step1,
        step2,
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

    // Verify parent-child relationships are preserved
    let context_cp = state.get_checkpoint_result("context-1").await;
    let context_op = context_cp.operation().unwrap();
    assert_eq!(context_op.parent_id, Some("exec-1".to_string()));

    let step1_cp = state.get_checkpoint_result("step-1").await;
    let step1_op = step1_cp.operation().unwrap();
    assert_eq!(step1_op.parent_id, Some("context-1".to_string()));

    let step2_cp = state.get_checkpoint_result("step-2").await;
    let step2_op = step2_cp.operation().unwrap();
    assert_eq!(step2_op.parent_id, Some("context-1".to_string()));
}

/// Test that deeply nested contexts preserve all parent relationships.
#[tokio::test]
async fn test_replay_preserves_deep_nesting() {
    // Create a deeply nested structure:
    // exec-1
    //   └── context-1
    //         └── context-2
    //               └── context-3
    //                     └── step-1
    let mut ctx1 = create_context_operation("context-1", OperationStatus::Succeeded, None);
    ctx1.parent_id = Some("exec-1".to_string());

    let mut ctx2 = create_context_operation("context-2", OperationStatus::Succeeded, None);
    ctx2.parent_id = Some("context-1".to_string());

    let mut ctx3 = create_context_operation("context-3", OperationStatus::Succeeded, None);
    ctx3.parent_id = Some("context-2".to_string());

    let mut step = create_completed_step("step-1", "\"deep-result\"");
    step.parent_id = Some("context-3".to_string());

    let operations = vec![
        create_execution_operation("exec-1", OperationStatus::Started, None),
        ctx1,
        ctx2,
        ctx3,
        step,
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

    // Verify the entire chain of parent relationships
    let ctx1_op = state.get_operation("context-1").await.unwrap();
    assert_eq!(ctx1_op.parent_id, Some("exec-1".to_string()));

    let ctx2_op = state.get_operation("context-2").await.unwrap();
    assert_eq!(ctx2_op.parent_id, Some("context-1".to_string()));

    let ctx3_op = state.get_operation("context-3").await.unwrap();
    assert_eq!(ctx3_op.parent_id, Some("context-2".to_string()));

    let step_op = state.get_operation("step-1").await.unwrap();
    assert_eq!(step_op.parent_id, Some("context-3".to_string()));

    // Verify the step result is still accessible
    let step_cp = state.get_checkpoint_result("step-1").await;
    let result: Result<ReplayResult<String>, DurableError> = check_replay(
        &step_cp,
        OperationType::Step,
        "step-1",
        TEST_EXECUTION_ARN,
    );
    assert!(matches!(result, Ok(ReplayResult::Replayed(ref v)) if v == "deep-result"));
}

/// Test that sibling contexts at the same level are handled correctly.
#[tokio::test]
async fn test_replay_handles_sibling_contexts() {
    // Create sibling contexts:
    // exec-1
    //   ├── context-a
    //   │     └── step-a
    //   └── context-b
    //         └── step-b
    let mut ctx_a = create_context_operation("context-a", OperationStatus::Succeeded, None);
    ctx_a.parent_id = Some("exec-1".to_string());

    let mut ctx_b = create_context_operation("context-b", OperationStatus::Succeeded, None);
    ctx_b.parent_id = Some("exec-1".to_string());

    let mut step_a = create_completed_step("step-a", "\"result-a\"");
    step_a.parent_id = Some("context-a".to_string());

    let mut step_b = create_completed_step("step-b", "\"result-b\"");
    step_b.parent_id = Some("context-b".to_string());

    let operations = vec![
        create_execution_operation("exec-1", OperationStatus::Started, None),
        ctx_a,
        ctx_b,
        step_a,
        step_b,
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

    // Verify both contexts have the same parent
    let ctx_a_op = state.get_operation("context-a").await.unwrap();
    let ctx_b_op = state.get_operation("context-b").await.unwrap();
    assert_eq!(ctx_a_op.parent_id, ctx_b_op.parent_id);
    assert_eq!(ctx_a_op.parent_id, Some("exec-1".to_string()));

    // Verify steps have correct parents
    let step_a_op = state.get_operation("step-a").await.unwrap();
    assert_eq!(step_a_op.parent_id, Some("context-a".to_string()));

    let step_b_op = state.get_operation("step-b").await.unwrap();
    assert_eq!(step_b_op.parent_id, Some("context-b".to_string()));

    // Verify both step results are accessible
    let step_a_cp = state.get_checkpoint_result("step-a").await;
    let result_a: Result<ReplayResult<String>, DurableError> = check_replay(
        &step_a_cp,
        OperationType::Step,
        "step-a",
        TEST_EXECUTION_ARN,
    );
    assert!(matches!(result_a, Ok(ReplayResult::Replayed(ref v)) if v == "result-a"));

    let step_b_cp = state.get_checkpoint_result("step-b").await;
    let result_b: Result<ReplayResult<String>, DurableError> = check_replay(
        &step_b_cp,
        OperationType::Step,
        "step-b",
        TEST_EXECUTION_ARN,
    );
    assert!(matches!(result_b, Ok(ReplayResult::Replayed(ref v)) if v == "result-b"));
}

/// Test that context names are preserved during replay.
#[tokio::test]
async fn test_replay_preserves_context_names() {
    let mut ctx = create_context_operation("context-1", OperationStatus::Succeeded, None);
    ctx.name = Some("my-named-context".to_string());
    ctx.parent_id = Some("exec-1".to_string());

    let operations = vec![
        create_execution_operation("exec-1", OperationStatus::Started, None),
        ctx,
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

    let ctx_op = state.get_operation("context-1").await.unwrap();
    assert_eq!(ctx_op.name, Some("my-named-context".to_string()));
}

// =============================================================================
// Additional Edge Case Tests
// =============================================================================

/// Test that replay mode transitions to new mode after all operations are replayed.
#[tokio::test]
async fn test_replay_mode_transitions_after_all_replayed() {
    let operations = vec![
        create_execution_operation("exec-1", OperationStatus::Started, None),
        create_completed_step("step-1", "\"done\""),
    ];

    let client = Arc::new(MockDurableServiceClient::new().with_checkpoint_responses(5));
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

    // Initially in replay mode
    assert!(state.is_replay(), "Should start in replay mode");

    // Track replay of all operations
    state.track_replay("exec-1").await;
    state.track_replay("step-1").await;

    // Should transition to new mode
    assert!(state.is_new(), "Should transition to new mode after all replayed");
}

/// Test that empty workflow starts in new mode.
#[tokio::test]
async fn test_empty_workflow_starts_in_new_mode() {
    let operations = vec![];

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

    assert!(state.is_new(), "Empty workflow should start in new mode");
}

/// Test that non-existent operation returns NotFound.
#[tokio::test]
async fn test_replay_non_existent_operation_returns_not_found() {
    let operations = vec![
        create_execution_operation("exec-1", OperationStatus::Started, None),
        create_completed_step("step-1", "\"done\""),
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

    // Query for non-existent operation
    let checkpoint = state.get_checkpoint_result("non-existent-op").await;
    assert!(!checkpoint.is_existent(), "Non-existent operation should not exist");

    let result: Result<ReplayResult<String>, DurableError> = check_replay(
        &checkpoint,
        OperationType::Step,
        "non-existent-op",
        TEST_EXECUTION_ARN,
    );
    assert!(matches!(result, Ok(ReplayResult::NotFound)));
}
