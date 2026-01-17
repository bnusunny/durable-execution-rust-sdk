//! Execution manager for managing the state of all executions.
//!
//! This module implements the ExecutionManager which manages the state of all
//! executions, with each execution having its own CheckpointManager.

use std::collections::HashMap;

use uuid::Uuid;

use super::checkpoint_manager::{CheckpointManager, OperationEvents};
use super::checkpoint_token::{decode_checkpoint_token, encode_checkpoint_token, CheckpointTokenData};
use super::event_processor::{EventType, HistoryEvent};
use super::types::{
    CompleteInvocationRequest, ExecutionId, InvocationId, StartDurableExecutionRequest,
    StartInvocationRequest,
};
use crate::error::TestError;

/// Result of starting an invocation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InvocationResult {
    /// The checkpoint token for this invocation
    pub checkpoint_token: String,
    /// The execution ID
    pub execution_id: ExecutionId,
    /// The invocation ID
    pub invocation_id: InvocationId,
    /// The operation events for this execution
    pub operation_events: Vec<OperationEvents>,
}

/// Response from completing an invocation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompleteInvocationResponse {
    /// The history event for the invocation completion
    pub event: HistoryEvent,
    /// Whether there are dirty operations
    pub has_dirty_operations: bool,
}


/// Parameters for starting an execution.
#[derive(Debug, Clone)]
pub struct StartExecutionParams {
    /// The execution ID
    pub execution_id: ExecutionId,
    /// The invocation ID
    pub invocation_id: InvocationId,
    /// Optional payload
    pub payload: Option<String>,
}

/// Manages all execution states.
#[derive(Debug, Default)]
pub struct ExecutionManager {
    /// Map of execution ID to checkpoint manager
    executions: HashMap<ExecutionId, CheckpointManager>,
}

impl ExecutionManager {
    /// Create a new execution manager.
    pub fn new() -> Self {
        Self {
            executions: HashMap::new(),
        }
    }

    /// Start a new execution.
    pub fn start_execution(&mut self, params: StartExecutionParams) -> InvocationResult {
        let execution_id = params.execution_id;
        let invocation_id = params.invocation_id;
        let payload = params.payload.unwrap_or_else(|| "{}".to_string());

        let mut checkpoint_manager = CheckpointManager::new(&execution_id);

        // Initialize with the first operation
        let initial_operation = checkpoint_manager.initialize(&payload);

        // Start the invocation
        checkpoint_manager.start_invocation(&invocation_id);

        // Generate checkpoint token
        let checkpoint_token = encode_checkpoint_token(&CheckpointTokenData {
            execution_id: execution_id.clone(),
            token: Uuid::new_v4().to_string(),
            invocation_id: invocation_id.clone(),
        });

        self.executions.insert(execution_id.clone(), checkpoint_manager);

        InvocationResult {
            checkpoint_token,
            execution_id,
            invocation_id,
            operation_events: vec![initial_operation],
        }
    }


    /// Start a new execution from a request.
    pub fn start_execution_from_request(
        &mut self,
        request: StartDurableExecutionRequest,
    ) -> InvocationResult {
        let execution_id = Uuid::new_v4().to_string();
        self.start_execution(StartExecutionParams {
            execution_id,
            invocation_id: request.invocation_id,
            payload: request.payload,
        })
    }

    /// Start an invocation for an existing execution.
    pub fn start_invocation(
        &mut self,
        params: StartInvocationRequest,
    ) -> Result<InvocationResult, TestError> {
        let checkpoint_manager = self
            .executions
            .get_mut(&params.execution_id)
            .ok_or_else(|| TestError::ExecutionNotFound(params.execution_id.clone()))?;

        if checkpoint_manager.is_execution_completed() {
            return Err(TestError::ExecutionNotFound(format!(
                "Execution {} is already completed",
                params.execution_id
            )));
        }

        let operation_events = checkpoint_manager.start_invocation(&params.invocation_id);

        let checkpoint_token = encode_checkpoint_token(&CheckpointTokenData {
            execution_id: params.execution_id.clone(),
            token: Uuid::new_v4().to_string(),
            invocation_id: params.invocation_id.clone(),
        });

        Ok(InvocationResult {
            checkpoint_token,
            execution_id: params.execution_id,
            invocation_id: params.invocation_id,
            operation_events,
        })
    }

    /// Complete an invocation.
    pub fn complete_invocation(
        &mut self,
        request: CompleteInvocationRequest,
    ) -> Result<CompleteInvocationResponse, TestError> {
        let checkpoint_manager = self
            .executions
            .get_mut(&request.execution_id)
            .ok_or_else(|| TestError::ExecutionNotFound(request.execution_id.clone()))?;

        let timestamps = checkpoint_manager.complete_invocation(&request.invocation_id)?;

        // Create the completion event
        let event = HistoryEvent {
            id: checkpoint_manager.event_processor().event_count() + 1,
            event_type: EventType::InvocationCompleted,
            timestamp: timestamps.end_timestamp,
            operation_id: None,
            details_type: "InvocationCompletedDetails".to_string(),
            details: serde_json::json!({
                "start_timestamp": timestamps.start_timestamp,
                "end_timestamp": timestamps.end_timestamp,
                "error": request.error,
                "request_id": request.invocation_id,
            }),
        };

        Ok(CompleteInvocationResponse {
            event,
            has_dirty_operations: checkpoint_manager.has_dirty_operations(),
        })
    }


    /// Get checkpoint manager for an execution.
    pub fn get_checkpoints_by_execution(
        &self,
        execution_id: &str,
    ) -> Option<&CheckpointManager> {
        self.executions.get(execution_id)
    }

    /// Get mutable checkpoint manager for an execution.
    pub fn get_checkpoints_by_execution_mut(
        &mut self,
        execution_id: &str,
    ) -> Option<&mut CheckpointManager> {
        self.executions.get_mut(execution_id)
    }

    /// Get checkpoint manager by checkpoint token.
    pub fn get_checkpoints_by_token(
        &self,
        token: &str,
    ) -> Result<Option<&CheckpointManager>, TestError> {
        let token_data = decode_checkpoint_token(token)?;
        Ok(self.executions.get(&token_data.execution_id))
    }

    /// Get mutable checkpoint manager by checkpoint token.
    pub fn get_checkpoints_by_token_mut(
        &mut self,
        token: &str,
    ) -> Result<Option<&mut CheckpointManager>, TestError> {
        let token_data = decode_checkpoint_token(token)?;
        Ok(self.executions.get_mut(&token_data.execution_id))
    }

    /// Get all execution IDs.
    pub fn get_execution_ids(&self) -> Vec<&ExecutionId> {
        self.executions.keys().collect()
    }

    /// Check if an execution exists.
    pub fn has_execution(&self, execution_id: &str) -> bool {
        self.executions.contains_key(execution_id)
    }

    /// Remove an execution.
    pub fn remove_execution(&mut self, execution_id: &str) -> Option<CheckpointManager> {
        self.executions.remove(execution_id)
    }

    /// Clear all executions.
    pub fn clear(&mut self) {
        self.executions.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_start_execution() {
        let mut manager = ExecutionManager::new();

        let result = manager.start_execution(StartExecutionParams {
            execution_id: "exec-1".to_string(),
            invocation_id: "inv-1".to_string(),
            payload: Some(r#"{"test": true}"#.to_string()),
        });

        assert_eq!(result.execution_id, "exec-1");
        assert_eq!(result.invocation_id, "inv-1");
        assert!(!result.checkpoint_token.is_empty());
        assert_eq!(result.operation_events.len(), 1);
    }

    #[test]
    fn test_start_execution_from_request() {
        let mut manager = ExecutionManager::new();

        let result = manager.start_execution_from_request(StartDurableExecutionRequest {
            invocation_id: "inv-1".to_string(),
            payload: Some("{}".to_string()),
        });

        assert!(!result.execution_id.is_empty());
        assert_eq!(result.invocation_id, "inv-1");
    }

    #[test]
    fn test_start_invocation() {
        let mut manager = ExecutionManager::new();

        manager.start_execution(StartExecutionParams {
            execution_id: "exec-1".to_string(),
            invocation_id: "inv-1".to_string(),
            payload: None,
        });

        // Complete first invocation
        manager
            .complete_invocation(CompleteInvocationRequest {
                execution_id: "exec-1".to_string(),
                invocation_id: "inv-1".to_string(),
                error: None,
            })
            .unwrap();

        // Start second invocation
        let result = manager
            .start_invocation(StartInvocationRequest {
                execution_id: "exec-1".to_string(),
                invocation_id: "inv-2".to_string(),
            })
            .unwrap();

        assert_eq!(result.execution_id, "exec-1");
        assert_eq!(result.invocation_id, "inv-2");
    }

    #[test]
    fn test_start_invocation_unknown_execution() {
        let mut manager = ExecutionManager::new();

        let result = manager.start_invocation(StartInvocationRequest {
            execution_id: "unknown".to_string(),
            invocation_id: "inv-1".to_string(),
        });

        assert!(matches!(result, Err(TestError::ExecutionNotFound(_))));
    }

    #[test]
    fn test_complete_invocation() {
        let mut manager = ExecutionManager::new();

        manager.start_execution(StartExecutionParams {
            execution_id: "exec-1".to_string(),
            invocation_id: "inv-1".to_string(),
            payload: None,
        });

        let response = manager
            .complete_invocation(CompleteInvocationRequest {
                execution_id: "exec-1".to_string(),
                invocation_id: "inv-1".to_string(),
                error: None,
            })
            .unwrap();

        assert_eq!(response.event.event_type, EventType::InvocationCompleted);
    }

    #[test]
    fn test_get_checkpoints_by_token() {
        let mut manager = ExecutionManager::new();

        let result = manager.start_execution(StartExecutionParams {
            execution_id: "exec-1".to_string(),
            invocation_id: "inv-1".to_string(),
            payload: None,
        });

        let checkpoint_manager = manager
            .get_checkpoints_by_token(&result.checkpoint_token)
            .unwrap()
            .unwrap();

        assert_eq!(checkpoint_manager.execution_id(), "exec-1");
    }
}
