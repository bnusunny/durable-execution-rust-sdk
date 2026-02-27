//! Event processor for generating history events.
//!
//! This module implements the EventProcessor which generates history events
//! for execution tracking, matching the Node.js SDK's event processor.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use aws_durable_execution_sdk::operation::{
    Operation, OperationAction, OperationType, OperationUpdate,
};

use super::nodejs_event_types::{
    CallbackStartedDetails, CallbackStartedDetailsWrapper, ContextFailedDetails,
    ContextFailedDetailsWrapper, ContextStartedDetails, ContextStartedDetailsWrapper,
    ContextSucceededDetails, ContextSucceededDetailsWrapper, ExecutionFailedDetails,
    ExecutionFailedDetailsWrapper, ExecutionStartedDetails, ExecutionStartedDetailsWrapper,
    ExecutionSucceededDetails, ExecutionSucceededDetailsWrapper, NodeJsEventDetails,
    NodeJsEventType, NodeJsHistoryEvent, PayloadWrapper, RetryDetails, StepFailedDetails,
    StepFailedDetailsWrapper, StepStartedDetails, StepStartedDetailsWrapper, StepSucceededDetails,
    StepSucceededDetailsWrapper, WaitStartedDetails, WaitStartedDetailsWrapper,
    WaitSucceededDetails, WaitSucceededDetailsWrapper,
};

/// Event types for history tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventType {
    /// An operation has started
    OperationStarted,
    /// An operation has completed
    OperationCompleted,
    /// An invocation has started
    InvocationStarted,
    /// An invocation has completed
    InvocationCompleted,
    /// Execution has started
    ExecutionStarted,
    /// Execution has completed
    ExecutionCompleted,
}

/// A history event generated during execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEvent {
    /// Unique event ID
    pub id: u64,
    /// The type of event
    pub event_type: EventType,
    /// Timestamp when the event occurred
    pub timestamp: DateTime<Utc>,
    /// Optional operation ID this event relates to
    pub operation_id: Option<String>,
    /// The details type (e.g., "InvocationCompletedDetails")
    pub details_type: String,
    /// The event details as JSON
    pub details: serde_json::Value,
}

/// Processes and generates history events for execution tracking.
#[derive(Debug, Default)]
pub struct EventProcessor {
    /// All generated events (legacy format)
    events: Vec<HistoryEvent>,
    /// Counter for generating unique event IDs (legacy format)
    event_counter: u64,
    /// Node.js-compatible events
    nodejs_events: Vec<NodeJsHistoryEvent>,
    /// Counter for generating unique Node.js event IDs (starts from 1)
    nodejs_event_counter: u64,
}

impl EventProcessor {
    /// Create a new event processor.
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            event_counter: 0,
            nodejs_events: Vec::new(),
            nodejs_event_counter: 0,
        }
    }

    /// Format a timestamp as ISO 8601 string with millisecond precision in UTC.
    ///
    /// # Arguments
    ///
    /// * `timestamp` - The DateTime to format
    ///
    /// # Returns
    ///
    /// A string in the format "2025-12-03T22:58:35.094Z"
    pub fn format_timestamp(timestamp: DateTime<Utc>) -> String {
        timestamp.to_rfc3339_opts(SecondsFormat::Millis, true)
    }

    /// Get the current timestamp formatted as ISO 8601 string.
    pub fn current_timestamp() -> String {
        Self::format_timestamp(Utc::now())
    }

    /// Create a Node.js-compatible history event.
    ///
    /// This method generates events with sequential EventId starting from 1,
    /// matching the Node.js SDK's event format.
    ///
    /// # Arguments
    ///
    /// * `event_type` - The type of event (e.g., ExecutionStarted, StepSucceeded)
    /// * `operation` - Optional operation this event relates to
    /// * `details` - Event-specific details
    ///
    /// # Returns
    ///
    /// The created Node.js-compatible history event.
    pub fn create_nodejs_event(
        &mut self,
        event_type: NodeJsEventType,
        operation: Option<&Operation>,
        details: NodeJsEventDetails,
    ) -> NodeJsHistoryEvent {
        self.nodejs_event_counter += 1;

        let event = NodeJsHistoryEvent {
            event_type,
            event_id: self.nodejs_event_counter,
            id: operation.map(|op| op.operation_id.clone()),
            event_timestamp: Self::current_timestamp(),
            sub_type: operation.and_then(|op| op.sub_type.clone()),
            name: operation.and_then(|op| op.name.clone()),
            parent_id: operation.and_then(|op| op.parent_id.clone()),
            details,
        };

        self.nodejs_events.push(event.clone());
        event
    }

    /// Process an operation update and generate appropriate Node.js-compatible events.
    ///
    /// This method maps (OperationAction, OperationType) combinations to the appropriate
    /// NodeJsEventType according to the design specification:
    ///
    /// | Action  | Type       | EventType           |
    /// |---------|------------|---------------------|
    /// | Start   | Execution  | ExecutionStarted    |
    /// | Succeed | Execution  | ExecutionSucceeded  |
    /// | Fail    | Execution  | ExecutionFailed     |
    /// | Start   | Step       | StepStarted         |
    /// | Succeed | Step       | StepSucceeded       |
    /// | Fail    | Step       | StepFailed          |
    /// | Start   | Wait       | WaitStarted         |
    /// | Succeed | Wait       | WaitSucceeded       |
    /// | Start   | Callback   | CallbackStarted     |
    /// | Start   | Context    | ContextStarted      |
    /// | Succeed | Context    | ContextSucceeded    |
    /// | Fail    | Context    | ContextFailed       |
    ///
    /// # Arguments
    ///
    /// * `update` - The operation update containing action and type
    /// * `operation` - The operation being updated
    ///
    /// # Returns
    ///
    /// A vector of generated Node.js-compatible history events.
    pub fn process_operation_update(
        &mut self,
        update: &OperationUpdate,
        operation: &Operation,
    ) -> Vec<NodeJsHistoryEvent> {
        let mut events = Vec::new();

        match (update.action, update.operation_type) {
            // Execution events
            (OperationAction::Start, OperationType::Execution) => {
                let details =
                    NodeJsEventDetails::ExecutionStarted(ExecutionStartedDetailsWrapper {
                        execution_started_details: ExecutionStartedDetails {
                            input: PayloadWrapper::new(update.result.clone().unwrap_or_default()),
                            execution_timeout: None,
                        },
                    });
                events.push(self.create_nodejs_event(
                    NodeJsEventType::ExecutionStarted,
                    Some(operation),
                    details,
                ));
            }
            (OperationAction::Succeed, OperationType::Execution) => {
                let details =
                    NodeJsEventDetails::ExecutionSucceeded(ExecutionSucceededDetailsWrapper {
                        execution_succeeded_details: ExecutionSucceededDetails {
                            result: PayloadWrapper::new(update.result.clone().unwrap_or_default()),
                        },
                    });
                events.push(self.create_nodejs_event(
                    NodeJsEventType::ExecutionSucceeded,
                    Some(operation),
                    details,
                ));
            }
            (OperationAction::Fail, OperationType::Execution) => {
                let error_payload = update
                    .error
                    .as_ref()
                    .map(|e| serde_json::to_string(e).unwrap_or_default())
                    .unwrap_or_default();
                let details = NodeJsEventDetails::ExecutionFailed(ExecutionFailedDetailsWrapper {
                    execution_failed_details: ExecutionFailedDetails {
                        error: PayloadWrapper::new(error_payload),
                    },
                });
                events.push(self.create_nodejs_event(
                    NodeJsEventType::ExecutionFailed,
                    Some(operation),
                    details,
                ));
            }

            // Step events
            (OperationAction::Start, OperationType::Step) => {
                let details = NodeJsEventDetails::StepStarted(StepStartedDetailsWrapper {
                    step_started_details: StepStartedDetails {},
                });
                events.push(self.create_nodejs_event(
                    NodeJsEventType::StepStarted,
                    Some(operation),
                    details,
                ));
            }
            (OperationAction::Succeed, OperationType::Step) => {
                let retry_details = Self::extract_retry_details(update, operation);
                let details = NodeJsEventDetails::StepSucceeded(StepSucceededDetailsWrapper {
                    step_succeeded_details: StepSucceededDetails {
                        result: PayloadWrapper::new(update.result.clone().unwrap_or_default()),
                        retry_details,
                    },
                });
                events.push(self.create_nodejs_event(
                    NodeJsEventType::StepSucceeded,
                    Some(operation),
                    details,
                ));
            }
            (OperationAction::Fail, OperationType::Step) => {
                let retry_details = Self::extract_retry_details(update, operation);
                let error_payload = update
                    .error
                    .as_ref()
                    .map(|e| serde_json::to_string(e).unwrap_or_default())
                    .unwrap_or_default();
                let details = NodeJsEventDetails::StepFailed(StepFailedDetailsWrapper {
                    step_failed_details: StepFailedDetails {
                        error: PayloadWrapper::new(error_payload),
                        retry_details,
                    },
                });
                events.push(self.create_nodejs_event(
                    NodeJsEventType::StepFailed,
                    Some(operation),
                    details,
                ));
            }
            (OperationAction::Retry, OperationType::Step) => {
                // Retry generates a StepFailed event with retry details
                let retry_details = Self::extract_retry_details(update, operation);
                let error_payload = update
                    .error
                    .as_ref()
                    .map(|e| serde_json::to_string(e).unwrap_or_default())
                    .unwrap_or_default();
                let details = NodeJsEventDetails::StepFailed(StepFailedDetailsWrapper {
                    step_failed_details: StepFailedDetails {
                        error: PayloadWrapper::new(error_payload),
                        retry_details,
                    },
                });
                events.push(self.create_nodejs_event(
                    NodeJsEventType::StepFailed,
                    Some(operation),
                    details,
                ));
            }

            // Wait events
            (OperationAction::Start, OperationType::Wait) => {
                let (duration, scheduled_end) = Self::extract_wait_details(update);
                let details = NodeJsEventDetails::WaitStarted(WaitStartedDetailsWrapper {
                    wait_started_details: WaitStartedDetails {
                        duration,
                        scheduled_end_timestamp: scheduled_end,
                    },
                });
                events.push(self.create_nodejs_event(
                    NodeJsEventType::WaitStarted,
                    Some(operation),
                    details,
                ));
            }
            (OperationAction::Succeed, OperationType::Wait) => {
                let details = NodeJsEventDetails::WaitSucceeded(WaitSucceededDetailsWrapper {
                    wait_succeeded_details: WaitSucceededDetails {},
                });
                events.push(self.create_nodejs_event(
                    NodeJsEventType::WaitSucceeded,
                    Some(operation),
                    details,
                ));
            }

            // Callback events
            (OperationAction::Start, OperationType::Callback) => {
                let (callback_id, timeout, heartbeat_timeout, input) =
                    Self::extract_callback_details(update, operation);
                let details = NodeJsEventDetails::CallbackStarted(CallbackStartedDetailsWrapper {
                    callback_started_details: CallbackStartedDetails {
                        callback_id,
                        timeout,
                        heartbeat_timeout,
                        input,
                    },
                });
                events.push(self.create_nodejs_event(
                    NodeJsEventType::CallbackStarted,
                    Some(operation),
                    details,
                ));
            }

            // Context events
            (OperationAction::Start, OperationType::Context) => {
                let details = NodeJsEventDetails::ContextStarted(ContextStartedDetailsWrapper {
                    context_started_details: ContextStartedDetails {},
                });
                events.push(self.create_nodejs_event(
                    NodeJsEventType::ContextStarted,
                    Some(operation),
                    details,
                ));
            }
            (OperationAction::Succeed, OperationType::Context) => {
                let details =
                    NodeJsEventDetails::ContextSucceeded(ContextSucceededDetailsWrapper {
                        context_succeeded_details: ContextSucceededDetails {
                            result: PayloadWrapper::new(update.result.clone().unwrap_or_default()),
                        },
                    });
                events.push(self.create_nodejs_event(
                    NodeJsEventType::ContextSucceeded,
                    Some(operation),
                    details,
                ));
            }
            (OperationAction::Fail, OperationType::Context) => {
                let error_payload = update
                    .error
                    .as_ref()
                    .map(|e| serde_json::to_string(e).unwrap_or_default())
                    .unwrap_or_default();
                let details = NodeJsEventDetails::ContextFailed(ContextFailedDetailsWrapper {
                    context_failed_details: ContextFailedDetails {
                        error: PayloadWrapper::new(error_payload),
                    },
                });
                events.push(self.create_nodejs_event(
                    NodeJsEventType::ContextFailed,
                    Some(operation),
                    details,
                ));
            }

            // Other combinations are not mapped (e.g., Cancel actions, Invoke type)
            _ => {}
        }

        events
    }

    /// Extract retry details from an operation update.
    fn extract_retry_details(update: &OperationUpdate, operation: &Operation) -> RetryDetails {
        let current_attempt = operation
            .step_details
            .as_ref()
            .and_then(|d| d.attempt)
            .map(|a| a + 1); // Convert 0-indexed to 1-indexed

        let next_attempt_delay_seconds = update
            .step_options
            .as_ref()
            .and_then(|o| o.next_attempt_delay_seconds);

        RetryDetails {
            current_attempt,
            next_attempt_delay_seconds,
        }
    }

    /// Extract wait details from an operation update.
    fn extract_wait_details(update: &OperationUpdate) -> (Option<String>, Option<String>) {
        let wait_seconds = update.wait_options.as_ref().map(|o| o.wait_seconds);

        let duration = wait_seconds.map(|s| format!("PT{}S", s));

        let scheduled_end = wait_seconds.map(|s| {
            let end_time = Utc::now() + chrono::Duration::seconds(s as i64);
            Self::format_timestamp(end_time)
        });

        (duration, scheduled_end)
    }

    /// Extract callback details from an operation update.
    fn extract_callback_details(
        update: &OperationUpdate,
        operation: &Operation,
    ) -> (String, Option<u64>, Option<u64>, Option<PayloadWrapper>) {
        let callback_id = operation
            .callback_details
            .as_ref()
            .and_then(|d| d.callback_id.clone())
            .unwrap_or_else(|| operation.operation_id.clone());

        let timeout = update
            .callback_options
            .as_ref()
            .and_then(|o| o.timeout_seconds);
        let heartbeat_timeout = update
            .callback_options
            .as_ref()
            .and_then(|o| o.heartbeat_timeout_seconds);

        let input = update
            .result
            .as_ref()
            .map(|r| PayloadWrapper::new(r.clone()));

        (callback_id, timeout, heartbeat_timeout, input)
    }

    /// Get all Node.js-compatible events.
    pub fn get_nodejs_events(&self) -> &[NodeJsHistoryEvent] {
        &self.nodejs_events
    }

    /// Get Node.js-compatible events as owned vector.
    pub fn into_nodejs_events(self) -> Vec<NodeJsHistoryEvent> {
        self.nodejs_events
    }

    /// Get the current Node.js event count.
    pub fn nodejs_event_count(&self) -> u64 {
        self.nodejs_event_counter
    }

    /// Clear all Node.js-compatible events.
    pub fn clear_nodejs_events(&mut self) {
        self.nodejs_events.clear();
        self.nodejs_event_counter = 0;
    }

    /// Create a history event.
    ///
    /// # Arguments
    ///
    /// * `event_type` - The type of event
    /// * `operation_id` - Optional operation ID this event relates to
    /// * `details_type` - The details type name
    /// * `details` - The event details (will be serialized to JSON)
    ///
    /// # Returns
    ///
    /// The created history event.
    pub fn create_history_event<T: Serialize>(
        &mut self,
        event_type: EventType,
        operation_id: Option<&str>,
        details_type: &str,
        details: T,
    ) -> HistoryEvent {
        self.event_counter += 1;

        let event = HistoryEvent {
            id: self.event_counter,
            event_type,
            timestamp: Utc::now(),
            operation_id: operation_id.map(String::from),
            details_type: details_type.to_string(),
            details: serde_json::to_value(details).unwrap_or(serde_json::Value::Null),
        };

        self.events.push(event.clone());
        event
    }

    /// Get all events.
    pub fn get_events(&self) -> &[HistoryEvent] {
        &self.events
    }

    /// Get events as owned vector.
    pub fn into_events(self) -> Vec<HistoryEvent> {
        self.events
    }

    /// Clear all events.
    pub fn clear(&mut self) {
        self.events.clear();
        self.event_counter = 0;
        self.nodejs_events.clear();
        self.nodejs_event_counter = 0;
    }

    /// Get the current event count.
    pub fn event_count(&self) -> u64 {
        self.event_counter
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_durable_execution_sdk::operation::{CallbackDetails, CallbackOptions, StepDetails};

    #[derive(Serialize)]
    struct TestDetails {
        message: String,
    }

    #[test]
    fn test_create_history_event() {
        let mut processor = EventProcessor::new();

        let event = processor.create_history_event(
            EventType::OperationStarted,
            Some("op-123"),
            "TestDetails",
            TestDetails {
                message: "test".to_string(),
            },
        );

        assert_eq!(event.id, 1);
        assert_eq!(event.event_type, EventType::OperationStarted);
        assert_eq!(event.operation_id, Some("op-123".to_string()));
        assert_eq!(event.details_type, "TestDetails");
        assert_eq!(event.details["message"], "test");
    }

    #[test]
    fn test_event_counter_increments() {
        let mut processor = EventProcessor::new();

        let event1 = processor.create_history_event(
            EventType::OperationStarted,
            None,
            "Details",
            serde_json::json!({}),
        );
        let event2 = processor.create_history_event(
            EventType::OperationCompleted,
            None,
            "Details",
            serde_json::json!({}),
        );

        assert_eq!(event1.id, 1);
        assert_eq!(event2.id, 2);
        assert_eq!(processor.event_count(), 2);
    }

    #[test]
    fn test_get_events() {
        let mut processor = EventProcessor::new();

        processor.create_history_event(
            EventType::OperationStarted,
            Some("op-1"),
            "Details",
            serde_json::json!({}),
        );
        processor.create_history_event(
            EventType::OperationCompleted,
            Some("op-1"),
            "Details",
            serde_json::json!({}),
        );

        let events = processor.get_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, EventType::OperationStarted);
        assert_eq!(events[1].event_type, EventType::OperationCompleted);
    }

    #[test]
    fn test_clear_events() {
        let mut processor = EventProcessor::new();

        processor.create_history_event(
            EventType::OperationStarted,
            None,
            "Details",
            serde_json::json!({}),
        );

        assert_eq!(processor.event_count(), 1);

        processor.clear();

        assert_eq!(processor.event_count(), 0);
        assert!(processor.get_events().is_empty());
    }

    #[test]
    fn test_event_without_operation_id() {
        let mut processor = EventProcessor::new();

        let event = processor.create_history_event(
            EventType::InvocationCompleted,
            None,
            "InvocationCompletedDetails",
            serde_json::json!({"status": "success"}),
        );

        assert_eq!(event.operation_id, None);
        assert_eq!(event.event_type, EventType::InvocationCompleted);
    }

    // ============================================================================
    // Node.js-compatible event tests
    // ============================================================================

    #[test]
    fn test_format_timestamp_iso8601() {
        use chrono::TimeZone;
        let timestamp = Utc.with_ymd_and_hms(2025, 12, 3, 22, 58, 35).unwrap()
            + chrono::Duration::milliseconds(94);
        let formatted = EventProcessor::format_timestamp(timestamp);
        assert_eq!(formatted, "2025-12-03T22:58:35.094Z");
    }

    #[test]
    fn test_current_timestamp_format() {
        let timestamp = EventProcessor::current_timestamp();
        // Verify ISO 8601 format with milliseconds and Z suffix
        let re = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$").unwrap();
        assert!(
            re.is_match(&timestamp),
            "Timestamp '{}' does not match ISO 8601 format",
            timestamp
        );
    }

    #[test]
    fn test_create_nodejs_event_sequential_ids() {
        let mut processor = EventProcessor::new();

        let event1 = processor.create_nodejs_event(
            NodeJsEventType::ExecutionStarted,
            None,
            NodeJsEventDetails::default(),
        );
        let event2 = processor.create_nodejs_event(
            NodeJsEventType::StepStarted,
            None,
            NodeJsEventDetails::default(),
        );
        let event3 = processor.create_nodejs_event(
            NodeJsEventType::StepSucceeded,
            None,
            NodeJsEventDetails::default(),
        );

        assert_eq!(event1.event_id, 1);
        assert_eq!(event2.event_id, 2);
        assert_eq!(event3.event_id, 3);
        assert_eq!(processor.nodejs_event_count(), 3);
    }

    #[test]
    fn test_create_nodejs_event_with_operation() {
        let mut processor = EventProcessor::new();

        let mut operation = Operation::new("step-123", OperationType::Step);
        operation.name = Some("my-step".to_string());
        operation.sub_type = Some("Step".to_string());
        operation.parent_id = Some("exec-001".to_string());

        let event = processor.create_nodejs_event(
            NodeJsEventType::StepStarted,
            Some(&operation),
            NodeJsEventDetails::default(),
        );

        assert_eq!(event.event_type, NodeJsEventType::StepStarted);
        assert_eq!(event.id, Some("step-123".to_string()));
        assert_eq!(event.name, Some("my-step".to_string()));
        assert_eq!(event.sub_type, Some("Step".to_string()));
        assert_eq!(event.parent_id, Some("exec-001".to_string()));
    }

    #[test]
    fn test_create_nodejs_event_without_operation() {
        let mut processor = EventProcessor::new();

        let event = processor.create_nodejs_event(
            NodeJsEventType::InvocationCompleted,
            None,
            NodeJsEventDetails::default(),
        );

        assert_eq!(event.event_type, NodeJsEventType::InvocationCompleted);
        assert_eq!(event.id, None);
        assert_eq!(event.name, None);
        assert_eq!(event.sub_type, None);
        assert_eq!(event.parent_id, None);
    }

    #[test]
    fn test_process_operation_update_execution_started() {
        let mut processor = EventProcessor::new();

        let operation = Operation::new("exec-001", OperationType::Execution);
        let mut update = OperationUpdate::start("exec-001", OperationType::Execution);
        update.result = Some(r#"{"input": "test"}"#.to_string());

        let events = processor.process_operation_update(&update, &operation);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, NodeJsEventType::ExecutionStarted);
        assert_eq!(events[0].id, Some("exec-001".to_string()));
    }

    #[test]
    fn test_process_operation_update_execution_succeeded() {
        let mut processor = EventProcessor::new();

        let operation = Operation::new("exec-001", OperationType::Execution);
        let update = OperationUpdate::succeed(
            "exec-001",
            OperationType::Execution,
            Some("result".to_string()),
        );

        let events = processor.process_operation_update(&update, &operation);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, NodeJsEventType::ExecutionSucceeded);
    }

    #[test]
    fn test_process_operation_update_step_started() {
        let mut processor = EventProcessor::new();

        let operation = Operation::new("step-001", OperationType::Step);
        let update = OperationUpdate::start("step-001", OperationType::Step);

        let events = processor.process_operation_update(&update, &operation);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, NodeJsEventType::StepStarted);
    }

    #[test]
    fn test_process_operation_update_step_succeeded() {
        let mut processor = EventProcessor::new();

        let mut operation = Operation::new("step-001", OperationType::Step);
        operation.step_details = Some(StepDetails {
            result: None,
            attempt: Some(0),
            next_attempt_timestamp: None,
            error: None,
            payload: None,
        });

        let update =
            OperationUpdate::succeed("step-001", OperationType::Step, Some("42".to_string()));

        let events = processor.process_operation_update(&update, &operation);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, NodeJsEventType::StepSucceeded);
    }

    #[test]
    fn test_process_operation_update_wait_started() {
        let mut processor = EventProcessor::new();

        let operation = Operation::new("wait-001", OperationType::Wait);
        let update = OperationUpdate::start_wait("wait-001", 30);

        let events = processor.process_operation_update(&update, &operation);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, NodeJsEventType::WaitStarted);
    }

    #[test]
    fn test_process_operation_update_wait_succeeded() {
        let mut processor = EventProcessor::new();

        let operation = Operation::new("wait-001", OperationType::Wait);
        let update = OperationUpdate::succeed("wait-001", OperationType::Wait, None);

        let events = processor.process_operation_update(&update, &operation);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, NodeJsEventType::WaitSucceeded);
    }

    #[test]
    fn test_process_operation_update_callback_started() {
        let mut processor = EventProcessor::new();

        let mut operation = Operation::new("cb-001", OperationType::Callback);
        operation.callback_details = Some(CallbackDetails {
            callback_id: Some("callback-token-123".to_string()),
            result: None,
            error: None,
        });

        let mut update = OperationUpdate::start("cb-001", OperationType::Callback);
        update.callback_options = Some(CallbackOptions {
            timeout_seconds: Some(60),
            heartbeat_timeout_seconds: Some(10),
        });

        let events = processor.process_operation_update(&update, &operation);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, NodeJsEventType::CallbackStarted);
    }

    #[test]
    fn test_process_operation_update_context_started() {
        let mut processor = EventProcessor::new();

        let operation = Operation::new("ctx-001", OperationType::Context);
        let update = OperationUpdate::start("ctx-001", OperationType::Context);

        let events = processor.process_operation_update(&update, &operation);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, NodeJsEventType::ContextStarted);
    }

    #[test]
    fn test_process_operation_update_context_succeeded() {
        let mut processor = EventProcessor::new();

        let operation = Operation::new("ctx-001", OperationType::Context);
        let update = OperationUpdate::succeed(
            "ctx-001",
            OperationType::Context,
            Some("result".to_string()),
        );

        let events = processor.process_operation_update(&update, &operation);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, NodeJsEventType::ContextSucceeded);
    }

    #[test]
    fn test_get_nodejs_events() {
        let mut processor = EventProcessor::new();

        processor.create_nodejs_event(
            NodeJsEventType::ExecutionStarted,
            None,
            NodeJsEventDetails::default(),
        );
        processor.create_nodejs_event(
            NodeJsEventType::StepStarted,
            None,
            NodeJsEventDetails::default(),
        );

        let events = processor.get_nodejs_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, NodeJsEventType::ExecutionStarted);
        assert_eq!(events[1].event_type, NodeJsEventType::StepStarted);
    }

    #[test]
    fn test_clear_nodejs_events() {
        let mut processor = EventProcessor::new();

        processor.create_nodejs_event(
            NodeJsEventType::ExecutionStarted,
            None,
            NodeJsEventDetails::default(),
        );

        assert_eq!(processor.nodejs_event_count(), 1);

        processor.clear_nodejs_events();

        assert_eq!(processor.nodejs_event_count(), 0);
        assert!(processor.get_nodejs_events().is_empty());
    }

    #[test]
    fn test_clear_clears_both_event_types() {
        let mut processor = EventProcessor::new();

        // Add legacy event
        processor.create_history_event(
            EventType::OperationStarted,
            None,
            "Details",
            serde_json::json!({}),
        );

        // Add Node.js event
        processor.create_nodejs_event(
            NodeJsEventType::ExecutionStarted,
            None,
            NodeJsEventDetails::default(),
        );

        assert_eq!(processor.event_count(), 1);
        assert_eq!(processor.nodejs_event_count(), 1);

        processor.clear();

        assert_eq!(processor.event_count(), 0);
        assert_eq!(processor.nodejs_event_count(), 0);
        assert!(processor.get_events().is_empty());
        assert!(processor.get_nodejs_events().is_empty());
    }

    #[test]
    fn test_event_type_mapping_all_combinations() {
        let mut processor = EventProcessor::new();

        // Test all documented (Action, Type) -> EventType mappings
        let test_cases = vec![
            (
                OperationAction::Start,
                OperationType::Execution,
                NodeJsEventType::ExecutionStarted,
            ),
            (
                OperationAction::Succeed,
                OperationType::Execution,
                NodeJsEventType::ExecutionSucceeded,
            ),
            (
                OperationAction::Fail,
                OperationType::Execution,
                NodeJsEventType::ExecutionFailed,
            ),
            (
                OperationAction::Start,
                OperationType::Step,
                NodeJsEventType::StepStarted,
            ),
            (
                OperationAction::Succeed,
                OperationType::Step,
                NodeJsEventType::StepSucceeded,
            ),
            (
                OperationAction::Fail,
                OperationType::Step,
                NodeJsEventType::StepFailed,
            ),
            (
                OperationAction::Start,
                OperationType::Wait,
                NodeJsEventType::WaitStarted,
            ),
            (
                OperationAction::Succeed,
                OperationType::Wait,
                NodeJsEventType::WaitSucceeded,
            ),
            (
                OperationAction::Start,
                OperationType::Callback,
                NodeJsEventType::CallbackStarted,
            ),
            (
                OperationAction::Start,
                OperationType::Context,
                NodeJsEventType::ContextStarted,
            ),
            (
                OperationAction::Succeed,
                OperationType::Context,
                NodeJsEventType::ContextSucceeded,
            ),
            (
                OperationAction::Fail,
                OperationType::Context,
                NodeJsEventType::ContextFailed,
            ),
        ];

        for (action, op_type, expected_event_type) in test_cases {
            processor.clear();

            let operation = Operation::new("test-op", op_type);
            let update = match action {
                OperationAction::Start => {
                    if op_type == OperationType::Wait {
                        OperationUpdate::start_wait("test-op", 10)
                    } else {
                        OperationUpdate::start("test-op", op_type)
                    }
                }
                OperationAction::Succeed => OperationUpdate::succeed("test-op", op_type, None),
                OperationAction::Fail => {
                    use aws_durable_execution_sdk::error::ErrorObject;
                    OperationUpdate::fail("test-op", op_type, ErrorObject::new("TestError", "test"))
                }
                _ => continue,
            };

            let events = processor.process_operation_update(&update, &operation);

            assert_eq!(
                events.len(),
                1,
                "Expected 1 event for {:?} + {:?}",
                action,
                op_type
            );
            assert_eq!(
                events[0].event_type, expected_event_type,
                "Expected {:?} for {:?} + {:?}, got {:?}",
                expected_event_type, action, op_type, events[0].event_type
            );
        }
    }
}
