//! Tests for the common test utilities module.
//!
//! This file verifies that the test infrastructure works correctly.

mod common;

use aws_durable_execution_sdk::client::DurableServiceClient;
use aws_durable_execution_sdk::operation::{OperationStatus, OperationType, OperationUpdate};
use common::*;
use proptest::prelude::*;
use std::sync::Arc;

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
    let op = create_execution_operation(
        "exec-1",
        OperationStatus::Started,
        Some("{\"key\":\"value\"}"),
    );
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

// =============================================================================
// Tracing Span Property Tests
// =============================================================================

use aws_durable_execution_sdk::context::{create_operation_span, OperationIdentifier};
use std::collections::HashMap;
use tracing::field::Visit;
use tracing::span::Attributes;
use tracing::Subscriber;
use tracing_subscriber::layer::SubscriberExt;

/// A test visitor that captures span fields for verification.
#[derive(Debug, Default)]
struct FieldCapture {
    fields: HashMap<String, String>,
}

impl Visit for FieldCapture {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.fields
            .insert(field.name().to_string(), format!("{:?}", value));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }
}

/// A test layer that captures span attributes for verification.
struct TestLayer {
    captured_fields: Arc<std::sync::Mutex<HashMap<String, String>>>,
}

impl<S: Subscriber> tracing_subscriber::Layer<S> for TestLayer {
    fn on_new_span(
        &self,
        attrs: &Attributes<'_>,
        _id: &tracing::span::Id,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut capture = FieldCapture::default();
        attrs.record(&mut capture);
        let mut fields = self.captured_fields.lock().unwrap();
        fields.extend(capture.fields);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 7: Tracing Span Fields
    /// Validates: Requirements 3.2, 3.3, 3.4, 3.5
    ///
    /// This test verifies that create_operation_span includes all required fields:
    /// - operation_id (Requirement 3.2)
    /// - operation_type (Requirement 3.3)
    /// - parent_id when available (Requirement 3.4)
    /// - durable_execution_arn (Requirement 3.5)
    #[test]
    fn test_tracing_span_includes_required_fields(
        operation_type in prop_oneof![
            Just("step"),
            Just("wait"),
            Just("callback"),
            Just("invoke"),
            Just("map"),
            Just("parallel"),
        ],
        operation_id in operation_id_strategy(),
        parent_id in optional_string_strategy(),
        name in optional_string_strategy(),
        durable_execution_arn in execution_arn_strategy(),
    ) {
        // Create the operation identifier
        let op_id = OperationIdentifier::new(
            operation_id.clone(),
            parent_id.clone(),
            name.clone(),
        );

        // Set up a test subscriber to capture span fields
        let captured_fields = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let test_layer = TestLayer {
            captured_fields: captured_fields.clone(),
        };
        let subscriber = tracing_subscriber::registry().with(test_layer);

        // Create the span within the test subscriber context
        tracing::subscriber::with_default(subscriber, || {
            let span = create_operation_span(operation_type, &op_id, &durable_execution_arn);
            let _guard = span.enter();
        });

        // Verify the captured fields
        let fields = captured_fields.lock().unwrap();

        // Requirement 3.2: operation_id must be present
        prop_assert!(
            fields.contains_key("operation_id"),
            "Span must include operation_id field (Requirement 3.2)"
        );
        prop_assert_eq!(
            fields.get("operation_id").unwrap(),
            &operation_id,
            "operation_id must match the provided value"
        );

        // Requirement 3.3: operation_type must be present
        prop_assert!(
            fields.contains_key("operation_type"),
            "Span must include operation_type field (Requirement 3.3)"
        );
        prop_assert_eq!(
            fields.get("operation_type").unwrap(),
            operation_type,
            "operation_type must match the provided value"
        );

        // Requirement 3.4: parent_id must be present when available
        prop_assert!(
            fields.contains_key("parent_id"),
            "Span must include parent_id field (Requirement 3.4)"
        );
        if let Some(ref pid) = parent_id {
            // When parent_id is Some, it should be recorded as Some("value")
            let expected = format!("Some(\"{}\")", pid);
            prop_assert_eq!(
                fields.get("parent_id").unwrap(),
                &expected,
                "parent_id must match the provided value when present"
            );
        } else {
            // When parent_id is None, it should be recorded as None
            prop_assert_eq!(
                fields.get("parent_id").unwrap(),
                "None",
                "parent_id must be None when not provided"
            );
        }

        // Requirement 3.5: durable_execution_arn must be present
        prop_assert!(
            fields.contains_key("durable_execution_arn"),
            "Span must include durable_execution_arn field (Requirement 3.5)"
        );
        prop_assert_eq!(
            fields.get("durable_execution_arn").unwrap(),
            &durable_execution_arn,
            "durable_execution_arn must match the provided value"
        );
    }
}

// =============================================================================
// TracingLogger Extra Field Passthrough Property Tests
// =============================================================================

use aws_durable_execution_sdk::context::{LogInfo, Logger, TracingLogger};
use tracing::Event;

/// A test layer that captures event fields for verification.
struct EventCaptureLayer {
    captured_fields: Arc<std::sync::Mutex<HashMap<String, String>>>,
}

impl<S: Subscriber> tracing_subscriber::Layer<S> for EventCaptureLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        let mut capture = FieldCapture::default();
        event.record(&mut capture);
        let mut fields = self.captured_fields.lock().unwrap();
        fields.extend(capture.fields);
    }
}

/// Strategy for generating extra field key-value pairs.
/// Keys are alphanumeric identifiers, values can be any non-empty string.
fn extra_field_strategy() -> impl Strategy<Value = (String, String)> {
    (
        "[a-zA-Z][a-zA-Z0-9_]{0,15}", // key: valid identifier
        "[a-zA-Z0-9_\\-\\.]{1,32}",   // value: simple string without special chars
    )
}

/// Strategy for generating a list of extra fields.
fn extra_fields_strategy() -> impl Strategy<Value = Vec<(String, String)>> {
    prop::collection::vec(extra_field_strategy(), 0..5)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 11: TracingLogger Extra Field Passthrough
    /// Validates: Requirements 5.1, 5.2
    ///
    /// This test verifies that when LogInfo contains extra fields, the TracingLogger
    /// includes them in the tracing output as key-value pairs.
    ///
    /// - Requirement 5.1: WHEN LogInfo contains extra fields, THE TracingLogger SHALL include them in the tracing output
    /// - Requirement 5.2: THE extra fields SHALL be formatted as key-value pairs in the tracing event
    #[test]
    fn test_tracing_logger_extra_field_passthrough(
        message in "[a-zA-Z0-9 ]{1,50}",
        durable_execution_arn in optional_string_strategy(),
        operation_id in optional_string_strategy(),
        parent_id in optional_string_strategy(),
        is_replay in proptest::bool::ANY,
        extra_fields in extra_fields_strategy(),
        log_level in prop_oneof![
            Just("debug"),
            Just("info"),
            Just("warn"),
            Just("error"),
        ],
    ) {
        // Build LogInfo with extra fields
        let mut log_info = LogInfo::default();
        log_info.is_replay = is_replay;

        if let Some(ref arn) = durable_execution_arn {
            log_info.durable_execution_arn = Some(arn.clone());
        }
        if let Some(ref op_id) = operation_id {
            log_info.operation_id = Some(op_id.clone());
        }
        if let Some(ref pid) = parent_id {
            log_info.parent_id = Some(pid.clone());
        }

        // Add extra fields
        for (key, value) in &extra_fields {
            log_info.extra.push((key.clone(), value.clone()));
        }

        // Set up a test subscriber to capture event fields
        let captured_fields = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let test_layer = EventCaptureLayer {
            captured_fields: captured_fields.clone(),
        };
        let subscriber = tracing_subscriber::registry().with(test_layer);

        // Create the logger and log a message
        let logger = TracingLogger;

        tracing::subscriber::with_default(subscriber, || {
            match log_level {
                "debug" => logger.debug(&message, &log_info),
                "info" => logger.info(&message, &log_info),
                "warn" => logger.warn(&message, &log_info),
                "error" => logger.error(&message, &log_info),
                _ => unreachable!(),
            }
        });

        // Verify the captured fields
        let fields = captured_fields.lock().unwrap();

        // Requirement 5.1 & 5.2: Extra fields must be present in the output
        // The extra field is formatted as "key1=value1, key2=value2, ..."
        prop_assert!(
            fields.contains_key("extra"),
            "TracingLogger must include 'extra' field in output (Requirement 5.1)"
        );

        let extra_output = fields.get("extra").unwrap();

        // Verify each extra field is present in the formatted output
        for (key, value) in &extra_fields {
            let expected_pair = format!("{}={}", key, value);
            prop_assert!(
                extra_output.contains(&expected_pair),
                "Extra field '{}' must be present in output as key=value pair (Requirement 5.2). Got: {}",
                expected_pair,
                extra_output
            );
        }

        // If no extra fields, the output should be empty
        if extra_fields.is_empty() {
            prop_assert!(
                extra_output.is_empty(),
                "Extra field output should be empty when no extra fields provided. Got: {}",
                extra_output
            );
        }
    }
}
