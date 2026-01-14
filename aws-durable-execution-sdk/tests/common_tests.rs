//! Tests for the common test utilities module.
//!
//! This file verifies that the test infrastructure works correctly.

mod common;

use common::*;
use aws_durable_execution_sdk::client::DurableServiceClient;
use aws_durable_execution_sdk::operation::{OperationStatus, OperationType, OperationUpdate};
use proptest::prelude::*;

// =============================================================================
// Mock Client Tests
// =============================================================================

#[tokio::test]
async fn test_mock_client_default_response() {
    let client = MockDurableServiceClient::new();
    let result = client
        .checkpoint(TEST_EXECUTION_ARN, TEST_CHECKPOINT_TOKEN, vec![])
        .await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.checkpoint_token, "mock-token");
}

#[tokio::test]
async fn test_mock_client_custom_response() {
    let client = MockDurableServiceClient::new()
        .with_checkpoint_response(Ok(create_checkpoint_response("custom-token")));

    let result = client
        .checkpoint(TEST_EXECUTION_ARN, TEST_CHECKPOINT_TOKEN, vec![])
        .await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.checkpoint_token, "custom-token");
}

#[tokio::test]
async fn test_mock_client_multiple_responses() {
    let client = MockDurableServiceClient::new()
        .with_checkpoint_response(Ok(create_checkpoint_response("token-1")))
        .with_checkpoint_response(Ok(create_checkpoint_response("token-2")))
        .with_checkpoint_response(Ok(create_checkpoint_response("token-3")));

    // First call
    let result1 = client
        .checkpoint(TEST_EXECUTION_ARN, "t1", vec![])
        .await
        .unwrap();
    assert_eq!(result1.checkpoint_token, "token-1");

    // Second call
    let result2 = client
        .checkpoint(TEST_EXECUTION_ARN, "t2", vec![])
        .await
        .unwrap();
    assert_eq!(result2.checkpoint_token, "token-2");

    // Third call
    let result3 = client
        .checkpoint(TEST_EXECUTION_ARN, "t3", vec![])
        .await
        .unwrap();
    assert_eq!(result3.checkpoint_token, "token-3");
}

#[tokio::test]
async fn test_mock_client_records_calls() {
    let client = MockDurableServiceClient::new();

    let updates = vec![OperationUpdate::start("op-1", OperationType::Step)];
    let _ = client
        .checkpoint(TEST_EXECUTION_ARN, TEST_CHECKPOINT_TOKEN, updates)
        .await;

    let calls = client.get_checkpoint_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].durable_execution_arn, TEST_EXECUTION_ARN);
    assert_eq!(calls[0].checkpoint_token, TEST_CHECKPOINT_TOKEN);
    assert_eq!(calls[0].operations.len(), 1);
}

#[tokio::test]
async fn test_mock_client_with_operations_response() {
    let operations = vec![
        create_completed_step("step-1", "\"result-1\""),
        create_completed_step("step-2", "\"result-2\""),
    ];

    let client = MockDurableServiceClient::new()
        .with_checkpoint_response_with_operations("token-with-ops", operations);

    let result = client
        .checkpoint(TEST_EXECUTION_ARN, TEST_CHECKPOINT_TOKEN, vec![])
        .await
        .unwrap();

    assert_eq!(result.checkpoint_token, "token-with-ops");
    assert!(result.new_execution_state.is_some());
    let state = result.new_execution_state.unwrap();
    assert_eq!(state.operations.len(), 2);
}

// =============================================================================
// Helper Function Tests
// =============================================================================

#[test]
fn test_create_operation() {
    let op = create_operation("test-op", OperationType::Step, OperationStatus::Succeeded);
    assert_eq!(op.operation_id, "test-op");
    assert_eq!(op.operation_type, OperationType::Step);
    assert_eq!(op.status, OperationStatus::Succeeded);
}

#[test]
fn test_create_completed_step() {
    let op = create_completed_step("step-1", "\"test-result\"");
    assert_eq!(op.operation_id, "step-1");
    assert_eq!(op.operation_type, OperationType::Step);
    assert_eq!(op.status, OperationStatus::Succeeded);
    assert!(op.step_details.is_some());
    assert_eq!(
        op.step_details.as_ref().unwrap().result,
        Some("\"test-result\"".to_string())
    );
}

#[test]
fn test_create_pending_step() {
    let op = create_pending_step("step-2", 1, 1234567890000);
    assert_eq!(op.operation_id, "step-2");
    assert_eq!(op.status, OperationStatus::Pending);
    assert!(op.step_details.is_some());
    let details = op.step_details.as_ref().unwrap();
    assert_eq!(details.attempt, Some(1));
    assert_eq!(details.next_attempt_timestamp, Some(1234567890000));
}

#[test]
fn test_create_failed_step() {
    let op = create_failed_step("step-3", "TestError", "Something went wrong");
    assert_eq!(op.operation_id, "step-3");
    assert_eq!(op.status, OperationStatus::Failed);
    assert!(op.step_details.is_some());
    let error = op.step_details.as_ref().unwrap().error.as_ref().unwrap();
    assert_eq!(error.error_type, "TestError");
    assert_eq!(error.error_message, "Something went wrong");
}

#[test]
fn test_create_completed_wait() {
    let op = create_completed_wait("wait-1");
    assert_eq!(op.operation_id, "wait-1");
    assert_eq!(op.operation_type, OperationType::Wait);
    assert_eq!(op.status, OperationStatus::Succeeded);
}

#[test]
fn test_create_pending_wait() {
    let op = create_pending_wait("wait-2", 9999999999000);
    assert_eq!(op.operation_id, "wait-2");
    assert_eq!(op.status, OperationStatus::Started);
    assert!(op.wait_details.is_some());
    assert_eq!(
        op.wait_details.as_ref().unwrap().scheduled_end_timestamp,
        Some(9999999999000)
    );
}

#[test]
fn test_create_completed_callback() {
    let op = create_completed_callback("cb-1", "callback-id-123", "\"callback-result\"");
    assert_eq!(op.operation_id, "cb-1");
    assert_eq!(op.operation_type, OperationType::Callback);
    assert_eq!(op.status, OperationStatus::Succeeded);
    assert!(op.callback_details.is_some());
    let details = op.callback_details.as_ref().unwrap();
    assert_eq!(details.callback_id, Some("callback-id-123".to_string()));
    assert_eq!(details.result, Some("\"callback-result\"".to_string()));
}

#[test]
fn test_create_timed_out_callback() {
    let op = create_timed_out_callback("cb-2", "callback-id-456");
    assert_eq!(op.operation_id, "cb-2");
    assert_eq!(op.status, OperationStatus::TimedOut);
    assert!(op.callback_details.is_some());
    let details = op.callback_details.as_ref().unwrap();
    assert!(details.error.is_some());
}

#[test]
fn test_create_context_operation() {
    let op = create_context_operation("ctx-1", OperationStatus::Started, Some("parent-op"));
    assert_eq!(op.operation_id, "ctx-1");
    assert_eq!(op.operation_type, OperationType::Context);
    assert_eq!(op.status, OperationStatus::Started);
    assert_eq!(op.parent_id, Some("parent-op".to_string()));
}

#[test]
fn test_create_execution_operation() {
    let op = create_execution_operation("exec-1", OperationStatus::Started, Some("{\"key\":\"value\"}"));
    assert_eq!(op.operation_id, "exec-1");
    assert_eq!(op.operation_type, OperationType::Execution);
    assert_eq!(op.status, OperationStatus::Started);
    assert!(op.execution_details.is_some());
    assert_eq!(
        op.execution_details.as_ref().unwrap().input_payload,
        Some("{\"key\":\"value\"}".to_string())
    );
}

// =============================================================================
// Proptest Strategy Tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    // Feature: rust-sdk-test-suite, Property 1: OperationType serialization round-trip
    #[test]
    fn test_operation_type_strategy_generates_valid_types(op_type in operation_type_strategy()) {
        // All generated types should be valid enum variants
        let json = serde_json::to_string(&op_type).unwrap();
        let deserialized: OperationType = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(op_type, deserialized);
    }

    // Feature: rust-sdk-test-suite, Property 2: OperationStatus serialization round-trip
    #[test]
    fn test_operation_status_strategy_generates_valid_statuses(status in operation_status_strategy()) {
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: OperationStatus = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(status, deserialized);
    }

    // Feature: rust-sdk-test-suite, Property 4: Terminal status classification
    #[test]
    fn test_terminal_status_strategy_generates_terminal_statuses(status in terminal_status_strategy()) {
        prop_assert!(status.is_terminal());
    }

    #[test]
    fn test_non_terminal_status_strategy_generates_non_terminal_statuses(status in non_terminal_status_strategy()) {
        prop_assert!(!status.is_terminal());
    }

    #[test]
    fn test_non_empty_string_strategy_generates_non_empty_strings(s in non_empty_string_strategy()) {
        prop_assert!(!s.is_empty());
        prop_assert!(s.len() <= 64);
    }

    #[test]
    fn test_execution_arn_strategy_generates_valid_arns(arn in execution_arn_strategy()) {
        prop_assert!(arn.starts_with("arn:"));
        prop_assert!(arn.contains(":lambda:"));
        prop_assert!(arn.contains(":durable:"));
    }

    // Feature: rust-sdk-test-suite, Property 5: Operation serialization round-trip
    #[test]
    fn test_operation_strategy_generates_serializable_operations(op in operation_strategy()) {
        let json = serde_json::to_string(&op).unwrap();
        let deserialized: aws_durable_execution_sdk::operation::Operation = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(op.operation_id, deserialized.operation_id);
        prop_assert_eq!(op.operation_type, deserialized.operation_type);
        prop_assert_eq!(op.status, deserialized.status);
    }

    #[test]
    fn test_operation_update_strategy_generates_valid_updates(update in operation_update_strategy()) {
        // All generated updates should be serializable
        let json = serde_json::to_string(&update).unwrap();
        prop_assert!(!json.is_empty());
        // Should contain required fields
        prop_assert!(json.contains("\"Id\""));
        prop_assert!(json.contains("\"Action\""));
        prop_assert!(json.contains("\"Type\""));
    }

    #[test]
    fn test_operation_update_batch_strategy_generates_valid_batches(batch in operation_update_batch_strategy(10)) {
        prop_assert!(batch.len() <= 10);
        for update in &batch {
            let json = serde_json::to_string(update).unwrap();
            prop_assert!(!json.is_empty());
        }
    }
}
