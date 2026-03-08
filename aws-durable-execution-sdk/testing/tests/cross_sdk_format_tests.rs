//! Integration tests verifying cross-SDK history format compatibility.
//!
//! These tests ensure that the Rust SDK generates history events in a format
//! that is compatible with the Node.js SDK, enabling cross-SDK history comparison.
//!
//! # Requirements Validated
//!
//! - 5.1: THE Event_History output SHALL be a JSON array at the root level
//! - 5.2: THE Event_History output SHALL NOT use an "events" wrapper key
//! - 5.4: WHEN serializing to JSON, THE Event_Processor SHALL use PascalCase for all field names
//! - 5.5: WHEN serializing to JSON, THE Event_Processor SHALL format timestamps as ISO 8601 strings
//! - 6.3: THE generated history file SHALL be parseable by the Node.js SDK's history comparison utilities

use durable_execution_sdk_testing::checkpoint_server::{
    ExecutionStartedDetails, ExecutionStartedDetailsWrapper, ExecutionSucceededDetails,
    ExecutionSucceededDetailsWrapper, NodeJsEventDetails, NodeJsEventType, NodeJsHistoryEvent,
    PayloadWrapper, RetryDetails, StepStartedDetails, StepStartedDetailsWrapper,
    StepSucceededDetails, StepSucceededDetailsWrapper,
};
use regex::Regex;
use serde_json::Value;

/// Test that the history output is a flat JSON array (not wrapped in an object).
///
/// **Validates: Requirements 5.1, 5.2**
#[test]
fn test_history_output_is_flat_json_array() {
    let events: Vec<NodeJsHistoryEvent> = vec![
        NodeJsHistoryEvent {
            event_type: NodeJsEventType::ExecutionStarted,
            event_id: 1,
            id: Some("exec-123".to_string()),
            event_timestamp: "2025-12-03T22:58:35.094Z".to_string(),
            sub_type: None,
            name: None,
            parent_id: None,
            details: NodeJsEventDetails::ExecutionStarted(ExecutionStartedDetailsWrapper {
                execution_started_details: ExecutionStartedDetails {
                    input: PayloadWrapper::new("{}"),
                    execution_timeout: None,
                },
            }),
        },
        NodeJsHistoryEvent {
            event_type: NodeJsEventType::ExecutionSucceeded,
            event_id: 2,
            id: Some("exec-123".to_string()),
            event_timestamp: "2025-12-03T22:58:35.100Z".to_string(),
            sub_type: None,
            name: None,
            parent_id: None,
            details: NodeJsEventDetails::ExecutionSucceeded(ExecutionSucceededDetailsWrapper {
                execution_succeeded_details: ExecutionSucceededDetails {
                    result: PayloadWrapper::new(r#""Hello World!""#),
                },
            }),
        },
    ];

    let json = serde_json::to_string_pretty(&events).unwrap();

    // Requirement 5.1: Output SHALL be a JSON array at the root level
    assert!(
        json.trim().starts_with('['),
        "JSON should start with '[' for flat array"
    );
    assert!(
        json.trim().ends_with(']'),
        "JSON should end with ']' for flat array"
    );

    // Requirement 5.2: Output SHALL NOT use an "events" wrapper key
    assert!(
        !json.contains(r#""events""#),
        "JSON should not have 'events' wrapper key"
    );

    // Verify it parses as a JSON array
    let parsed: Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.is_array(), "Parsed JSON should be an array");
    assert_eq!(parsed.as_array().unwrap().len(), 2);
}

/// Test that all field names use PascalCase.
///
/// **Validates: Requirement 5.4**
#[test]
fn test_all_field_names_are_pascal_case() {
    let event = NodeJsHistoryEvent {
        event_type: NodeJsEventType::StepSucceeded,
        event_id: 1,
        id: Some("step-123".to_string()),
        event_timestamp: "2025-12-03T22:58:35.094Z".to_string(),
        sub_type: Some("Step".to_string()),
        name: Some("process-data".to_string()),
        parent_id: Some("exec-123".to_string()),
        details: NodeJsEventDetails::StepSucceeded(StepSucceededDetailsWrapper {
            step_succeeded_details: StepSucceededDetails {
                result: PayloadWrapper::new(r#"{"value": 42}"#),
                retry_details: RetryDetails::new(1),
            },
        }),
    };

    let json = serde_json::to_string(&event).unwrap();
    let parsed: Value = serde_json::from_str(&json).unwrap();

    // Check all keys are PascalCase
    fn check_pascal_case(value: &Value, path: &str) {
        let pascal_case_pattern = Regex::new(r"^[A-Z][a-zA-Z0-9]*$").unwrap();

        match value {
            Value::Object(map) => {
                for (key, val) in map {
                    assert!(
                        pascal_case_pattern.is_match(key),
                        "Key '{}' at path '{}' is not PascalCase",
                        key,
                        path
                    );
                    check_pascal_case(val, &format!("{}.{}", path, key));
                }
            }
            Value::Array(arr) => {
                for (i, val) in arr.iter().enumerate() {
                    check_pascal_case(val, &format!("{}[{}]", path, i));
                }
            }
            _ => {}
        }
    }

    check_pascal_case(&parsed, "root");
}

/// Test that timestamps are in ISO 8601 format.
///
/// **Validates: Requirement 5.5**
#[test]
fn test_timestamps_are_iso_8601_format() {
    let event = NodeJsHistoryEvent {
        event_type: NodeJsEventType::ExecutionStarted,
        event_id: 1,
        id: Some("exec-123".to_string()),
        event_timestamp: "2025-12-03T22:58:35.094Z".to_string(),
        sub_type: None,
        name: None,
        parent_id: None,
        details: NodeJsEventDetails::ExecutionStarted(ExecutionStartedDetailsWrapper {
            execution_started_details: ExecutionStartedDetails {
                input: PayloadWrapper::new("{}"),
                execution_timeout: None,
            },
        }),
    };

    let json = serde_json::to_string(&event).unwrap();
    let parsed: Value = serde_json::from_str(&json).unwrap();

    // ISO 8601 format: YYYY-MM-DDTHH:MM:SS.sssZ
    let iso_8601_pattern = Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$").unwrap();

    let timestamp = parsed["EventTimestamp"].as_str().unwrap();
    assert!(
        iso_8601_pattern.is_match(timestamp),
        "Timestamp '{}' does not match ISO 8601 format",
        timestamp
    );
}

/// Test that the format matches Node.js SDK expected structure.
///
/// This test verifies that a complete workflow history can be serialized
/// and deserialized in a format compatible with the Node.js SDK.
///
/// **Validates: Requirement 6.3**
#[test]
fn test_nodejs_sdk_compatible_format() {
    // Create a typical workflow history
    let events: Vec<NodeJsHistoryEvent> = vec![
        NodeJsHistoryEvent {
            event_type: NodeJsEventType::ExecutionStarted,
            event_id: 1,
            id: Some("exec-abc123".to_string()),
            event_timestamp: "2025-12-03T22:58:35.094Z".to_string(),
            sub_type: None,
            name: None,
            parent_id: None,
            details: NodeJsEventDetails::ExecutionStarted(ExecutionStartedDetailsWrapper {
                execution_started_details: ExecutionStartedDetails {
                    input: PayloadWrapper::new(r#"{"orderId": "12345"}"#),
                    execution_timeout: None,
                },
            }),
        },
        NodeJsHistoryEvent {
            event_type: NodeJsEventType::StepStarted,
            event_id: 2,
            id: Some("step-def456".to_string()),
            event_timestamp: "2025-12-03T22:58:35.096Z".to_string(),
            sub_type: Some("Step".to_string()),
            name: Some("validate-order".to_string()),
            parent_id: Some("exec-abc123".to_string()),
            details: NodeJsEventDetails::StepStarted(StepStartedDetailsWrapper {
                step_started_details: StepStartedDetails {},
            }),
        },
        NodeJsHistoryEvent {
            event_type: NodeJsEventType::StepSucceeded,
            event_id: 3,
            id: Some("step-def456".to_string()),
            event_timestamp: "2025-12-03T22:58:35.100Z".to_string(),
            sub_type: Some("Step".to_string()),
            name: Some("validate-order".to_string()),
            parent_id: Some("exec-abc123".to_string()),
            details: NodeJsEventDetails::StepSucceeded(StepSucceededDetailsWrapper {
                step_succeeded_details: StepSucceededDetails {
                    result: PayloadWrapper::new(r#"{"valid": true}"#),
                    retry_details: RetryDetails::new(1),
                },
            }),
        },
        NodeJsHistoryEvent {
            event_type: NodeJsEventType::ExecutionSucceeded,
            event_id: 4,
            id: Some("exec-abc123".to_string()),
            event_timestamp: "2025-12-03T22:58:35.105Z".to_string(),
            sub_type: None,
            name: None,
            parent_id: None,
            details: NodeJsEventDetails::ExecutionSucceeded(ExecutionSucceededDetailsWrapper {
                execution_succeeded_details: ExecutionSucceededDetails {
                    result: PayloadWrapper::new(r#"{"status": "completed"}"#),
                },
            }),
        },
    ];

    // Serialize to JSON
    let json = serde_json::to_string_pretty(&events).unwrap();

    // Parse as generic JSON to verify structure
    let parsed: Value = serde_json::from_str(&json).unwrap();

    // Verify it's an array
    assert!(parsed.is_array());
    let arr = parsed.as_array().unwrap();
    assert_eq!(arr.len(), 4);

    // Verify each event has required fields
    for (i, event) in arr.iter().enumerate() {
        assert!(
            event.get("EventType").is_some(),
            "Event {} missing EventType",
            i
        );
        assert!(
            event.get("EventId").is_some(),
            "Event {} missing EventId",
            i
        );
        assert!(
            event.get("EventTimestamp").is_some(),
            "Event {} missing EventTimestamp",
            i
        );
    }

    // Verify event types are correct
    assert_eq!(arr[0]["EventType"], "ExecutionStarted");
    assert_eq!(arr[1]["EventType"], "StepStarted");
    assert_eq!(arr[2]["EventType"], "StepSucceeded");
    assert_eq!(arr[3]["EventType"], "ExecutionSucceeded");

    // Verify event IDs are sequential
    assert_eq!(arr[0]["EventId"], 1);
    assert_eq!(arr[1]["EventId"], 2);
    assert_eq!(arr[2]["EventId"], 3);
    assert_eq!(arr[3]["EventId"], 4);

    // Verify details fields are present with correct names
    assert!(arr[0].get("ExecutionStartedDetails").is_some());
    assert!(arr[1].get("StepStartedDetails").is_some());
    assert!(arr[2].get("StepSucceededDetails").is_some());
    assert!(arr[3].get("ExecutionSucceededDetails").is_some());

    // Verify SubType and Name are present where expected
    assert!(arr[1].get("SubType").is_some());
    assert_eq!(arr[1]["SubType"], "Step");
    assert!(arr[1].get("Name").is_some());
    assert_eq!(arr[1]["Name"], "validate-order");

    // Verify ParentId is present where expected
    assert!(arr[1].get("ParentId").is_some());
    assert_eq!(arr[1]["ParentId"], "exec-abc123");

    // Verify the JSON can be deserialized back to our types
    let deserialized: Vec<NodeJsHistoryEvent> = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.len(), 4);
    assert_eq!(
        deserialized[0].event_type,
        NodeJsEventType::ExecutionStarted
    );
    assert_eq!(deserialized[1].event_type, NodeJsEventType::StepStarted);
    assert_eq!(deserialized[2].event_type, NodeJsEventType::StepSucceeded);
    assert_eq!(
        deserialized[3].event_type,
        NodeJsEventType::ExecutionSucceeded
    );
}

/// Test that optional fields are omitted when None.
///
/// This ensures the JSON output is clean and matches Node.js SDK expectations.
#[test]
fn test_optional_fields_omitted_when_none() {
    let event = NodeJsHistoryEvent {
        event_type: NodeJsEventType::ExecutionStarted,
        event_id: 1,
        id: Some("exec-123".to_string()),
        event_timestamp: "2025-12-03T22:58:35.094Z".to_string(),
        sub_type: None,
        name: None,
        parent_id: None,
        details: NodeJsEventDetails::ExecutionStarted(ExecutionStartedDetailsWrapper {
            execution_started_details: ExecutionStartedDetails {
                input: PayloadWrapper::new("{}"),
                execution_timeout: None,
            },
        }),
    };

    let json = serde_json::to_string(&event).unwrap();

    // Optional fields should not appear in JSON when None
    assert!(
        !json.contains(r#""SubType""#),
        "SubType should be omitted when None"
    );
    assert!(
        !json.contains(r#""Name""#),
        "Name should be omitted when None"
    );
    assert!(
        !json.contains(r#""ParentId""#),
        "ParentId should be omitted when None"
    );
}

/// Test that all event types serialize correctly.
///
/// This ensures every event type in the enum can be serialized to JSON.
#[test]
fn test_all_event_types_serialize() {
    let event_types = vec![
        NodeJsEventType::ExecutionStarted,
        NodeJsEventType::ExecutionSucceeded,
        NodeJsEventType::ExecutionFailed,
        NodeJsEventType::StepStarted,
        NodeJsEventType::StepSucceeded,
        NodeJsEventType::StepFailed,
        NodeJsEventType::WaitStarted,
        NodeJsEventType::WaitSucceeded,
        NodeJsEventType::WaitCancelled,
        NodeJsEventType::CallbackStarted,
        NodeJsEventType::CallbackSucceeded,
        NodeJsEventType::CallbackFailed,
        NodeJsEventType::CallbackTimedOut,
        NodeJsEventType::ContextStarted,
        NodeJsEventType::ContextSucceeded,
        NodeJsEventType::ContextFailed,
        NodeJsEventType::ChainedInvokeStarted,
        NodeJsEventType::ChainedInvokeSucceeded,
        NodeJsEventType::ChainedInvokeFailed,
        NodeJsEventType::InvocationCompleted,
    ];

    for event_type in event_types {
        let json = serde_json::to_string(&event_type).unwrap();
        // Verify it serializes to a string (not an object or number)
        assert!(
            json.starts_with('"') && json.ends_with('"'),
            "Event type {:?} should serialize to a string, got: {}",
            event_type,
            json
        );

        // Verify it can be deserialized back
        let deserialized: NodeJsEventType = serde_json::from_str(&json).unwrap();
        assert_eq!(
            deserialized, event_type,
            "Event type {:?} should round-trip correctly",
            event_type
        );
    }
}
