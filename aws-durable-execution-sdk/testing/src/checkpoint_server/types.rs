//! Shared types for the checkpoint server module.
//!
//! This module defines the types used for communication between the main thread
//! and the checkpoint server thread, matching the Node.js SDK's worker API types.

use serde::{Deserialize, Serialize};

use aws_durable_execution_sdk::{ErrorObject, Operation, OperationUpdate};

/// Unique identifier for an execution.
pub type ExecutionId = String;

/// Unique identifier for an invocation within an execution.
pub type InvocationId = String;

/// Unique identifier for a checkpoint token.
pub type CheckpointToken = String;

/// API types supported by the checkpoint server.
///
/// These match the Node.js SDK's `ApiType` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ApiType {
    /// Start a new durable execution
    StartDurableExecution,
    /// Start an invocation for an existing execution
    StartInvocation,
    /// Complete an invocation
    CompleteInvocation,
    /// Update checkpoint data for an operation
    UpdateCheckpointData,
    /// Poll for checkpoint data updates
    PollCheckpointData,
    /// Get the current state of a durable execution
    GetDurableExecutionState,
    /// Checkpoint the durable execution state
    CheckpointDurableExecutionState,
    /// Send a callback success response
    SendDurableExecutionCallbackSuccess,
    /// Send a callback failure response
    SendDurableExecutionCallbackFailure,
    /// Send a callback heartbeat
    SendDurableExecutionCallbackHeartbeat,
}

/// Command types sent from the main thread to the worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkerCommandType {
    /// An API request to be processed
    ApiRequest,
}

/// Response types sent from the worker to the main thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkerResponseType {
    /// An API response
    ApiResponse,
}

/// Parameters for configuring the checkpoint worker.
#[derive(Debug, Clone, Default)]
pub struct CheckpointWorkerParams {
    /// Optional checkpoint delay in milliseconds to simulate network latency
    pub checkpoint_delay: Option<u64>,
}

// ============================================================================
// Request Types
// ============================================================================

/// Request to start a new durable execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartDurableExecutionRequest {
    /// The input payload for the execution (JSON string)
    pub payload: Option<String>,
    /// The invocation ID for this execution
    pub invocation_id: InvocationId,
}

/// Request to start an invocation for an existing execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartInvocationRequest {
    /// The execution ID
    pub execution_id: ExecutionId,
    /// The invocation ID
    pub invocation_id: InvocationId,
}

/// Request to complete an invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteInvocationRequest {
    /// The execution ID
    pub execution_id: ExecutionId,
    /// The invocation ID
    pub invocation_id: InvocationId,
    /// Optional error if the invocation failed
    pub error: Option<ErrorObject>,
}

/// Request to update checkpoint data for an operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckpointDataRequest {
    /// The execution ID
    pub execution_id: ExecutionId,
    /// The operation ID to update
    pub operation_id: String,
    /// Partial operation data to merge
    pub operation_data: Operation,
    /// Optional payload
    pub payload: Option<String>,
    /// Optional error
    pub error: Option<ErrorObject>,
}

/// Request to poll for checkpoint data updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollCheckpointDataRequest {
    /// The execution ID
    pub execution_id: ExecutionId,
}

/// Request to get the current state of a durable execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetDurableExecutionStateRequest {
    /// The execution ARN
    pub durable_execution_arn: String,
}

/// Request to checkpoint the durable execution state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointDurableExecutionStateRequest {
    /// The execution ARN
    pub durable_execution_arn: String,
    /// The checkpoint token
    pub checkpoint_token: CheckpointToken,
    /// The operation updates
    pub operations: Vec<OperationUpdate>,
}

/// Request to send a callback success response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendCallbackSuccessRequest {
    /// The callback ID
    pub callback_id: String,
    /// The success result (JSON string)
    pub result: String,
}

/// Request to send a callback failure response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendCallbackFailureRequest {
    /// The callback ID
    pub callback_id: String,
    /// The error object
    pub error: ErrorObject,
}

/// Request to send a callback heartbeat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendCallbackHeartbeatRequest {
    /// The callback ID
    pub callback_id: String,
}

// ============================================================================
// Worker API Request/Response
// ============================================================================

/// A request to the worker API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerApiRequest {
    /// The API type being called
    pub api_type: ApiType,
    /// Unique request ID for correlation
    pub request_id: String,
    /// The request payload (serialized as JSON)
    pub payload: String,
}

/// A response from the worker API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerApiResponse {
    /// The API type that was called
    pub api_type: ApiType,
    /// The request ID for correlation
    pub request_id: String,
    /// The response payload (serialized as JSON), or None if error
    pub payload: Option<String>,
    /// Error message if the request failed
    pub error: Option<String>,
}

impl WorkerApiResponse {
    /// Create a successful response.
    pub fn success(api_type: ApiType, request_id: String, payload: String) -> Self {
        Self {
            api_type,
            request_id,
            payload: Some(payload),
            error: None,
        }
    }

    /// Create an error response.
    pub fn error(api_type: ApiType, request_id: String, error: String) -> Self {
        Self {
            api_type,
            request_id,
            payload: None,
            error: Some(error),
        }
    }

    /// Check if this response is an error.
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }
}

/// A command sent from the main thread to the worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerCommand {
    /// The command type
    pub command_type: WorkerCommandType,
    /// The API request data
    pub data: WorkerApiRequest,
}

/// A response sent from the worker to the main thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerResponse {
    /// The response type
    pub response_type: WorkerResponseType,
    /// The API response data
    pub data: WorkerApiResponse,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_type_serialization() {
        let api_type = ApiType::StartDurableExecution;
        let json = serde_json::to_string(&api_type).unwrap();
        let deserialized: ApiType = serde_json::from_str(&json).unwrap();
        assert_eq!(api_type, deserialized);
    }

    #[test]
    fn test_worker_api_response_success() {
        let response =
            WorkerApiResponse::success(ApiType::StartDurableExecution, "req-1".into(), "{}".into());
        assert!(!response.is_error());
        assert_eq!(response.payload, Some("{}".into()));
        assert_eq!(response.error, None);
    }

    #[test]
    fn test_worker_api_response_error() {
        let response = WorkerApiResponse::error(
            ApiType::StartDurableExecution,
            "req-1".into(),
            "Something went wrong".into(),
        );
        assert!(response.is_error());
        assert_eq!(response.payload, None);
        assert_eq!(response.error, Some("Something went wrong".into()));
    }

    #[test]
    fn test_worker_command_serialization() {
        let command = WorkerCommand {
            command_type: WorkerCommandType::ApiRequest,
            data: WorkerApiRequest {
                api_type: ApiType::StartDurableExecution,
                request_id: "req-123".into(),
                payload: r#"{"invocation_id": "inv-1"}"#.into(),
            },
        };

        let json = serde_json::to_string(&command).unwrap();
        let deserialized: WorkerCommand = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.command_type, WorkerCommandType::ApiRequest);
        assert_eq!(
            deserialized.data.api_type,
            ApiType::StartDurableExecution
        );
        assert_eq!(deserialized.data.request_id, "req-123");
    }
}
