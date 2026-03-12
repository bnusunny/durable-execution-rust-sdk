//! Checkpoint manager for managing checkpoints of a single execution.
//!
//! This module implements the CheckpointManager which manages checkpoints for
//! a single execution, including operation state, callback lifecycle, and
//! history events, matching the Node.js SDK's checkpoint manager.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use uuid::Uuid;

use durable_execution_sdk::{
    Operation, OperationAction, OperationStatus, OperationType, OperationUpdate,
};

use super::callback_manager::CallbackManager;
use super::event_processor::{EventProcessor, EventType, HistoryEvent};
use super::nodejs_event_types::{
    ErrorWrapper, ExecutionStartedDetails, ExecutionStartedDetailsWrapper,
    InvocationCompletedDetails, InvocationCompletedDetailsWrapper, NodeJsEventDetails,
    NodeJsEventType, NodeJsHistoryEvent, PayloadWrapper,
};
use super::types::{ExecutionId, InvocationId};
use crate::error::TestError;

/// Operation with associated event data.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OperationEvents {
    /// The operation data
    pub operation: Operation,
    /// The original update that created/modified this operation
    pub update: Option<OperationUpdate>,
}

/// A checkpoint operation with update tracking.
#[derive(Debug, Clone)]
pub struct CheckpointOperation {
    /// The operation events
    pub events: OperationEvents,
    /// Whether this operation has pending updates
    pub has_pending_update: bool,
}

/// Manages checkpoints for a single execution.
#[derive(Debug)]
pub struct CheckpointManager {
    /// The execution ID
    execution_id: ExecutionId,
    /// Map of operation ID to operation events
    operation_data_map: HashMap<String, OperationEvents>,
    /// Order of operation IDs (for preserving insertion order)
    operation_order: Vec<String>,
    /// Callback manager for this execution
    callback_manager: CallbackManager,
    /// Event processor for generating history events
    event_processor: EventProcessor,
    /// Map of invocation ID to start timestamp
    invocations_map: HashMap<InvocationId, DateTime<Utc>>,
    /// Set of operation IDs that have been modified since last checkpoint
    dirty_operation_ids: HashSet<String>,
    /// Whether the execution has completed
    is_execution_completed: bool,
}

impl CheckpointManager {
    /// Create a new checkpoint manager for an execution.
    pub fn new(execution_id: &str) -> Self {
        Self {
            execution_id: execution_id.to_string(),
            operation_data_map: HashMap::new(),
            operation_order: Vec::new(),
            callback_manager: CallbackManager::new(execution_id),
            event_processor: EventProcessor::new(),
            invocations_map: HashMap::new(),
            dirty_operation_ids: HashSet::new(),
            is_execution_completed: false,
        }
    }

    /// Initialize with the first operation (EXECUTION type).
    pub fn initialize(&mut self, payload: &str) -> OperationEvents {
        let initial_id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp_millis();

        let initial_operation = Operation {
            operation_id: initial_id.clone(),
            operation_type: OperationType::Execution,
            status: OperationStatus::Started,
            result: Some(payload.to_string()),
            error: None,
            parent_id: None,
            name: None,
            sub_type: None,
            start_timestamp: Some(now),
            end_timestamp: None,
            wait_details: None,
            step_details: None,
            callback_details: None,
            chained_invoke_details: None,
            context_details: None,
            execution_details: None,
        };

        let events = OperationEvents {
            operation: initial_operation.clone(),
            update: None,
        };

        self.operation_data_map
            .insert(initial_id.clone(), events.clone());
        self.operation_order.push(initial_id.clone());
        self.dirty_operation_ids.insert(initial_id);

        // Generate legacy history event
        self.event_processor.create_history_event(
            EventType::OperationStarted,
            None,
            "ExecutionStartedDetails",
            serde_json::json!({ "input_payload": payload }),
        );

        // Generate Node.js-compatible ExecutionStarted event
        let nodejs_details = NodeJsEventDetails::ExecutionStarted(ExecutionStartedDetailsWrapper {
            execution_started_details: ExecutionStartedDetails {
                input: PayloadWrapper::new(payload.to_string()),
                execution_timeout: None,
            },
        });
        self.event_processor.create_nodejs_event(
            NodeJsEventType::ExecutionStarted,
            Some(&events.operation),
            nodejs_details,
        );

        events
    }

    /// Get operation data by ID.
    pub fn get_operation_data(&self, operation_id: &str) -> Option<&OperationEvents> {
        self.operation_data_map.get(operation_id)
    }

    /// Get all operation data.
    pub fn get_all_operation_data(&self) -> &HashMap<String, OperationEvents> {
        &self.operation_data_map
    }

    /// Check if there are dirty operations.
    pub fn has_dirty_operations(&self) -> bool {
        !self.dirty_operation_ids.is_empty()
    }

    /// Get and clear dirty operations.
    pub fn get_dirty_operations(&mut self) -> Vec<Operation> {
        let dirty_ops: Vec<Operation> = self
            .dirty_operation_ids
            .iter()
            .filter_map(|id| self.operation_data_map.get(id).map(|e| e.operation.clone()))
            .collect();

        self.dirty_operation_ids.clear();
        dirty_ops
    }

    /// Check if execution is completed.
    pub fn is_execution_completed(&self) -> bool {
        self.is_execution_completed
    }

    /// Start an invocation.
    pub fn start_invocation(&mut self, invocation_id: &str) -> Vec<OperationEvents> {
        self.invocations_map
            .insert(invocation_id.to_string(), Utc::now());

        // Clear dirty operations since client should know state from operation data
        self.dirty_operation_ids.clear();

        // Return operations in insertion order
        self.operation_order
            .iter()
            .filter_map(|id| self.operation_data_map.get(id).cloned())
            .collect()
    }

    /// Complete an invocation.
    pub fn complete_invocation(
        &mut self,
        invocation_id: &str,
    ) -> Result<InvocationTimestamps, TestError> {
        let start_timestamp = self
            .invocations_map
            .remove(invocation_id)
            .ok_or_else(|| TestError::InvocationNotFound(invocation_id.to_string()))?;

        let end_timestamp = Utc::now();

        // Generate legacy history event
        self.event_processor.create_history_event(
            EventType::InvocationCompleted,
            None,
            "InvocationCompletedDetails",
            serde_json::json!({
                "start_timestamp": start_timestamp,
                "end_timestamp": end_timestamp,
                "request_id": invocation_id,
            }),
        );

        // Generate Node.js-compatible InvocationCompleted event
        let nodejs_details =
            NodeJsEventDetails::InvocationCompleted(InvocationCompletedDetailsWrapper {
                invocation_completed_details: InvocationCompletedDetails {
                    start_timestamp: EventProcessor::format_timestamp(start_timestamp),
                    end_timestamp: EventProcessor::format_timestamp(end_timestamp),
                    request_id: invocation_id.to_string(),
                    error: ErrorWrapper::empty(),
                },
            });
        self.event_processor.create_nodejs_event(
            NodeJsEventType::InvocationCompleted,
            None,
            nodejs_details,
        );

        Ok(InvocationTimestamps {
            start_timestamp,
            end_timestamp,
        })
    }

    /// Get current state (all operations in insertion order).
    pub fn get_state(&self) -> Vec<Operation> {
        self.operation_order
            .iter()
            .filter_map(|id| self.operation_data_map.get(id).map(|e| e.operation.clone()))
            .collect()
    }

    /// Process a checkpoint with operation updates.
    pub fn process_checkpoint(
        &mut self,
        updates: Vec<OperationUpdate>,
    ) -> Result<Vec<Operation>, TestError> {
        for update in updates {
            self.process_operation_update(update)?;
        }

        Ok(self.get_dirty_operations())
    }

    /// Process a single operation update.
    fn process_operation_update(&mut self, update: OperationUpdate) -> Result<(), TestError> {
        let operation_id = update.operation_id.clone();
        let now = Utc::now().timestamp_millis();

        // Get or create operation
        let operation = if let Some(existing) = self.operation_data_map.get(&operation_id) {
            let mut op = existing.operation.clone();
            // Merge update into existing operation
            self.merge_operation_update(&mut op, &update, now);
            op
        } else {
            // Create new operation from update
            let op = self.create_operation_from_update(&update, now);

            // Register callback in the CallbackManager so external systems can
            // send success/failure/heartbeat responses via the callback_id.
            if op.operation_type == OperationType::Callback {
                if let Some(ref cb_details) = op.callback_details {
                    if let Some(ref callback_id) = cb_details.callback_id {
                        let timeout = update
                            .callback_options
                            .as_ref()
                            .and_then(|opts| opts.timeout_seconds)
                            .map(std::time::Duration::from_secs);
                        let _ = self
                            .callback_manager
                            .register_callback(callback_id, timeout);
                    }
                }
            }

            op
        };

        // Generate appropriate legacy history event based on action
        let event_type = match update.action {
            OperationAction::Start => EventType::OperationStarted,
            OperationAction::Succeed | OperationAction::Fail => EventType::OperationCompleted,
            OperationAction::Cancel | OperationAction::Retry => EventType::OperationCompleted,
        };

        self.event_processor.create_history_event(
            event_type,
            Some(&operation_id),
            "OperationDetails",
            serde_json::json!({
                "operation_id": operation_id,
                "action": format!("{:?}", update.action),
            }),
        );

        // Generate Node.js-compatible events using the EventProcessor's process_operation_update method
        self.event_processor
            .process_operation_update(&update, &operation);

        // Check if this completes the execution
        if operation.operation_type == OperationType::Execution
            && (operation.status == OperationStatus::Succeeded
                || operation.status == OperationStatus::Failed)
        {
            self.is_execution_completed = true;
        }

        let events = OperationEvents {
            operation,
            update: Some(update),
        };

        // Track order for new operations
        if !self.operation_data_map.contains_key(&operation_id) {
            self.operation_order.push(operation_id.clone());
        }

        self.operation_data_map.insert(operation_id.clone(), events);
        self.dirty_operation_ids.insert(operation_id);

        Ok(())
    }

    /// Merge an operation update into an existing operation.
    fn merge_operation_update(
        &self,
        operation: &mut Operation,
        update: &OperationUpdate,
        now: i64,
    ) {
        // Update status based on action
        operation.status = match update.action {
            OperationAction::Start => OperationStatus::Started,
            OperationAction::Succeed => OperationStatus::Succeeded,
            OperationAction::Fail => OperationStatus::Failed,
            OperationAction::Cancel => OperationStatus::Cancelled,
            OperationAction::Retry => OperationStatus::Pending, // Retry sets status to Pending
        };

        // Set end timestamp for completion actions
        if matches!(
            update.action,
            OperationAction::Succeed | OperationAction::Fail | OperationAction::Cancel
        ) {
            operation.end_timestamp = Some(now);
        }

        // Merge result/error
        if update.result.is_some() {
            operation.result = update.result.clone();
        }
        if update.error.is_some() {
            operation.error = update.error.clone();
        }

        // Handle step retry - update step_details with next_attempt_timestamp
        if update.action == OperationAction::Retry && update.operation_type == OperationType::Step {
            let next_attempt_timestamp = update
                .step_options
                .as_ref()
                .and_then(|opts| opts.next_attempt_delay_seconds)
                .map(|delay| now + (delay as i64 * 1000));

            let current_attempt = operation
                .step_details
                .as_ref()
                .and_then(|d| d.attempt)
                .unwrap_or(0);

            operation.step_details = Some(durable_execution_sdk::StepDetails {
                result: None,
                attempt: Some(current_attempt + 1),
                next_attempt_timestamp,
                error: update.error.clone(),
                payload: update.result.clone(), // For wait-for-condition, payload is in result field
            });
        }
    }

    /// Create a new operation from an update.
    fn create_operation_from_update(&self, update: &OperationUpdate, now: i64) -> Operation {
        let status = match update.action {
            OperationAction::Start => OperationStatus::Started,
            OperationAction::Succeed => OperationStatus::Succeeded,
            OperationAction::Fail => OperationStatus::Failed,
            OperationAction::Cancel => OperationStatus::Cancelled,
            OperationAction::Retry => OperationStatus::Pending, // Retry sets status to Pending
        };

        let end_timestamp = if matches!(
            update.action,
            OperationAction::Succeed | OperationAction::Fail | OperationAction::Cancel
        ) {
            Some(now)
        } else {
            None
        };

        // Generate callback_details for callback operations
        let callback_details = if update.operation_type == OperationType::Callback {
            // Generate a unique callback_id for the callback operation
            let callback_id = Uuid::new_v4().to_string();
            Some(durable_execution_sdk::CallbackDetails {
                callback_id: Some(callback_id),
                result: None,
                error: None,
            })
        } else {
            None
        };

        // Generate wait_details for wait operations from wait_options
        // This converts wait_seconds to scheduled_end_timestamp for time-skipping support
        let wait_details = if update.operation_type == OperationType::Wait {
            if let Some(ref wait_options) = update.wait_options {
                // Calculate scheduled_end_timestamp as now + (wait_seconds * 1000) milliseconds
                let scheduled_end_timestamp = now + (wait_options.wait_seconds as i64 * 1000);
                Some(durable_execution_sdk::WaitDetails {
                    scheduled_end_timestamp: Some(scheduled_end_timestamp),
                })
            } else {
                None
            }
        } else {
            None
        };

        // Generate step_details for step operations with retry
        // This converts next_attempt_delay_seconds to next_attempt_timestamp for time-skipping support
        let step_details = if update.operation_type == OperationType::Step {
            if let Some(ref step_options) = update.step_options {
                // Calculate next_attempt_timestamp as now + (delay_seconds * 1000) milliseconds
                let next_attempt_timestamp = step_options
                    .next_attempt_delay_seconds
                    .map(|delay| now + (delay as i64 * 1000));
                Some(durable_execution_sdk::StepDetails {
                    result: None,
                    attempt: Some(1), // Start with attempt 1
                    next_attempt_timestamp,
                    error: update.error.clone(),
                    payload: update.result.clone(), // For wait-for-condition, payload is in result field
                })
            } else if update.action == OperationAction::Retry {
                // Retry without step_options - still create step_details
                Some(durable_execution_sdk::StepDetails {
                    result: None,
                    attempt: Some(1),
                    next_attempt_timestamp: None,
                    error: update.error.clone(),
                    payload: update.result.clone(),
                })
            } else {
                None
            }
        } else {
            None
        };

        Operation {
            operation_id: update.operation_id.clone(),
            operation_type: update.operation_type,
            status,
            result: update.result.clone(),
            error: update.error.clone(),
            parent_id: update.parent_id.clone(),
            name: update.name.clone(),
            sub_type: update.sub_type.clone(),
            start_timestamp: Some(now),
            end_timestamp,
            wait_details,
            step_details,
            callback_details,
            chained_invoke_details: None,
            context_details: None,
            execution_details: None,
        }
    }

    /// Get the callback manager.
    pub fn callback_manager(&self) -> &CallbackManager {
        &self.callback_manager
    }

    /// Get mutable callback manager.
    pub fn callback_manager_mut(&mut self) -> &mut CallbackManager {
        &mut self.callback_manager
    }

    /// Complete a callback operation by updating its status and result/error.
    ///
    /// Finds the operation whose `callback_details.callback_id` matches the given
    /// `callback_id` and transitions it to `Succeeded` (with result) or `Failed`
    /// (with error). This is called when an external system sends a callback
    /// response via the `CallbackManager`.
    pub fn complete_callback_operation(
        &mut self,
        callback_id: &str,
        result: Option<String>,
        error: Option<durable_execution_sdk::ErrorObject>,
    ) {
        let now = chrono::Utc::now().timestamp_millis();
        for events in self.operation_data_map.values_mut() {
            if events.operation.operation_type == OperationType::Callback {
                if let Some(ref cb) = events.operation.callback_details {
                    if cb.callback_id.as_deref() == Some(callback_id) {
                        if error.is_some() {
                            events.operation.status = OperationStatus::Failed;
                            events.operation.error = error;
                        } else {
                            events.operation.status = OperationStatus::Succeeded;
                            events.operation.result = result;
                        }
                        events.operation.end_timestamp = Some(now);
                        // Update callback_details with the result
                        if let Some(ref mut cb_details) = events.operation.callback_details {
                            cb_details.result = events.operation.result.clone();
                            cb_details.error = events.operation.error.clone();
                        }
                        self.dirty_operation_ids
                            .insert(events.operation.operation_id.clone());
                        return;
                    }
                }
            }
        }
    }

    /// Get the event processor.
    pub fn event_processor(&self) -> &EventProcessor {
        &self.event_processor
    }

    /// Get mutable event processor.
    pub fn event_processor_mut(&mut self) -> &mut EventProcessor {
        &mut self.event_processor
    }

    /// Get all history events.
    pub fn get_history_events(&self) -> Vec<HistoryEvent> {
        self.event_processor.get_events().to_vec()
    }

    /// Get all Node.js-compatible history events.
    ///
    /// Returns a vector of events in the Node.js SDK compatible format,
    /// suitable for cross-SDK history comparison.
    pub fn get_nodejs_history_events(&self) -> Vec<NodeJsHistoryEvent> {
        self.event_processor.get_nodejs_events().to_vec()
    }

    /// Generate a terminal ExecutionSucceeded or ExecutionFailed event.
    ///
    /// This should be called when the execution completes to ensure the
    /// terminal event is recorded in the history, matching cloud behavior.
    pub fn complete_execution(
        &mut self,
        result: Option<&str>,
        error: Option<&durable_execution_sdk::ErrorObject>,
    ) {
        // Only generate if not already completed via operation updates
        if self.is_execution_completed {
            return;
        }
        self.is_execution_completed = true;

        // Find the execution operation to use as context
        let exec_op = self.operation_order.iter().find_map(|id| {
            let events = self.operation_data_map.get(id)?;
            if events.operation.operation_type == OperationType::Execution {
                Some(events.operation.clone())
            } else {
                None
            }
        });

        if let Some(mut op) = exec_op {
            if let Some(err) = error {
                op.status = OperationStatus::Failed;
                let error_payload = serde_json::to_string(err).unwrap_or_default();
                let details = NodeJsEventDetails::ExecutionFailed(
                    crate::checkpoint_server::nodejs_event_types::ExecutionFailedDetailsWrapper {
                        execution_failed_details:
                            crate::checkpoint_server::nodejs_event_types::ExecutionFailedDetails {
                                error: crate::checkpoint_server::nodejs_event_types::PayloadWrapper::new(error_payload),
                            },
                    },
                );
                self.event_processor.create_nodejs_event(
                    NodeJsEventType::ExecutionFailed,
                    Some(&op),
                    details,
                );
            } else {
                op.status = OperationStatus::Succeeded;
                let result_str = result.unwrap_or("null");
                let details = NodeJsEventDetails::ExecutionSucceeded(
                    crate::checkpoint_server::nodejs_event_types::ExecutionSucceededDetailsWrapper {
                        execution_succeeded_details:
                            crate::checkpoint_server::nodejs_event_types::ExecutionSucceededDetails {
                                result: crate::checkpoint_server::nodejs_event_types::PayloadWrapper::new(result_str),
                            },
                    },
                );
                self.event_processor.create_nodejs_event(
                    NodeJsEventType::ExecutionSucceeded,
                    Some(&op),
                    details,
                );
            }
        }
    }

    /// Get the execution ID.
    pub fn execution_id(&self) -> &str {
        &self.execution_id
    }

    /// Update operation data directly.
    ///
    /// This method is used by the orchestrator to update operation state
    /// (e.g., marking wait operations as SUCCEEDED after time advancement).
    pub fn update_operation_data(&mut self, operation_id: &str, updated_operation: Operation) {
        if let Some(existing) = self.operation_data_map.get_mut(operation_id) {
            // Update the operation
            existing.operation = updated_operation;

            // Mark as dirty so it's returned in the next get_state call
            self.dirty_operation_ids.insert(operation_id.to_string());

            // Check if this completes the execution
            if existing.operation.operation_type == OperationType::Execution
                && (existing.operation.status == OperationStatus::Succeeded
                    || existing.operation.status == OperationStatus::Failed)
            {
                self.is_execution_completed = true;
            }
        } else {
            // Operation doesn't exist, add it
            let events = OperationEvents {
                operation: updated_operation,
                update: None,
            };
            self.operation_data_map
                .insert(operation_id.to_string(), events);
            self.operation_order.push(operation_id.to_string());
            self.dirty_operation_ids.insert(operation_id.to_string());
        }
    }
}

/// Timestamps for an invocation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InvocationTimestamps {
    /// When the invocation started
    pub start_timestamp: DateTime<Utc>,
    /// When the invocation ended
    pub end_timestamp: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialize() {
        let mut manager = CheckpointManager::new("exec-1");
        let events = manager.initialize(r#"{"input": "test"}"#);

        assert!(!events.operation.operation_id.is_empty());
        assert_eq!(events.operation.operation_type, OperationType::Execution);
        assert_eq!(events.operation.status, OperationStatus::Started);
        assert!(manager.has_dirty_operations());
    }

    #[test]
    fn test_start_invocation() {
        let mut manager = CheckpointManager::new("exec-1");
        manager.initialize("{}");

        let ops = manager.start_invocation("inv-1");
        assert_eq!(ops.len(), 1);
        assert!(!manager.has_dirty_operations()); // Cleared on start
    }

    #[test]
    fn test_complete_invocation() {
        let mut manager = CheckpointManager::new("exec-1");
        manager.initialize("{}");
        manager.start_invocation("inv-1");

        let timestamps = manager.complete_invocation("inv-1").unwrap();
        assert!(timestamps.end_timestamp >= timestamps.start_timestamp);
    }

    #[test]
    fn test_complete_unknown_invocation_fails() {
        let mut manager = CheckpointManager::new("exec-1");
        let result = manager.complete_invocation("unknown");
        assert!(matches!(result, Err(TestError::InvocationNotFound(_))));
    }

    #[test]
    fn test_process_checkpoint() {
        let mut manager = CheckpointManager::new("exec-1");
        manager.initialize("{}");
        manager.get_dirty_operations(); // Clear dirty

        let update = OperationUpdate::start("op-1", OperationType::Step).with_name("test-step");

        let dirty = manager.process_checkpoint(vec![update]).unwrap();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].name, Some("test-step".to_string()));
    }

    #[test]
    fn test_execution_completed_on_success() {
        let mut manager = CheckpointManager::new("exec-1");
        let events = manager.initialize("{}");
        let exec_id = events.operation.operation_id.clone();

        assert!(!manager.is_execution_completed());

        let update = OperationUpdate {
            operation_id: exec_id,
            action: OperationAction::Succeed,
            operation_type: OperationType::Execution,
            result: Some("done".to_string()),
            error: None,
            parent_id: None,
            name: None,
            sub_type: None,
            wait_options: None,
            step_options: None,
            callback_options: None,
            chained_invoke_options: None,
            context_options: None,
        };

        manager.process_checkpoint(vec![update]).unwrap();
        assert!(manager.is_execution_completed());
    }

    #[test]
    fn test_get_state() {
        let mut manager = CheckpointManager::new("exec-1");
        manager.initialize("{}");

        let update = OperationUpdate::start("op-1", OperationType::Step);
        manager.process_checkpoint(vec![update]).unwrap();

        let state = manager.get_state();
        assert_eq!(state.len(), 2); // Initial execution + step
    }

    // ============================================================================
    // Node.js-compatible event generation tests
    // ============================================================================

    #[test]
    fn test_initialize_generates_nodejs_execution_started_event() {
        let mut manager = CheckpointManager::new("exec-1");
        let events = manager.initialize(r#"{"input": "test"}"#);

        let nodejs_events = manager.get_nodejs_history_events();
        assert_eq!(nodejs_events.len(), 1);
        assert_eq!(
            nodejs_events[0].event_type,
            NodeJsEventType::ExecutionStarted
        );
        assert_eq!(nodejs_events[0].event_id, 1);
        assert_eq!(
            nodejs_events[0].id,
            Some(events.operation.operation_id.clone())
        );
    }

    #[test]
    fn test_process_operation_update_generates_nodejs_events() {
        let mut manager = CheckpointManager::new("exec-1");
        manager.initialize("{}");

        // Process a step start
        let update = OperationUpdate::start("step-1", OperationType::Step).with_name("my-step");
        manager.process_checkpoint(vec![update]).unwrap();

        let nodejs_events = manager.get_nodejs_history_events();
        assert_eq!(nodejs_events.len(), 2); // ExecutionStarted + StepStarted
        assert_eq!(
            nodejs_events[0].event_type,
            NodeJsEventType::ExecutionStarted
        );
        assert_eq!(nodejs_events[1].event_type, NodeJsEventType::StepStarted);
        assert_eq!(nodejs_events[1].name, Some("my-step".to_string()));
    }

    #[test]
    fn test_complete_invocation_generates_nodejs_event() {
        let mut manager = CheckpointManager::new("exec-1");
        manager.initialize("{}");
        manager.start_invocation("inv-1");

        manager.complete_invocation("inv-1").unwrap();

        let nodejs_events = manager.get_nodejs_history_events();
        assert_eq!(nodejs_events.len(), 2); // ExecutionStarted + InvocationCompleted
        assert_eq!(
            nodejs_events[1].event_type,
            NodeJsEventType::InvocationCompleted
        );
    }

    #[test]
    fn test_step_lifecycle_generates_correct_nodejs_events() {
        let mut manager = CheckpointManager::new("exec-1");
        manager.initialize("{}");

        // Start step
        let start_update =
            OperationUpdate::start("step-1", OperationType::Step).with_name("compute");
        manager.process_checkpoint(vec![start_update]).unwrap();

        // Succeed step
        let succeed_update =
            OperationUpdate::succeed("step-1", OperationType::Step, Some("42".to_string()));
        manager.process_checkpoint(vec![succeed_update]).unwrap();

        let nodejs_events = manager.get_nodejs_history_events();
        assert_eq!(nodejs_events.len(), 3); // ExecutionStarted + StepStarted + StepSucceeded
        assert_eq!(
            nodejs_events[0].event_type,
            NodeJsEventType::ExecutionStarted
        );
        assert_eq!(nodejs_events[1].event_type, NodeJsEventType::StepStarted);
        assert_eq!(nodejs_events[2].event_type, NodeJsEventType::StepSucceeded);
    }

    #[test]
    fn test_execution_lifecycle_generates_correct_nodejs_events() {
        let mut manager = CheckpointManager::new("exec-1");
        let init_events = manager.initialize("{}");
        let exec_id = init_events.operation.operation_id.clone();

        // Succeed execution
        let succeed_update = OperationUpdate::succeed(
            &exec_id,
            OperationType::Execution,
            Some("result".to_string()),
        );
        manager.process_checkpoint(vec![succeed_update]).unwrap();

        let nodejs_events = manager.get_nodejs_history_events();
        assert_eq!(nodejs_events.len(), 2); // ExecutionStarted + ExecutionSucceeded
        assert_eq!(
            nodejs_events[0].event_type,
            NodeJsEventType::ExecutionStarted
        );
        assert_eq!(
            nodejs_events[1].event_type,
            NodeJsEventType::ExecutionSucceeded
        );
    }

    #[test]
    fn test_wait_lifecycle_generates_correct_nodejs_events() {
        let mut manager = CheckpointManager::new("exec-1");
        manager.initialize("{}");

        // Start wait
        let start_update = OperationUpdate::start_wait("wait-1", 30);
        manager.process_checkpoint(vec![start_update]).unwrap();

        // Succeed wait
        let succeed_update = OperationUpdate::succeed("wait-1", OperationType::Wait, None);
        manager.process_checkpoint(vec![succeed_update]).unwrap();

        let nodejs_events = manager.get_nodejs_history_events();
        assert_eq!(nodejs_events.len(), 3); // ExecutionStarted + WaitStarted + WaitSucceeded
        assert_eq!(nodejs_events[1].event_type, NodeJsEventType::WaitStarted);
        assert_eq!(nodejs_events[2].event_type, NodeJsEventType::WaitSucceeded);
    }

    #[test]
    fn test_retry_operation_generates_step_failed_event() {
        use durable_execution_sdk::error::ErrorObject;
        use durable_execution_sdk::StepOptions;

        let mut manager = CheckpointManager::new("exec-1");
        manager.initialize("{}");

        // Start step
        let start_update = OperationUpdate::start("step-1", OperationType::Step);
        manager.process_checkpoint(vec![start_update]).unwrap();

        // Retry step (generates StepFailed event with retry details)
        let retry_update = OperationUpdate {
            operation_id: "step-1".to_string(),
            action: OperationAction::Retry,
            operation_type: OperationType::Step,
            result: None,
            error: Some(ErrorObject::new("RetryError", "Temporary failure")),
            parent_id: None,
            name: None,
            sub_type: None,
            wait_options: None,
            step_options: Some(StepOptions {
                next_attempt_delay_seconds: Some(5),
            }),
            callback_options: None,
            chained_invoke_options: None,
            context_options: None,
        };
        manager.process_checkpoint(vec![retry_update]).unwrap();

        let nodejs_events = manager.get_nodejs_history_events();
        assert_eq!(nodejs_events.len(), 3); // ExecutionStarted + StepStarted + StepFailed
        assert_eq!(nodejs_events[2].event_type, NodeJsEventType::StepFailed);
    }

    #[test]
    fn test_nodejs_event_ids_are_sequential() {
        let mut manager = CheckpointManager::new("exec-1");
        manager.initialize("{}");

        // Add multiple operations
        let step1 = OperationUpdate::start("step-1", OperationType::Step);
        let step2 = OperationUpdate::start("step-2", OperationType::Step);
        manager.process_checkpoint(vec![step1, step2]).unwrap();

        let nodejs_events = manager.get_nodejs_history_events();
        assert_eq!(nodejs_events.len(), 3);
        assert_eq!(nodejs_events[0].event_id, 1);
        assert_eq!(nodejs_events[1].event_id, 2);
        assert_eq!(nodejs_events[2].event_id, 3);
    }
}
