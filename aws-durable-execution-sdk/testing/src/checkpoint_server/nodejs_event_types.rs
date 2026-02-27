//! Node.js SDK compatible event types for history tracking.
//!
//! This module defines event types that match the Node.js SDK's event history format,
//! enabling cross-SDK history comparison and consistent behavior across both SDKs.
//!
//! The types use PascalCase field names via serde's `rename_all` attribute to match
//! the Node.js SDK's JSON output format.

use serde::{Deserialize, Serialize};

/// Event types for Node.js SDK compatible history tracking.
///
/// These event types match the Node.js SDK's event type enumeration exactly,
/// providing granular Started/Succeeded/Failed events for each operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeJsEventType {
    /// An execution has started
    ExecutionStarted,
    /// An execution has succeeded
    ExecutionSucceeded,
    /// An execution has failed
    ExecutionFailed,
    /// A step operation has started
    StepStarted,
    /// A step operation has succeeded
    StepSucceeded,
    /// A step operation has failed
    StepFailed,
    /// A wait operation has started
    WaitStarted,
    /// A wait operation has succeeded
    WaitSucceeded,
    /// A wait operation was cancelled
    WaitCancelled,
    /// A callback operation has started
    CallbackStarted,
    /// A callback operation has succeeded
    CallbackSucceeded,
    /// A callback operation has failed
    CallbackFailed,
    /// A callback operation has timed out
    CallbackTimedOut,
    /// A context operation has started
    ContextStarted,
    /// A context operation has succeeded
    ContextSucceeded,
    /// A context operation has failed
    ContextFailed,
    /// A chained invoke operation has started
    ChainedInvokeStarted,
    /// A chained invoke operation has succeeded
    ChainedInvokeSucceeded,
    /// A chained invoke operation has failed
    ChainedInvokeFailed,
    /// An invocation has completed
    InvocationCompleted,
}

/// A history event in Node.js SDK compatible format.
///
/// This struct matches the Node.js SDK's event format exactly, using PascalCase
/// field names for JSON serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NodeJsHistoryEvent {
    /// The type of event (e.g., "ExecutionStarted", "StepSucceeded")
    pub event_type: NodeJsEventType,

    /// Sequential event ID starting from 1
    pub event_id: u64,

    /// The operation/execution ID (UUID)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// ISO 8601 timestamp (e.g., "2025-12-03T22:58:35.094Z")
    pub event_timestamp: String,

    /// Operation sub-type (e.g., "Step")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<String>,

    /// Operation name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Parent operation ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,

    /// Event-specific details (dynamically typed based on EventType)
    /// Uses flatten to merge details fields into the parent object
    #[serde(flatten)]
    pub details: NodeJsEventDetails,
}

/// Wrapper for payload data in event details.
///
/// Used to wrap input, result, and error payloads in a consistent format
/// matching the Node.js SDK's `{ Payload: "..." }` structure.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct PayloadWrapper {
    /// The payload data as a JSON string
    pub payload: String,
}

impl PayloadWrapper {
    /// Create a new PayloadWrapper with the given payload.
    pub fn new(payload: impl Into<String>) -> Self {
        Self {
            payload: payload.into(),
        }
    }

    /// Create an empty PayloadWrapper.
    pub fn empty() -> Self {
        Self {
            payload: String::new(),
        }
    }
}

/// Retry details for step operations.
///
/// Contains information about retry attempts for step operations,
/// matching the Node.js SDK's retry details structure.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct RetryDetails {
    /// The current attempt number (1-indexed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_attempt: Option<u32>,

    /// Delay in seconds before the next retry attempt
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_attempt_delay_seconds: Option<u64>,
}

impl RetryDetails {
    /// Create new RetryDetails with the given attempt number.
    pub fn new(current_attempt: u32) -> Self {
        Self {
            current_attempt: Some(current_attempt),
            next_attempt_delay_seconds: None,
        }
    }

    /// Create new RetryDetails with attempt and delay.
    pub fn with_delay(current_attempt: u32, delay_seconds: u64) -> Self {
        Self {
            current_attempt: Some(current_attempt),
            next_attempt_delay_seconds: Some(delay_seconds),
        }
    }

    /// Create empty RetryDetails.
    pub fn empty() -> Self {
        Self::default()
    }
}

/// Event-specific details for Node.js SDK compatible events.
///
/// Each variant corresponds to a specific event type and contains
/// the appropriate details structure for that event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum NodeJsEventDetails {
    /// Details for ExecutionStarted events
    ExecutionStarted(ExecutionStartedDetailsWrapper),
    /// Details for ExecutionSucceeded events
    ExecutionSucceeded(ExecutionSucceededDetailsWrapper),
    /// Details for ExecutionFailed events
    ExecutionFailed(ExecutionFailedDetailsWrapper),
    /// Details for StepStarted events
    StepStarted(StepStartedDetailsWrapper),
    /// Details for StepSucceeded events
    StepSucceeded(StepSucceededDetailsWrapper),
    /// Details for StepFailed events
    StepFailed(StepFailedDetailsWrapper),
    /// Details for WaitStarted events
    WaitStarted(WaitStartedDetailsWrapper),
    /// Details for WaitSucceeded events
    WaitSucceeded(WaitSucceededDetailsWrapper),
    /// Details for CallbackStarted events
    CallbackStarted(CallbackStartedDetailsWrapper),
    /// Details for CallbackSucceeded events
    CallbackSucceeded(CallbackSucceededDetailsWrapper),
    /// Details for CallbackFailed events
    CallbackFailed(CallbackFailedDetailsWrapper),
    /// Details for ContextStarted events
    ContextStarted(ContextStartedDetailsWrapper),
    /// Details for ContextSucceeded events
    ContextSucceeded(ContextSucceededDetailsWrapper),
    /// Details for ContextFailed events
    ContextFailed(ContextFailedDetailsWrapper),
    /// Details for InvocationCompleted events
    InvocationCompleted(InvocationCompletedDetailsWrapper),
    /// Empty details (for events that don't have specific details)
    Empty(EmptyDetails),
}

impl Default for NodeJsEventDetails {
    fn default() -> Self {
        NodeJsEventDetails::Empty(EmptyDetails {})
    }
}

/// Empty details struct for events without specific details.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmptyDetails {}

// ============================================================================
// Detail Wrapper Structs
// These wrappers ensure the details are serialized with the correct field name
// (e.g., "ExecutionStartedDetails": { ... })
// ============================================================================

/// Wrapper for ExecutionStartedDetails to ensure correct field name in JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ExecutionStartedDetailsWrapper {
    /// The execution started details
    pub execution_started_details: ExecutionStartedDetails,
}

/// Wrapper for ExecutionSucceededDetails to ensure correct field name in JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ExecutionSucceededDetailsWrapper {
    /// The execution succeeded details
    pub execution_succeeded_details: ExecutionSucceededDetails,
}

/// Wrapper for ExecutionFailedDetails to ensure correct field name in JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ExecutionFailedDetailsWrapper {
    /// The execution failed details
    pub execution_failed_details: ExecutionFailedDetails,
}

/// Wrapper for StepStartedDetails to ensure correct field name in JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct StepStartedDetailsWrapper {
    /// The step started details
    pub step_started_details: StepStartedDetails,
}

/// Wrapper for StepSucceededDetails to ensure correct field name in JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct StepSucceededDetailsWrapper {
    /// The step succeeded details
    pub step_succeeded_details: StepSucceededDetails,
}

/// Wrapper for StepFailedDetails to ensure correct field name in JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct StepFailedDetailsWrapper {
    /// The step failed details
    pub step_failed_details: StepFailedDetails,
}

/// Wrapper for WaitStartedDetails to ensure correct field name in JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct WaitStartedDetailsWrapper {
    /// The wait started details
    pub wait_started_details: WaitStartedDetails,
}

/// Wrapper for WaitSucceededDetails to ensure correct field name in JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct WaitSucceededDetailsWrapper {
    /// The wait succeeded details
    pub wait_succeeded_details: WaitSucceededDetails,
}

/// Wrapper for CallbackStartedDetails to ensure correct field name in JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct CallbackStartedDetailsWrapper {
    /// The callback started details
    pub callback_started_details: CallbackStartedDetails,
}

/// Wrapper for CallbackSucceededDetails to ensure correct field name in JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct CallbackSucceededDetailsWrapper {
    /// The callback succeeded details
    pub callback_succeeded_details: CallbackSucceededDetails,
}

/// Wrapper for CallbackFailedDetails to ensure correct field name in JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct CallbackFailedDetailsWrapper {
    /// The callback failed details
    pub callback_failed_details: CallbackFailedDetails,
}

/// Wrapper for ContextStartedDetails to ensure correct field name in JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct ContextStartedDetailsWrapper {
    /// The context started details
    pub context_started_details: ContextStartedDetails,
}

/// Wrapper for ContextSucceededDetails to ensure correct field name in JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ContextSucceededDetailsWrapper {
    /// The context succeeded details
    pub context_succeeded_details: ContextSucceededDetails,
}

/// Wrapper for ContextFailedDetails to ensure correct field name in JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ContextFailedDetailsWrapper {
    /// The context failed details
    pub context_failed_details: ContextFailedDetails,
}

/// Wrapper for InvocationCompletedDetails to ensure correct field name in JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct InvocationCompletedDetailsWrapper {
    /// The invocation completed details
    pub invocation_completed_details: InvocationCompletedDetails,
}

// ============================================================================
// Detail Structs
// ============================================================================

/// Details for ExecutionStarted events.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ExecutionStartedDetails {
    /// The input payload for the execution
    pub input: PayloadWrapper,

    /// Optional execution timeout in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_timeout: Option<u64>,
}

/// Details for ExecutionSucceeded events.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ExecutionSucceededDetails {
    /// The result payload of the execution
    pub result: PayloadWrapper,
}

/// Details for ExecutionFailed events.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ExecutionFailedDetails {
    /// The error payload of the execution
    pub error: PayloadWrapper,
}

/// Details for StepStarted events.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct StepStartedDetails {}

/// Details for StepSucceeded events.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct StepSucceededDetails {
    /// The result payload of the step
    pub result: PayloadWrapper,

    /// Retry details for the step
    pub retry_details: RetryDetails,
}

/// Details for StepFailed events.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct StepFailedDetails {
    /// The error payload of the step
    pub error: PayloadWrapper,

    /// Retry details for the step
    pub retry_details: RetryDetails,
}

/// Details for WaitStarted events.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct WaitStartedDetails {
    /// The duration of the wait in ISO 8601 duration format
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,

    /// The scheduled end timestamp in ISO 8601 format
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_end_timestamp: Option<String>,
}

/// Details for WaitSucceeded events.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct WaitSucceededDetails {}

/// Details for CallbackStarted events.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct CallbackStartedDetails {
    /// The callback ID
    pub callback_id: String,

    /// The callback timeout in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,

    /// The heartbeat timeout in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heartbeat_timeout: Option<u64>,

    /// The input payload for the callback
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<PayloadWrapper>,
}

/// Details for CallbackSucceeded events.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct CallbackSucceededDetails {
    /// The result payload of the callback
    pub result: PayloadWrapper,
}

/// Details for CallbackFailed events.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct CallbackFailedDetails {
    /// The error payload of the callback
    pub error: PayloadWrapper,
}

/// Details for ContextStarted events.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct ContextStartedDetails {}

/// Details for ContextSucceeded events.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ContextSucceededDetails {
    /// The result payload of the context
    pub result: PayloadWrapper,
}

/// Details for ContextFailed events.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ContextFailedDetails {
    /// The error payload of the context
    pub error: PayloadWrapper,
}

/// Error wrapper for invocation completed details.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct ErrorWrapper {
    /// The error name/type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The error message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl ErrorWrapper {
    /// Create an empty error wrapper (no error).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Create an error wrapper with the given name and message.
    pub fn new(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            message: Some(message.into()),
        }
    }
}

/// Details for InvocationCompleted events.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct InvocationCompletedDetails {
    /// The start timestamp of the invocation in ISO 8601 format
    pub start_timestamp: String,

    /// The end timestamp of the invocation in ISO 8601 format
    pub end_timestamp: String,

    /// The request ID of the invocation
    pub request_id: String,

    /// Error information (empty object if no error)
    pub error: ErrorWrapper,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nodejs_event_type_serialization() {
        let event_type = NodeJsEventType::ExecutionStarted;
        let json = serde_json::to_string(&event_type).unwrap();
        assert_eq!(json, r#""ExecutionStarted""#);

        let deserialized: NodeJsEventType = serde_json::from_str(&json).unwrap();
        assert_eq!(event_type, deserialized);
    }

    #[test]
    fn test_all_event_types_serialize_correctly() {
        let event_types = vec![
            (NodeJsEventType::ExecutionStarted, "ExecutionStarted"),
            (NodeJsEventType::ExecutionSucceeded, "ExecutionSucceeded"),
            (NodeJsEventType::ExecutionFailed, "ExecutionFailed"),
            (NodeJsEventType::StepStarted, "StepStarted"),
            (NodeJsEventType::StepSucceeded, "StepSucceeded"),
            (NodeJsEventType::StepFailed, "StepFailed"),
            (NodeJsEventType::WaitStarted, "WaitStarted"),
            (NodeJsEventType::WaitSucceeded, "WaitSucceeded"),
            (NodeJsEventType::CallbackStarted, "CallbackStarted"),
            (NodeJsEventType::ContextStarted, "ContextStarted"),
            (NodeJsEventType::ContextSucceeded, "ContextSucceeded"),
            (NodeJsEventType::ContextFailed, "ContextFailed"),
            (NodeJsEventType::InvocationCompleted, "InvocationCompleted"),
        ];

        for (event_type, expected_str) in event_types {
            let json = serde_json::to_string(&event_type).unwrap();
            assert_eq!(json, format!(r#""{}""#, expected_str));
        }
    }

    #[test]
    fn test_payload_wrapper_serialization() {
        let wrapper = PayloadWrapper::new(r#"{"key": "value"}"#);
        let json = serde_json::to_string(&wrapper).unwrap();
        assert!(json.contains(r#""Payload""#));

        let deserialized: PayloadWrapper = serde_json::from_str(&json).unwrap();
        assert_eq!(wrapper, deserialized);
    }

    #[test]
    fn test_retry_details_serialization() {
        let details = RetryDetails::with_delay(2, 5);
        let json = serde_json::to_string(&details).unwrap();
        assert!(json.contains(r#""CurrentAttempt":2"#));
        assert!(json.contains(r#""NextAttemptDelaySeconds":5"#));

        let deserialized: RetryDetails = serde_json::from_str(&json).unwrap();
        assert_eq!(details, deserialized);
    }

    #[test]
    fn test_retry_details_skips_none_fields() {
        let details = RetryDetails::empty();
        let json = serde_json::to_string(&details).unwrap();
        assert_eq!(json, "{}");
    }

    #[test]
    fn test_nodejs_history_event_serialization() {
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

        let json = serde_json::to_string_pretty(&event).unwrap();

        // Verify PascalCase field names
        assert!(json.contains(r#""EventType""#));
        assert!(json.contains(r#""EventId""#));
        assert!(json.contains(r#""Id""#));
        assert!(json.contains(r#""EventTimestamp""#));
        assert!(json.contains(r#""ExecutionStartedDetails""#));
        assert!(json.contains(r#""Input""#));
        assert!(json.contains(r#""Payload""#));

        // Verify optional fields are not present when None
        assert!(!json.contains(r#""SubType""#));
        assert!(!json.contains(r#""Name""#));
        assert!(!json.contains(r#""ParentId""#));
    }

    #[test]
    fn test_nodejs_history_event_with_optional_fields() {
        let event = NodeJsHistoryEvent {
            event_type: NodeJsEventType::StepStarted,
            event_id: 2,
            id: Some("step-456".to_string()),
            event_timestamp: "2025-12-03T22:58:35.096Z".to_string(),
            sub_type: Some("Step".to_string()),
            name: Some("my-step".to_string()),
            parent_id: Some("exec-123".to_string()),
            details: NodeJsEventDetails::StepStarted(StepStartedDetailsWrapper {
                step_started_details: StepStartedDetails {},
            }),
        };

        let json = serde_json::to_string_pretty(&event).unwrap();

        // Verify optional fields are present
        assert!(json.contains(r#""SubType""#));
        assert!(json.contains(r#""Name""#));
        assert!(json.contains(r#""ParentId""#));
        assert!(json.contains(r#""StepStartedDetails""#));
    }

    #[test]
    fn test_execution_started_details() {
        let details = ExecutionStartedDetails {
            input: PayloadWrapper::new(r#"{"name": "test"}"#),
            execution_timeout: Some(300),
        };

        let json = serde_json::to_string(&details).unwrap();
        assert!(json.contains(r#""Input""#));
        assert!(json.contains(r#""ExecutionTimeout":300"#));
    }

    #[test]
    fn test_step_succeeded_details() {
        let details = StepSucceededDetails {
            result: PayloadWrapper::new(r#"{"result": 42}"#),
            retry_details: RetryDetails::new(1),
        };

        let json = serde_json::to_string(&details).unwrap();
        assert!(json.contains(r#""Result""#));
        assert!(json.contains(r#""RetryDetails""#));
        assert!(json.contains(r#""CurrentAttempt":1"#));
    }

    #[test]
    fn test_invocation_completed_details() {
        let details = InvocationCompletedDetails {
            start_timestamp: "2025-12-03T22:58:35.094Z".to_string(),
            end_timestamp: "2025-12-03T22:58:36.094Z".to_string(),
            request_id: "req-789".to_string(),
            error: ErrorWrapper::empty(),
        };

        let json = serde_json::to_string(&details).unwrap();
        assert!(json.contains(r#""StartTimestamp""#));
        assert!(json.contains(r#""EndTimestamp""#));
        assert!(json.contains(r#""RequestId""#));
        assert!(json.contains(r#""Error""#));
    }

    #[test]
    fn test_error_wrapper() {
        let error = ErrorWrapper::new("ValidationError", "Invalid input");
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains(r#""Name":"ValidationError""#));
        assert!(json.contains(r#""Message":"Invalid input""#));
    }

    #[test]
    fn test_empty_error_wrapper() {
        let error = ErrorWrapper::empty();
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(json, "{}");
    }

    #[test]
    fn test_callback_started_details() {
        let details = CallbackStartedDetails {
            callback_id: "cb-123".to_string(),
            timeout: Some(60),
            heartbeat_timeout: Some(10),
            input: Some(PayloadWrapper::new("{}")),
        };

        let json = serde_json::to_string(&details).unwrap();
        assert!(json.contains(r#""CallbackId":"cb-123""#));
        assert!(json.contains(r#""Timeout":60"#));
        assert!(json.contains(r#""HeartbeatTimeout":10"#));
        assert!(json.contains(r#""Input""#));
    }

    #[test]
    fn test_wait_started_details() {
        let details = WaitStartedDetails {
            duration: Some("PT30S".to_string()),
            scheduled_end_timestamp: Some("2025-12-03T22:59:05.094Z".to_string()),
        };

        let json = serde_json::to_string(&details).unwrap();
        assert!(json.contains(r#""Duration":"PT30S""#));
        assert!(json.contains(r#""ScheduledEndTimestamp""#));
    }
}
