//! Event processor for generating history events.
//!
//! This module implements the EventProcessor which generates history events
//! for execution tracking, matching the Node.js SDK's event processor.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
    /// All generated events
    events: Vec<HistoryEvent>,
    /// Counter for generating unique event IDs
    event_counter: u64,
}

impl EventProcessor {
    /// Create a new event processor.
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            event_counter: 0,
        }
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
    }

    /// Get the current event count.
    pub fn event_count(&self) -> u64 {
        self.event_counter
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
