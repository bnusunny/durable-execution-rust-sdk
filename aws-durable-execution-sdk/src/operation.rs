//! Operation types for the AWS Durable Execution SDK.
//!
//! This module defines the core operation types used for checkpointing
//! and replay in durable execution workflows.

use serde::{Deserialize, Serialize};

use crate::error::ErrorObject;

/// Represents a checkpointed operation in a durable execution.
///
/// Operations are the fundamental unit of state in durable executions.
/// Each operation has a unique ID and tracks its type, status, and result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    /// Unique identifier for this operation
    #[serde(rename = "Id", alias = "OperationId")]
    pub operation_id: String,

    /// The type of operation (Step, Wait, Callback, etc.)
    #[serde(rename = "Type", alias = "OperationType")]
    pub operation_type: OperationType,

    /// Current status of the operation
    #[serde(rename = "Status")]
    pub status: OperationStatus,

    /// Serialized result if the operation succeeded (legacy field, prefer type-specific details)
    #[serde(rename = "Result", skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,

    /// Error details if the operation failed
    #[serde(rename = "Error", skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,

    /// Parent operation ID for nested operations
    #[serde(rename = "ParentId", skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,

    /// Optional human-readable name for the operation
    #[serde(rename = "Name", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// SDK-level categorization of the operation (e.g., "map", "parallel", "wait_for_condition")
    /// Requirements: 23.3, 23.4
    #[serde(rename = "SubType", skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<String>,

    /// Start timestamp of the operation (milliseconds since epoch)
    #[serde(rename = "StartTimestamp", skip_serializing_if = "Option::is_none")]
    pub start_timestamp: Option<i64>,

    /// End timestamp of the operation (milliseconds since epoch)
    #[serde(rename = "EndTimestamp", skip_serializing_if = "Option::is_none")]
    pub end_timestamp: Option<i64>,

    /// Execution details for EXECUTION type operations
    #[serde(rename = "ExecutionDetails", skip_serializing_if = "Option::is_none")]
    pub execution_details: Option<ExecutionDetails>,

    /// Step details for STEP type operations
    #[serde(rename = "StepDetails", skip_serializing_if = "Option::is_none")]
    pub step_details: Option<StepDetails>,

    /// Wait details for WAIT type operations
    #[serde(rename = "WaitDetails", skip_serializing_if = "Option::is_none")]
    pub wait_details: Option<WaitDetails>,

    /// Callback details for CALLBACK type operations
    #[serde(rename = "CallbackDetails", skip_serializing_if = "Option::is_none")]
    pub callback_details: Option<CallbackDetails>,

    /// Chained invoke details for CHAINED_INVOKE type operations
    #[serde(rename = "ChainedInvokeDetails", skip_serializing_if = "Option::is_none")]
    pub chained_invoke_details: Option<ChainedInvokeDetails>,

    /// Context details for CONTEXT type operations
    #[serde(rename = "ContextDetails", skip_serializing_if = "Option::is_none")]
    pub context_details: Option<ContextDetails>,
}

/// Details specific to EXECUTION type operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionDetails {
    /// The input payload for the execution
    #[serde(rename = "InputPayload", skip_serializing_if = "Option::is_none")]
    pub input_payload: Option<String>,
}

/// Details specific to STEP type operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepDetails {
    /// The result payload if the step succeeded
    #[serde(rename = "Result", skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// The current retry attempt (0-indexed)
    #[serde(rename = "Attempt", skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    /// Timestamp for the next retry attempt
    #[serde(rename = "NextAttemptTimestamp", skip_serializing_if = "Option::is_none")]
    pub next_attempt_timestamp: Option<i64>,
    /// Error details if the step failed
    #[serde(rename = "Error", skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
    /// Payload for RETRY action - stores state for wait-for-condition pattern
    /// Requirements: 4.9
    #[serde(rename = "Payload", skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
}

/// Details specific to WAIT type operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitDetails {
    /// Timestamp when the wait is scheduled to end
    #[serde(rename = "ScheduledEndTimestamp", skip_serializing_if = "Option::is_none")]
    pub scheduled_end_timestamp: Option<i64>,
}

/// Details specific to CALLBACK type operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackDetails {
    /// The callback ID for external systems to use
    #[serde(rename = "CallbackId", skip_serializing_if = "Option::is_none")]
    pub callback_id: Option<String>,
    /// The result payload if the callback succeeded
    #[serde(rename = "Result", skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// Error details if the callback failed
    #[serde(rename = "Error", skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
}

/// Details specific to CHAINED_INVOKE type operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainedInvokeDetails {
    /// The result payload if the invocation succeeded
    #[serde(rename = "Result", skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// Error details if the invocation failed
    #[serde(rename = "Error", skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
}

/// Details specific to CONTEXT type operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextDetails {
    /// The result payload if the context succeeded
    #[serde(rename = "Result", skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// Whether to replay children when loading state
    #[serde(rename = "ReplayChildren", skip_serializing_if = "Option::is_none")]
    pub replay_children: Option<bool>,
    /// Error details if the context failed
    #[serde(rename = "Error", skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
}

impl Operation {
    /// Creates a new Operation with the given ID and type.
    pub fn new(operation_id: impl Into<String>, operation_type: OperationType) -> Self {
        Self {
            operation_id: operation_id.into(),
            operation_type,
            status: OperationStatus::Started,
            result: None,
            error: None,
            parent_id: None,
            name: None,
            sub_type: None,
            start_timestamp: None,
            end_timestamp: None,
            execution_details: None,
            step_details: None,
            wait_details: None,
            callback_details: None,
            chained_invoke_details: None,
            context_details: None,
        }
    }

    /// Sets the parent ID for this operation.
    pub fn with_parent_id(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_id = Some(parent_id.into());
        self
    }

    /// Sets the name for this operation.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the sub-type for this operation.
    /// Requirements: 23.3, 23.4
    pub fn with_sub_type(mut self, sub_type: impl Into<String>) -> Self {
        self.sub_type = Some(sub_type.into());
        self
    }

    /// Returns true if the operation has completed (succeeded or failed).
    pub fn is_completed(&self) -> bool {
        matches!(
            self.status,
            OperationStatus::Succeeded
                | OperationStatus::Failed
                | OperationStatus::Cancelled
                | OperationStatus::TimedOut
                | OperationStatus::Stopped
        )
    }

    /// Returns true if the operation succeeded.
    pub fn is_succeeded(&self) -> bool {
        matches!(self.status, OperationStatus::Succeeded)
    }

    /// Returns true if the operation failed.
    pub fn is_failed(&self) -> bool {
        matches!(
            self.status,
            OperationStatus::Failed | OperationStatus::Cancelled | OperationStatus::TimedOut
        )
    }

    /// Gets the result from the appropriate details field based on operation type.
    pub fn get_result(&self) -> Option<&str> {
        // First check type-specific details
        match self.operation_type {
            OperationType::Step => {
                if let Some(ref details) = self.step_details {
                    if details.result.is_some() {
                        return details.result.as_deref();
                    }
                }
            }
            OperationType::Callback => {
                if let Some(ref details) = self.callback_details {
                    if details.result.is_some() {
                        return details.result.as_deref();
                    }
                }
            }
            OperationType::Invoke => {
                if let Some(ref details) = self.chained_invoke_details {
                    if details.result.is_some() {
                        return details.result.as_deref();
                    }
                }
            }
            OperationType::Context => {
                if let Some(ref details) = self.context_details {
                    if details.result.is_some() {
                        return details.result.as_deref();
                    }
                }
            }
            _ => {}
        }
        // Fall back to legacy result field
        self.result.as_deref()
    }

    /// Gets the retry payload from StepDetails for STEP operations.
    ///
    /// This is used for the wait-for-condition pattern where state is passed
    /// between retry attempts via the Payload field.
    ///
    /// # Returns
    ///
    /// The payload string if this is a STEP operation with a payload, None otherwise.
    ///
    /// # Requirements
    ///
    /// - 4.9: THE Step_Operation SHALL support RETRY action with Payload for wait-for-condition pattern
    pub fn get_retry_payload(&self) -> Option<&str> {
        if self.operation_type == OperationType::Step {
            if let Some(ref details) = self.step_details {
                return details.payload.as_deref();
            }
        }
        None
    }

    /// Gets the current attempt number from StepDetails for STEP operations.
    ///
    /// # Returns
    ///
    /// The attempt number (0-indexed) if this is a STEP operation with attempt tracking, None otherwise.
    ///
    /// # Requirements
    ///
    /// - 4.8: THE Step_Operation SHALL track attempt numbers in StepDetails.Attempt
    pub fn get_attempt(&self) -> Option<u32> {
        if self.operation_type == OperationType::Step {
            if let Some(ref details) = self.step_details {
                return details.attempt;
            }
        }
        None
    }
}


/// The type of operation in a durable execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationType {
    /// The root execution operation
    #[serde(rename = "EXECUTION")]
    Execution,
    /// A step operation (unit of work)
    #[serde(rename = "STEP")]
    Step,
    /// A wait/sleep operation
    #[serde(rename = "WAIT")]
    Wait,
    /// A callback operation waiting for external signal
    #[serde(rename = "CALLBACK")]
    Callback,
    /// An invoke operation calling another Lambda function
    #[serde(rename = "INVOKE")]
    Invoke,
    /// A context operation for nested child contexts
    #[serde(rename = "CONTEXT")]
    Context,
}

impl std::fmt::Display for OperationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Execution => write!(f, "Execution"),
            Self::Step => write!(f, "Step"),
            Self::Wait => write!(f, "Wait"),
            Self::Callback => write!(f, "Callback"),
            Self::Invoke => write!(f, "Invoke"),
            Self::Context => write!(f, "Context"),
        }
    }
}

/// The status of an operation in a durable execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationStatus {
    /// Operation has started but not completed
    #[serde(rename = "STARTED")]
    Started,
    /// Operation is pending (e.g., step waiting for retry)
    /// Requirements: 3.7, 4.7
    #[serde(rename = "PENDING")]
    Pending,
    /// Operation is ready to resume execution (e.g., after retry delay)
    /// Requirements: 3.7, 4.7
    #[serde(rename = "READY")]
    Ready,
    /// Operation completed successfully
    #[serde(rename = "SUCCEEDED")]
    Succeeded,
    /// Operation failed with an error
    #[serde(rename = "FAILED")]
    Failed,
    /// Operation was cancelled
    #[serde(rename = "CANCELLED")]
    Cancelled,
    /// Operation timed out
    #[serde(rename = "TIMED_OUT")]
    TimedOut,
    /// Operation was stopped externally
    #[serde(rename = "STOPPED")]
    Stopped,
}

impl OperationStatus {
    /// Returns true if this status represents a terminal state.
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::Started | Self::Pending | Self::Ready)
    }

    /// Returns true if this status represents a successful completion.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Succeeded)
    }

    /// Returns true if this status represents a failure.
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failed | Self::Cancelled | Self::TimedOut | Self::Stopped)
    }

    /// Returns true if this status indicates the operation is pending (waiting for retry).
    /// Requirements: 3.7, 4.7
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Pending)
    }

    /// Returns true if this status indicates the operation is ready to resume.
    /// Requirements: 3.7, 4.7
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }

    /// Returns true if this status indicates the operation can be resumed.
    /// This includes both PENDING and READY statuses.
    /// Requirements: 3.7
    pub fn is_resumable(&self) -> bool {
        matches!(self, Self::Started | Self::Pending | Self::Ready)
    }
}

impl std::fmt::Display for OperationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Started => write!(f, "Started"),
            Self::Pending => write!(f, "Pending"),
            Self::Ready => write!(f, "Ready"),
            Self::Succeeded => write!(f, "Succeeded"),
            Self::Failed => write!(f, "Failed"),
            Self::Cancelled => write!(f, "Cancelled"),
            Self::TimedOut => write!(f, "TimedOut"),
            Self::Stopped => write!(f, "Stopped"),
        }
    }
}

/// Action to perform on an operation during checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationAction {
    /// Start a new operation
    #[serde(rename = "START")]
    Start,
    /// Mark operation as succeeded
    #[serde(rename = "SUCCEED")]
    Succeed,
    /// Mark operation as failed
    #[serde(rename = "FAIL")]
    Fail,
    /// Cancel an operation (e.g., cancel a wait)
    /// Requirements: 5.5
    #[serde(rename = "CANCEL")]
    Cancel,
    /// Retry an operation with optional payload (state) for wait-for-condition pattern
    /// Requirements: 4.7, 4.8, 4.9
    #[serde(rename = "RETRY")]
    Retry,
}

impl std::fmt::Display for OperationAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Start => write!(f, "Start"),
            Self::Succeed => write!(f, "Succeed"),
            Self::Fail => write!(f, "Fail"),
            Self::Cancel => write!(f, "Cancel"),
            Self::Retry => write!(f, "Retry"),
        }
    }
}

/// Options for WAIT operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitOptions {
    /// Number of seconds to wait
    #[serde(rename = "WaitSeconds")]
    pub wait_seconds: u64,
}

/// Options for STEP operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepOptions {
    /// Delay in seconds before the next retry attempt
    #[serde(rename = "NextAttemptDelaySeconds", skip_serializing_if = "Option::is_none")]
    pub next_attempt_delay_seconds: Option<u64>,
}

/// Options for CALLBACK operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackOptions {
    /// Timeout in seconds for the callback
    #[serde(rename = "TimeoutSeconds", skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
    /// Heartbeat timeout in seconds
    #[serde(rename = "HeartbeatTimeoutSeconds", skip_serializing_if = "Option::is_none")]
    pub heartbeat_timeout_seconds: Option<u64>,
}

/// Options for CHAINED_INVOKE operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainedInvokeOptions {
    /// The function name or ARN to invoke
    #[serde(rename = "FunctionName")]
    pub function_name: String,
    /// Optional tenant ID for multi-tenant scenarios
    #[serde(rename = "TenantId", skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
}

/// Options for CONTEXT operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextOptions {
    /// Whether to replay children when the context is loaded
    #[serde(rename = "ReplayChildren", skip_serializing_if = "Option::is_none")]
    pub replay_children: Option<bool>,
}

/// Represents an update to be checkpointed for an operation.
///
/// This struct is used to send checkpoint requests to the Lambda service.
/// Field names match the CheckpointDurableExecution API format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationUpdate {
    /// Unique identifier for this operation
    #[serde(rename = "Id")]
    pub operation_id: String,

    /// The action to perform (Start, Succeed, Fail)
    #[serde(rename = "Action")]
    pub action: OperationAction,

    /// The type of operation
    #[serde(rename = "Type")]
    pub operation_type: OperationType,

    /// Serialized result if succeeding (called "Payload" in the API)
    #[serde(rename = "Payload", skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,

    /// Error details if failing
    #[serde(rename = "Error", skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,

    /// Parent operation ID for nested operations
    #[serde(rename = "ParentId", skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,

    /// Optional human-readable name for the operation
    #[serde(rename = "Name", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// SDK-level categorization of the operation (e.g., "map", "parallel", "wait_for_condition")
    /// Requirements: 23.3, 23.4
    #[serde(rename = "SubType", skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<String>,

    /// Options for WAIT operations
    #[serde(rename = "WaitOptions", skip_serializing_if = "Option::is_none")]
    pub wait_options: Option<WaitOptions>,

    /// Options for STEP operations
    #[serde(rename = "StepOptions", skip_serializing_if = "Option::is_none")]
    pub step_options: Option<StepOptions>,

    /// Options for CALLBACK operations
    #[serde(rename = "CallbackOptions", skip_serializing_if = "Option::is_none")]
    pub callback_options: Option<CallbackOptions>,

    /// Options for CHAINED_INVOKE operations
    #[serde(rename = "ChainedInvokeOptions", skip_serializing_if = "Option::is_none")]
    pub chained_invoke_options: Option<ChainedInvokeOptions>,

    /// Options for CONTEXT operations
    #[serde(rename = "ContextOptions", skip_serializing_if = "Option::is_none")]
    pub context_options: Option<ContextOptions>,
}

impl OperationUpdate {
    /// Creates a new OperationUpdate to start an operation.
    pub fn start(
        operation_id: impl Into<String>,
        operation_type: OperationType,
    ) -> Self {
        Self {
            operation_id: operation_id.into(),
            action: OperationAction::Start,
            operation_type,
            result: None,
            error: None,
            parent_id: None,
            name: None,
            sub_type: None,
            wait_options: None,
            step_options: None,
            callback_options: None,
            chained_invoke_options: None,
            context_options: None,
        }
    }

    /// Creates a new OperationUpdate to start a WAIT operation with the required WaitOptions.
    pub fn start_wait(
        operation_id: impl Into<String>,
        wait_seconds: u64,
    ) -> Self {
        Self {
            operation_id: operation_id.into(),
            action: OperationAction::Start,
            operation_type: OperationType::Wait,
            result: None,
            error: None,
            parent_id: None,
            name: None,
            sub_type: None,
            wait_options: Some(WaitOptions { wait_seconds }),
            step_options: None,
            callback_options: None,
            chained_invoke_options: None,
            context_options: None,
        }
    }

    /// Creates a new OperationUpdate to mark an operation as succeeded.
    pub fn succeed(
        operation_id: impl Into<String>,
        operation_type: OperationType,
        result: Option<String>,
    ) -> Self {
        Self {
            operation_id: operation_id.into(),
            action: OperationAction::Succeed,
            operation_type,
            result,
            error: None,
            parent_id: None,
            name: None,
            sub_type: None,
            wait_options: None,
            step_options: None,
            callback_options: None,
            chained_invoke_options: None,
            context_options: None,
        }
    }

    /// Creates a new OperationUpdate to mark an operation as failed.
    pub fn fail(
        operation_id: impl Into<String>,
        operation_type: OperationType,
        error: ErrorObject,
    ) -> Self {
        Self {
            operation_id: operation_id.into(),
            action: OperationAction::Fail,
            operation_type,
            result: None,
            error: Some(error),
            parent_id: None,
            name: None,
            sub_type: None,
            wait_options: None,
            step_options: None,
            callback_options: None,
            chained_invoke_options: None,
            context_options: None,
        }
    }

    /// Creates a new OperationUpdate to cancel an operation.
    ///
    /// This is primarily used for cancelling WAIT operations.
    ///
    /// # Arguments
    ///
    /// * `operation_id` - The ID of the operation to cancel
    /// * `operation_type` - The type of operation being cancelled
    ///
    /// # Requirements
    ///
    /// - 5.5: THE Wait_Operation SHALL support cancellation of active waits via CANCEL action
    pub fn cancel(
        operation_id: impl Into<String>,
        operation_type: OperationType,
    ) -> Self {
        Self {
            operation_id: operation_id.into(),
            action: OperationAction::Cancel,
            operation_type,
            result: None,
            error: None,
            parent_id: None,
            name: None,
            sub_type: None,
            wait_options: None,
            step_options: None,
            callback_options: None,
            chained_invoke_options: None,
            context_options: None,
        }
    }

    /// Creates a new OperationUpdate to retry an operation with optional payload.
    ///
    /// This is used for the wait-for-condition pattern where state needs to be
    /// passed between retry attempts. The payload contains the state to preserve
    /// across retries, not an error.
    ///
    /// # Arguments
    ///
    /// * `operation_id` - The ID of the operation to retry
    /// * `operation_type` - The type of operation being retried
    /// * `payload` - Optional state payload to preserve across retries
    /// * `next_attempt_delay_seconds` - Optional delay before the next retry attempt
    ///
    /// # Requirements
    ///
    /// - 4.7: THE Step_Operation SHALL support RETRY action with NextAttemptDelaySeconds for backoff
    /// - 4.8: THE Step_Operation SHALL track attempt numbers in StepDetails.Attempt
    /// - 4.9: THE Step_Operation SHALL support RETRY action with Payload (not just Error) for wait-for-condition pattern
    pub fn retry(
        operation_id: impl Into<String>,
        operation_type: OperationType,
        payload: Option<String>,
        next_attempt_delay_seconds: Option<u64>,
    ) -> Self {
        Self {
            operation_id: operation_id.into(),
            action: OperationAction::Retry,
            operation_type,
            result: payload,
            error: None,
            parent_id: None,
            name: None,
            sub_type: None,
            wait_options: None,
            step_options: Some(StepOptions { next_attempt_delay_seconds }),
            callback_options: None,
            chained_invoke_options: None,
            context_options: None,
        }
    }

    /// Creates a new OperationUpdate to retry an operation with an error.
    ///
    /// This is used for traditional retry scenarios where the operation failed
    /// and needs to be retried after a delay.
    ///
    /// # Arguments
    ///
    /// * `operation_id` - The ID of the operation to retry
    /// * `operation_type` - The type of operation being retried
    /// * `error` - The error that caused the retry
    /// * `next_attempt_delay_seconds` - Optional delay before the next retry attempt
    ///
    /// # Requirements
    ///
    /// - 4.7: THE Step_Operation SHALL support RETRY action with NextAttemptDelaySeconds for backoff
    pub fn retry_with_error(
        operation_id: impl Into<String>,
        operation_type: OperationType,
        error: ErrorObject,
        next_attempt_delay_seconds: Option<u64>,
    ) -> Self {
        Self {
            operation_id: operation_id.into(),
            action: OperationAction::Retry,
            operation_type,
            result: None,
            error: Some(error),
            parent_id: None,
            name: None,
            sub_type: None,
            wait_options: None,
            step_options: Some(StepOptions { next_attempt_delay_seconds }),
            callback_options: None,
            chained_invoke_options: None,
            context_options: None,
        }
    }

    /// Sets the parent ID for this operation update.
    pub fn with_parent_id(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_id = Some(parent_id.into());
        self
    }

    /// Sets the name for this operation update.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the sub-type for this operation update.
    /// Requirements: 23.3, 23.4
    pub fn with_sub_type(mut self, sub_type: impl Into<String>) -> Self {
        self.sub_type = Some(sub_type.into());
        self
    }

    /// Sets the wait options for this operation update.
    pub fn with_wait_options(mut self, wait_seconds: u64) -> Self {
        self.wait_options = Some(WaitOptions { wait_seconds });
        self
    }

    /// Sets the step options for this operation update.
    pub fn with_step_options(mut self, next_attempt_delay_seconds: Option<u64>) -> Self {
        self.step_options = Some(StepOptions { next_attempt_delay_seconds });
        self
    }

    /// Sets the callback options for this operation update.
    pub fn with_callback_options(mut self, timeout_seconds: Option<u64>, heartbeat_timeout_seconds: Option<u64>) -> Self {
        self.callback_options = Some(CallbackOptions { timeout_seconds, heartbeat_timeout_seconds });
        self
    }

    /// Sets the chained invoke options for this operation update.
    pub fn with_chained_invoke_options(mut self, function_name: impl Into<String>, tenant_id: Option<String>) -> Self {
        self.chained_invoke_options = Some(ChainedInvokeOptions { 
            function_name: function_name.into(), 
            tenant_id 
        });
        self
    }

    /// Sets the context options for this operation update.
    pub fn with_context_options(mut self, replay_children: Option<bool>) -> Self {
        self.context_options = Some(ContextOptions { replay_children });
        self
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_new() {
        let op = Operation::new("op-123", OperationType::Step);
        assert_eq!(op.operation_id, "op-123");
        assert_eq!(op.operation_type, OperationType::Step);
        assert_eq!(op.status, OperationStatus::Started);
        assert!(op.result.is_none());
        assert!(op.error.is_none());
        assert!(op.parent_id.is_none());
        assert!(op.name.is_none());
    }

    #[test]
    fn test_operation_with_parent_and_name() {
        let op = Operation::new("op-123", OperationType::Step)
            .with_parent_id("parent-456")
            .with_name("my-step");
        assert_eq!(op.parent_id, Some("parent-456".to_string()));
        assert_eq!(op.name, Some("my-step".to_string()));
    }

    #[test]
    fn test_operation_is_completed() {
        let mut op = Operation::new("op-123", OperationType::Step);
        assert!(!op.is_completed());

        op.status = OperationStatus::Succeeded;
        assert!(op.is_completed());

        op.status = OperationStatus::Failed;
        assert!(op.is_completed());

        op.status = OperationStatus::Cancelled;
        assert!(op.is_completed());

        op.status = OperationStatus::TimedOut;
        assert!(op.is_completed());

        op.status = OperationStatus::Stopped;
        assert!(op.is_completed());
    }

    #[test]
    fn test_operation_is_succeeded() {
        let mut op = Operation::new("op-123", OperationType::Step);
        assert!(!op.is_succeeded());

        op.status = OperationStatus::Succeeded;
        assert!(op.is_succeeded());

        op.status = OperationStatus::Failed;
        assert!(!op.is_succeeded());
    }

    #[test]
    fn test_operation_is_failed() {
        let mut op = Operation::new("op-123", OperationType::Step);
        assert!(!op.is_failed());

        op.status = OperationStatus::Failed;
        assert!(op.is_failed());

        op.status = OperationStatus::Cancelled;
        assert!(op.is_failed());

        op.status = OperationStatus::TimedOut;
        assert!(op.is_failed());

        op.status = OperationStatus::Succeeded;
        assert!(!op.is_failed());
    }

    #[test]
    fn test_operation_type_display() {
        assert_eq!(OperationType::Execution.to_string(), "Execution");
        assert_eq!(OperationType::Step.to_string(), "Step");
        assert_eq!(OperationType::Wait.to_string(), "Wait");
        assert_eq!(OperationType::Callback.to_string(), "Callback");
        assert_eq!(OperationType::Invoke.to_string(), "Invoke");
        assert_eq!(OperationType::Context.to_string(), "Context");
    }

    #[test]
    fn test_operation_status_is_terminal() {
        assert!(!OperationStatus::Started.is_terminal());
        assert!(!OperationStatus::Pending.is_terminal());
        assert!(!OperationStatus::Ready.is_terminal());
        assert!(OperationStatus::Succeeded.is_terminal());
        assert!(OperationStatus::Failed.is_terminal());
        assert!(OperationStatus::Cancelled.is_terminal());
        assert!(OperationStatus::TimedOut.is_terminal());
        assert!(OperationStatus::Stopped.is_terminal());
    }

    #[test]
    fn test_operation_status_is_success() {
        assert!(!OperationStatus::Started.is_success());
        assert!(!OperationStatus::Pending.is_success());
        assert!(!OperationStatus::Ready.is_success());
        assert!(OperationStatus::Succeeded.is_success());
        assert!(!OperationStatus::Failed.is_success());
    }

    #[test]
    fn test_operation_status_is_failure() {
        assert!(!OperationStatus::Started.is_failure());
        assert!(!OperationStatus::Pending.is_failure());
        assert!(!OperationStatus::Ready.is_failure());
        assert!(!OperationStatus::Succeeded.is_failure());
        assert!(OperationStatus::Failed.is_failure());
        assert!(OperationStatus::Cancelled.is_failure());
        assert!(OperationStatus::TimedOut.is_failure());
        assert!(OperationStatus::Stopped.is_failure());
    }

    #[test]
    fn test_operation_status_is_pending() {
        assert!(!OperationStatus::Started.is_pending());
        assert!(OperationStatus::Pending.is_pending());
        assert!(!OperationStatus::Ready.is_pending());
        assert!(!OperationStatus::Succeeded.is_pending());
        assert!(!OperationStatus::Failed.is_pending());
    }

    #[test]
    fn test_operation_status_is_ready() {
        assert!(!OperationStatus::Started.is_ready());
        assert!(!OperationStatus::Pending.is_ready());
        assert!(OperationStatus::Ready.is_ready());
        assert!(!OperationStatus::Succeeded.is_ready());
        assert!(!OperationStatus::Failed.is_ready());
    }

    #[test]
    fn test_operation_status_is_resumable() {
        assert!(OperationStatus::Started.is_resumable());
        assert!(OperationStatus::Pending.is_resumable());
        assert!(OperationStatus::Ready.is_resumable());
        assert!(!OperationStatus::Succeeded.is_resumable());
        assert!(!OperationStatus::Failed.is_resumable());
        assert!(!OperationStatus::Cancelled.is_resumable());
        assert!(!OperationStatus::TimedOut.is_resumable());
        assert!(!OperationStatus::Stopped.is_resumable());
    }

    #[test]
    fn test_operation_update_start() {
        let update = OperationUpdate::start("op-123", OperationType::Step);
        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.action, OperationAction::Start);
        assert_eq!(update.operation_type, OperationType::Step);
        assert!(update.result.is_none());
        assert!(update.error.is_none());
    }

    #[test]
    fn test_operation_update_succeed() {
        let update = OperationUpdate::succeed(
            "op-123",
            OperationType::Step,
            Some(r#"{"value": 42}"#.to_string()),
        );
        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.action, OperationAction::Succeed);
        assert_eq!(update.result, Some(r#"{"value": 42}"#.to_string()));
        assert!(update.error.is_none());
    }

    #[test]
    fn test_operation_update_fail() {
        let error = ErrorObject::new("TestError", "Something went wrong");
        let update = OperationUpdate::fail("op-123", OperationType::Step, error);
        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.action, OperationAction::Fail);
        assert!(update.result.is_none());
        assert!(update.error.is_some());
        assert_eq!(update.error.as_ref().unwrap().error_type, "TestError");
    }

    #[test]
    fn test_operation_update_with_parent_and_name() {
        let update = OperationUpdate::start("op-123", OperationType::Step)
            .with_parent_id("parent-456")
            .with_name("my-step");
        assert_eq!(update.parent_id, Some("parent-456".to_string()));
        assert_eq!(update.name, Some("my-step".to_string()));
    }

    #[test]
    fn test_operation_serialization() {
        let op = Operation::new("op-123", OperationType::Step)
            .with_parent_id("parent-456")
            .with_name("my-step");
        
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("\"Id\":\"op-123\""));
        assert!(json.contains("\"Type\":\"STEP\""));
        assert!(json.contains("\"Status\":\"STARTED\""));
        assert!(json.contains("\"ParentId\":\"parent-456\""));
        assert!(json.contains("\"Name\":\"my-step\""));
    }

    #[test]
    fn test_operation_deserialization() {
        // Test with new API field names (Id, Type)
        let json = r#"{
            "Id": "op-123",
            "Type": "STEP",
            "Status": "SUCCEEDED",
            "Result": "{\"value\": 42}",
            "ParentId": "parent-456",
            "Name": "my-step"
        }"#;
        
        let op: Operation = serde_json::from_str(json).unwrap();
        assert_eq!(op.operation_id, "op-123");
        assert_eq!(op.operation_type, OperationType::Step);
        assert_eq!(op.status, OperationStatus::Succeeded);
        assert_eq!(op.result, Some(r#"{"value": 42}"#.to_string()));
        assert_eq!(op.parent_id, Some("parent-456".to_string()));
        assert_eq!(op.name, Some("my-step".to_string()));
    }

    #[test]
    fn test_operation_deserialization_legacy_field_names() {
        // Test with legacy field names (OperationId, OperationType) for backward compatibility
        let json = r#"{
            "OperationId": "op-123",
            "OperationType": "STEP",
            "Status": "SUCCEEDED",
            "Result": "{\"value\": 42}",
            "ParentId": "parent-456",
            "Name": "my-step"
        }"#;
        
        let op: Operation = serde_json::from_str(json).unwrap();
        assert_eq!(op.operation_id, "op-123");
        assert_eq!(op.operation_type, OperationType::Step);
        assert_eq!(op.status, OperationStatus::Succeeded);
    }

    #[test]
    fn test_operation_deserialization_with_timestamps() {
        // Test with timestamps and execution details (as sent by the API)
        let json = r#"{
            "Id": "778f03ea-ab5a-3e77-8d6d-9119253f8565",
            "Name": "21e26aa2-4866-4c09-958a-15a272f16c87",
            "Type": "EXECUTION",
            "StartTimestamp": 1767896523358,
            "Status": "STARTED",
            "ExecutionDetails": {
                "InputPayload": "{\"order_id\":\"order-122342134\"}"
            }
        }"#;
        
        let op: Operation = serde_json::from_str(json).unwrap();
        assert_eq!(op.operation_id, "778f03ea-ab5a-3e77-8d6d-9119253f8565");
        assert_eq!(op.operation_type, OperationType::Execution);
        assert_eq!(op.status, OperationStatus::Started);
        assert_eq!(op.start_timestamp, Some(1767896523358));
        assert!(op.execution_details.is_some());
        let details = op.execution_details.unwrap();
        assert!(details.input_payload.is_some());
    }

    #[test]
    fn test_operation_update_serialization() {
        let update = OperationUpdate::succeed(
            "op-123",
            OperationType::Step,
            Some(r#"{"value": 42}"#.to_string()),
        ).with_parent_id("parent-456");
        
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("\"Id\":\"op-123\""));
        assert!(json.contains("\"Action\":\"SUCCEED\""));
        assert!(json.contains("\"Type\":\"STEP\""));
        assert!(json.contains("\"Payload\":\"{\\\"value\\\": 42}\""));
        assert!(json.contains("\"ParentId\":\"parent-456\""));
    }

    #[test]
    fn test_operation_status_pending_serialization() {
        // Test PENDING status serialization/deserialization
        let json = r#"{
            "Id": "op-123",
            "Type": "STEP",
            "Status": "PENDING"
        }"#;
        
        let op: Operation = serde_json::from_str(json).unwrap();
        assert_eq!(op.status, OperationStatus::Pending);
        assert!(op.status.is_pending());
        assert!(!op.status.is_terminal());
        assert!(op.status.is_resumable());
    }

    #[test]
    fn test_operation_status_ready_serialization() {
        // Test READY status serialization/deserialization
        let json = r#"{
            "Id": "op-123",
            "Type": "STEP",
            "Status": "READY"
        }"#;
        
        let op: Operation = serde_json::from_str(json).unwrap();
        assert_eq!(op.status, OperationStatus::Ready);
        assert!(op.status.is_ready());
        assert!(!op.status.is_terminal());
        assert!(op.status.is_resumable());
    }

    #[test]
    fn test_operation_status_display() {
        assert_eq!(OperationStatus::Started.to_string(), "Started");
        assert_eq!(OperationStatus::Pending.to_string(), "Pending");
        assert_eq!(OperationStatus::Ready.to_string(), "Ready");
        assert_eq!(OperationStatus::Succeeded.to_string(), "Succeeded");
        assert_eq!(OperationStatus::Failed.to_string(), "Failed");
        assert_eq!(OperationStatus::Cancelled.to_string(), "Cancelled");
        assert_eq!(OperationStatus::TimedOut.to_string(), "TimedOut");
        assert_eq!(OperationStatus::Stopped.to_string(), "Stopped");
    }

    #[test]
    fn test_operation_with_sub_type() {
        let op = Operation::new("op-123", OperationType::Context)
            .with_sub_type("map");
        assert_eq!(op.sub_type, Some("map".to_string()));
    }

    #[test]
    fn test_operation_update_with_sub_type() {
        let update = OperationUpdate::start("op-123", OperationType::Context)
            .with_sub_type("parallel");
        assert_eq!(update.sub_type, Some("parallel".to_string()));
    }

    #[test]
    fn test_operation_sub_type_serialization() {
        let op = Operation::new("op-123", OperationType::Context)
            .with_sub_type("wait_for_condition");
        
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("\"SubType\":\"wait_for_condition\""));
    }

    #[test]
    fn test_operation_sub_type_deserialization() {
        let json = r#"{
            "Id": "op-123",
            "Type": "CONTEXT",
            "Status": "STARTED",
            "SubType": "map"
        }"#;
        
        let op: Operation = serde_json::from_str(json).unwrap();
        assert_eq!(op.sub_type, Some("map".to_string()));
    }

    #[test]
    fn test_operation_metadata_fields() {
        // Test that start_timestamp and end_timestamp are properly deserialized
        let json = r#"{
            "Id": "op-123",
            "Type": "STEP",
            "Status": "SUCCEEDED",
            "StartTimestamp": 1704067200000,
            "EndTimestamp": 1704067260000,
            "Name": "my-step",
            "SubType": "custom"
        }"#;
        
        let op: Operation = serde_json::from_str(json).unwrap();
        assert_eq!(op.start_timestamp, Some(1704067200000));
        assert_eq!(op.end_timestamp, Some(1704067260000));
        assert_eq!(op.name, Some("my-step".to_string()));
        assert_eq!(op.sub_type, Some("custom".to_string()));
    }

    #[test]
    fn test_operation_action_retry_display() {
        assert_eq!(OperationAction::Retry.to_string(), "Retry");
    }

    #[test]
    fn test_operation_update_retry_with_payload() {
        let update = OperationUpdate::retry(
            "op-123",
            OperationType::Step,
            Some(r#"{"state": "waiting"}"#.to_string()),
            Some(5),
        );
        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.action, OperationAction::Retry);
        assert_eq!(update.operation_type, OperationType::Step);
        assert_eq!(update.result, Some(r#"{"state": "waiting"}"#.to_string()));
        assert!(update.error.is_none());
        assert!(update.step_options.is_some());
        assert_eq!(update.step_options.as_ref().unwrap().next_attempt_delay_seconds, Some(5));
    }

    #[test]
    fn test_operation_update_retry_with_error() {
        let error = ErrorObject::new("RetryableError", "Temporary failure");
        let update = OperationUpdate::retry_with_error(
            "op-123",
            OperationType::Step,
            error,
            Some(10),
        );
        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.action, OperationAction::Retry);
        assert!(update.result.is_none());
        assert!(update.error.is_some());
        assert_eq!(update.error.as_ref().unwrap().error_type, "RetryableError");
        assert_eq!(update.step_options.as_ref().unwrap().next_attempt_delay_seconds, Some(10));
    }

    #[test]
    fn test_operation_update_retry_serialization() {
        let update = OperationUpdate::retry(
            "op-123",
            OperationType::Step,
            Some(r#"{"counter": 5}"#.to_string()),
            Some(3),
        );
        
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("\"Action\":\"RETRY\""));
        assert!(json.contains("\"Payload\":\"{\\\"counter\\\": 5}\""));
        assert!(json.contains("\"NextAttemptDelaySeconds\":3"));
    }

    #[test]
    fn test_step_details_with_payload() {
        let json = r#"{
            "Id": "op-123",
            "Type": "STEP",
            "Status": "PENDING",
            "StepDetails": {
                "Attempt": 2,
                "Payload": "{\"state\": \"processing\"}"
            }
        }"#;
        
        let op: Operation = serde_json::from_str(json).unwrap();
        assert_eq!(op.status, OperationStatus::Pending);
        assert!(op.step_details.is_some());
        let details = op.step_details.as_ref().unwrap();
        assert_eq!(details.attempt, Some(2));
        assert_eq!(details.payload, Some(r#"{"state": "processing"}"#.to_string()));
    }

    #[test]
    fn test_operation_get_retry_payload() {
        let mut op = Operation::new("op-123", OperationType::Step);
        op.step_details = Some(StepDetails {
            result: None,
            attempt: Some(1),
            next_attempt_timestamp: None,
            error: None,
            payload: Some(r#"{"counter": 3}"#.to_string()),
        });
        
        assert_eq!(op.get_retry_payload(), Some(r#"{"counter": 3}"#));
    }

    #[test]
    fn test_operation_get_attempt() {
        let mut op = Operation::new("op-123", OperationType::Step);
        op.step_details = Some(StepDetails {
            result: None,
            attempt: Some(5),
            next_attempt_timestamp: None,
            error: None,
            payload: None,
        });
        
        assert_eq!(op.get_attempt(), Some(5));
    }

    #[test]
    fn test_operation_get_attempt_no_details() {
        let op = Operation::new("op-123", OperationType::Step);
        assert_eq!(op.get_attempt(), None);
    }

    #[test]
    fn test_operation_get_retry_payload_wrong_type() {
        let op = Operation::new("op-123", OperationType::Wait);
        assert_eq!(op.get_retry_payload(), None);
    }
}
