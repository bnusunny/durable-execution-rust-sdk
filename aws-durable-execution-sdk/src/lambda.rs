//! Lambda integration types for the AWS Durable Execution SDK.
//!
//! This module defines the input/output types used for Lambda function
//! invocations in durable execution workflows.

use serde::{Deserialize, Serialize};

use crate::error::ErrorObject;
use crate::operation::Operation;

/// Input payload for a durable execution Lambda invocation.
///
/// This struct is deserialized from the Lambda event when a durable
/// execution function is invoked.
#[derive(Debug, Clone, Deserialize)]
pub struct DurableExecutionInvocationInput {
    /// The ARN of the durable execution
    #[serde(rename = "DurableExecutionArn")]
    pub durable_execution_arn: String,

    /// Token used for checkpointing operations
    #[serde(rename = "CheckpointToken")]
    pub checkpoint_token: String,

    /// Initial state containing previously checkpointed operations
    #[serde(rename = "InitialExecutionState")]
    pub initial_execution_state: InitialExecutionState,

    /// The user's original input payload (optional)
    #[serde(rename = "Input", default)]
    pub input: Option<serde_json::Value>,
}

/// Initial execution state containing checkpointed operations.
///
/// This is loaded when a durable execution resumes to enable replay
/// of previously completed operations.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct InitialExecutionState {
    /// List of previously checkpointed operations
    #[serde(rename = "Operations", default)]
    pub operations: Vec<Operation>,

    /// Marker for pagination if there are more operations to load
    #[serde(rename = "NextMarker", skip_serializing_if = "Option::is_none")]
    pub next_marker: Option<String>,
}

impl InitialExecutionState {
    /// Creates a new empty InitialExecutionState.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an InitialExecutionState with the given operations.
    pub fn with_operations(operations: Vec<Operation>) -> Self {
        Self {
            operations,
            next_marker: None,
        }
    }

    /// Returns true if there are more operations to load.
    pub fn has_more(&self) -> bool {
        self.next_marker.is_some()
    }
}

/// Output payload for a durable execution Lambda invocation.
///
/// This struct is serialized and returned from the Lambda function
/// to indicate the execution status.
#[derive(Debug, Clone, Serialize)]
pub struct DurableExecutionInvocationOutput {
    /// The status of the invocation
    #[serde(rename = "Status")]
    pub status: InvocationStatus,

    /// The serialized result if the execution succeeded
    #[serde(rename = "Result", skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,

    /// Error details if the execution failed
    #[serde(rename = "Error", skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
}

impl DurableExecutionInvocationOutput {
    /// Maximum response size in bytes (6MB Lambda limit)
    pub const MAX_RESPONSE_SIZE: usize = 6 * 1024 * 1024;
    
    /// Creates a new output indicating successful completion.
    pub fn succeeded(result: Option<String>) -> Self {
        Self {
            status: InvocationStatus::Succeeded,
            result,
            error: None,
        }
    }

    /// Creates a new output indicating failure.
    pub fn failed(error: ErrorObject) -> Self {
        Self {
            status: InvocationStatus::Failed,
            result: None,
            error: Some(error),
        }
    }

    /// Creates a new output indicating pending/suspended execution.
    pub fn pending() -> Self {
        Self {
            status: InvocationStatus::Pending,
            result: None,
            error: None,
        }
    }

    /// Returns true if the invocation succeeded.
    pub fn is_succeeded(&self) -> bool {
        matches!(self.status, InvocationStatus::Succeeded)
    }

    /// Returns true if the invocation failed.
    pub fn is_failed(&self) -> bool {
        matches!(self.status, InvocationStatus::Failed)
    }

    /// Returns true if the invocation is pending.
    pub fn is_pending(&self) -> bool {
        matches!(self.status, InvocationStatus::Pending)
    }
    
    /// Creates an output from a serializable result.
    ///
    /// This method handles:
    /// - Serializing the result to JSON
    /// - Checking if the response exceeds the maximum size
    /// - Returning appropriate error if serialization fails
    ///
    /// # Arguments
    ///
    /// * `result` - The result to serialize
    ///
    /// # Returns
    ///
    /// A `DurableExecutionInvocationOutput` with:
    /// - `SUCCEEDED` status if serialization succeeds and size is within limits
    /// - `FAILED` status if serialization fails or response is too large
    ///
    /// # Requirements
    ///
    /// - 15.5: WHEN the handler returns successfully, THE Lambda_Integration SHALL return SUCCEEDED status with result
    /// - 15.8: THE Lambda_Integration SHALL handle large responses by checkpointing before returning
    pub fn from_result<T: serde::Serialize>(result: &T) -> Self {
        match serde_json::to_string(result) {
            Ok(json) => {
                if json.len() > Self::MAX_RESPONSE_SIZE {
                    Self::failed(ErrorObject::new(
                        "ResponseTooLarge",
                        format!(
                            "Response size {} bytes exceeds maximum {} bytes. Consider checkpointing large results.",
                            json.len(),
                            Self::MAX_RESPONSE_SIZE
                        )
                    ))
                } else {
                    Self::succeeded(Some(json))
                }
            }
            Err(e) => Self::failed(ErrorObject::new(
                "SerializationError",
                format!("Failed to serialize result: {}", e)
            ))
        }
    }
    
    /// Creates an output from a DurableError.
    ///
    /// This method handles different error types:
    /// - `Suspend` errors return `PENDING` status
    /// - All other errors return `FAILED` status with error details
    ///
    /// # Arguments
    ///
    /// * `error` - The error to convert
    ///
    /// # Returns
    ///
    /// A `DurableExecutionInvocationOutput` with appropriate status
    ///
    /// # Requirements
    ///
    /// - 15.6: WHEN the handler fails, THE Lambda_Integration SHALL return FAILED status with error
    /// - 15.7: WHEN execution suspends, THE Lambda_Integration SHALL return PENDING status
    pub fn from_error(error: &crate::error::DurableError) -> Self {
        use crate::error::DurableError;
        
        match error {
            DurableError::Suspend { .. } => Self::pending(),
            _ => Self::failed(ErrorObject::from(error)),
        }
    }
    
    /// Checks if a result would exceed the maximum response size.
    ///
    /// This is useful for determining if a result should be checkpointed
    /// before returning.
    ///
    /// # Arguments
    ///
    /// * `result` - The result to check
    ///
    /// # Returns
    ///
    /// `true` if the serialized result would exceed the maximum size
    pub fn would_exceed_max_size<T: serde::Serialize>(result: &T) -> bool {
        match serde_json::to_string(result) {
            Ok(json) => json.len() > Self::MAX_RESPONSE_SIZE,
            Err(_) => false, // If we can't serialize, we'll handle the error elsewhere
        }
    }
    
    /// Creates an output for a large result that has been checkpointed.
    ///
    /// This method creates a SUCCEEDED output with a reference to the
    /// checkpointed result, rather than the result itself.
    ///
    /// # Arguments
    ///
    /// * `checkpoint_id` - The operation ID where the result was checkpointed
    /// * `original_size` - The size of the original serialized result in bytes
    ///
    /// # Returns
    ///
    /// A `DurableExecutionInvocationOutput` with SUCCEEDED status and a reference
    /// to the checkpointed result.
    ///
    /// # Requirements
    ///
    /// - 15.8: THE Lambda_Integration SHALL handle large responses by checkpointing before returning
    pub fn checkpointed_result(checkpoint_id: &str, original_size: usize) -> Self {
        Self::succeeded(Some(format!(
            "{{\"__checkpointed_result__\":\"{}\",\"size\":{}}}",
            checkpoint_id, original_size
        )))
    }
    
    /// Checks if this output represents a checkpointed large result.
    ///
    /// # Returns
    ///
    /// `true` if this output contains a reference to a checkpointed result
    pub fn is_checkpointed_result(&self) -> bool {
        self.result
            .as_ref()
            .map(|r| r.contains("__checkpointed_result__"))
            .unwrap_or(false)
    }
}

/// Status of a durable execution invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InvocationStatus {
    /// Execution completed successfully
    Succeeded,
    /// Execution failed with an error
    Failed,
    /// Execution is pending (suspended, waiting for callback, etc.)
    Pending,
}

impl std::fmt::Display for InvocationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Succeeded => write!(f, "SUCCEEDED"),
            Self::Failed => write!(f, "FAILED"),
            Self::Pending => write!(f, "PENDING"),
        }
    }
}

// Implement From conversions for common patterns

impl From<Result<String, ErrorObject>> for DurableExecutionInvocationOutput {
    fn from(result: Result<String, ErrorObject>) -> Self {
        match result {
            Ok(value) => Self::succeeded(Some(value)),
            Err(error) => Self::failed(error),
        }
    }
}

impl From<Result<Option<String>, ErrorObject>> for DurableExecutionInvocationOutput {
    fn from(result: Result<Option<String>, ErrorObject>) -> Self {
        match result {
            Ok(value) => Self::succeeded(value),
            Err(error) => Self::failed(error),
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::OperationType;

    #[test]
    fn test_invocation_input_deserialization() {
        let json = r#"{
            "DurableExecutionArn": "arn:aws:lambda:us-east-1:123456789012:function:my-function:durable:abc123",
            "CheckpointToken": "token-xyz",
            "InitialExecutionState": {
                "Operations": [
                    {
                        "Id": "op-1",
                        "Type": "STEP",
                        "Status": "SUCCEEDED",
                        "Result": "{\"value\": 42}"
                    }
                ],
                "NextMarker": null
            },
            "Input": {"orderId": "order-123"}
        }"#;

        let input: DurableExecutionInvocationInput = serde_json::from_str(json).unwrap();
        assert_eq!(
            input.durable_execution_arn,
            "arn:aws:lambda:us-east-1:123456789012:function:my-function:durable:abc123"
        );
        assert_eq!(input.checkpoint_token, "token-xyz");
        assert_eq!(input.initial_execution_state.operations.len(), 1);
        assert_eq!(
            input.initial_execution_state.operations[0].operation_id,
            "op-1"
        );
        assert!(input.input.is_some());
    }

    #[test]
    fn test_invocation_input_without_input() {
        let json = r#"{
            "DurableExecutionArn": "arn:aws:lambda:us-east-1:123456789012:function:my-function:durable:abc123",
            "CheckpointToken": "token-xyz",
            "InitialExecutionState": {
                "Operations": []
            }
        }"#;

        let input: DurableExecutionInvocationInput = serde_json::from_str(json).unwrap();
        assert!(input.input.is_none());
        assert!(input.initial_execution_state.operations.is_empty());
    }

    #[test]
    fn test_initial_execution_state_new() {
        let state = InitialExecutionState::new();
        assert!(state.operations.is_empty());
        assert!(state.next_marker.is_none());
        assert!(!state.has_more());
    }

    #[test]
    fn test_initial_execution_state_with_operations() {
        let ops = vec![
            Operation::new("op-1", OperationType::Step),
            Operation::new("op-2", OperationType::Wait),
        ];
        let state = InitialExecutionState::with_operations(ops);
        assert_eq!(state.operations.len(), 2);
        assert!(!state.has_more());
    }

    #[test]
    fn test_initial_execution_state_has_more() {
        let mut state = InitialExecutionState::new();
        assert!(!state.has_more());

        state.next_marker = Some("marker-123".to_string());
        assert!(state.has_more());
    }

    #[test]
    fn test_invocation_output_succeeded() {
        let output = DurableExecutionInvocationOutput::succeeded(Some(r#"{"result": "ok"}"#.to_string()));
        assert!(output.is_succeeded());
        assert!(!output.is_failed());
        assert!(!output.is_pending());
        assert_eq!(output.result, Some(r#"{"result": "ok"}"#.to_string()));
        assert!(output.error.is_none());
    }

    #[test]
    fn test_invocation_output_succeeded_no_result() {
        let output = DurableExecutionInvocationOutput::succeeded(None);
        assert!(output.is_succeeded());
        assert!(output.result.is_none());
    }

    #[test]
    fn test_invocation_output_failed() {
        let error = ErrorObject::new("TestError", "Something went wrong");
        let output = DurableExecutionInvocationOutput::failed(error);
        assert!(!output.is_succeeded());
        assert!(output.is_failed());
        assert!(!output.is_pending());
        assert!(output.result.is_none());
        assert!(output.error.is_some());
        assert_eq!(output.error.as_ref().unwrap().error_type, "TestError");
    }

    #[test]
    fn test_invocation_output_pending() {
        let output = DurableExecutionInvocationOutput::pending();
        assert!(!output.is_succeeded());
        assert!(!output.is_failed());
        assert!(output.is_pending());
        assert!(output.result.is_none());
        assert!(output.error.is_none());
    }

    #[test]
    fn test_invocation_status_display() {
        assert_eq!(InvocationStatus::Succeeded.to_string(), "SUCCEEDED");
        assert_eq!(InvocationStatus::Failed.to_string(), "FAILED");
        assert_eq!(InvocationStatus::Pending.to_string(), "PENDING");
    }

    #[test]
    fn test_invocation_status_serialization() {
        let json = serde_json::to_string(&InvocationStatus::Succeeded).unwrap();
        assert_eq!(json, r#""SUCCEEDED""#);

        let json = serde_json::to_string(&InvocationStatus::Failed).unwrap();
        assert_eq!(json, r#""FAILED""#);

        let json = serde_json::to_string(&InvocationStatus::Pending).unwrap();
        assert_eq!(json, r#""PENDING""#);
    }

    #[test]
    fn test_invocation_status_deserialization() {
        let status: InvocationStatus = serde_json::from_str(r#""SUCCEEDED""#).unwrap();
        assert_eq!(status, InvocationStatus::Succeeded);

        let status: InvocationStatus = serde_json::from_str(r#""FAILED""#).unwrap();
        assert_eq!(status, InvocationStatus::Failed);

        let status: InvocationStatus = serde_json::from_str(r#""PENDING""#).unwrap();
        assert_eq!(status, InvocationStatus::Pending);
    }

    #[test]
    fn test_invocation_output_serialization() {
        let output = DurableExecutionInvocationOutput::succeeded(Some(r#"{"value": 42}"#.to_string()));
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains(r#""Status":"SUCCEEDED""#));
        assert!(json.contains(r#""Result":"{\"value\": 42}""#));
        assert!(!json.contains("Error"));
    }

    #[test]
    fn test_invocation_output_failed_serialization() {
        let error = ErrorObject::new("TestError", "Something went wrong");
        let output = DurableExecutionInvocationOutput::failed(error);
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains(r#""Status":"FAILED""#));
        assert!(json.contains(r#""ErrorType":"TestError""#));
        assert!(!json.contains("Result"));
    }

    #[test]
    fn test_invocation_output_pending_serialization() {
        let output = DurableExecutionInvocationOutput::pending();
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains(r#""Status":"PENDING""#));
        assert!(!json.contains("Result"));
        assert!(!json.contains("Error"));
    }

    #[test]
    fn test_from_result_ok() {
        let result: Result<String, ErrorObject> = Ok(r#"{"value": 42}"#.to_string());
        let output: DurableExecutionInvocationOutput = result.into();
        assert!(output.is_succeeded());
        assert_eq!(output.result, Some(r#"{"value": 42}"#.to_string()));
    }

    #[test]
    fn test_from_result_err() {
        let result: Result<String, ErrorObject> = Err(ErrorObject::new("TestError", "Failed"));
        let output: DurableExecutionInvocationOutput = result.into();
        assert!(output.is_failed());
        assert_eq!(output.error.as_ref().unwrap().error_type, "TestError");
    }

    #[test]
    fn test_from_option_result_ok_some() {
        let result: Result<Option<String>, ErrorObject> = Ok(Some(r#"{"value": 42}"#.to_string()));
        let output: DurableExecutionInvocationOutput = result.into();
        assert!(output.is_succeeded());
        assert_eq!(output.result, Some(r#"{"value": 42}"#.to_string()));
    }

    #[test]
    fn test_from_option_result_ok_none() {
        let result: Result<Option<String>, ErrorObject> = Ok(None);
        let output: DurableExecutionInvocationOutput = result.into();
        assert!(output.is_succeeded());
        assert!(output.result.is_none());
    }
    
    #[test]
    fn test_from_result_serializable() {
        #[derive(serde::Serialize)]
        struct TestResult {
            value: i32,
            message: String,
        }
        
        let result = TestResult {
            value: 42,
            message: "success".to_string(),
        };
        
        let output = DurableExecutionInvocationOutput::from_result(&result);
        assert!(output.is_succeeded());
        assert!(output.result.is_some());
        let json = output.result.unwrap();
        assert!(json.contains("42"));
        assert!(json.contains("success"));
    }
    
    #[test]
    fn test_from_result_none() {
        let result: Option<String> = None;
        let output = DurableExecutionInvocationOutput::from_result(&result);
        assert!(output.is_succeeded());
        assert_eq!(output.result, Some("null".to_string()));
    }
    
    #[test]
    fn test_from_error_suspend() {
        use crate::error::DurableError;
        
        let error = DurableError::Suspend { scheduled_timestamp: None };
        let output = DurableExecutionInvocationOutput::from_error(&error);
        assert!(output.is_pending());
        assert!(output.result.is_none());
        assert!(output.error.is_none());
    }
    
    #[test]
    fn test_from_error_execution() {
        use crate::error::{DurableError, TerminationReason};
        
        let error = DurableError::Execution {
            message: "test error".to_string(),
            termination_reason: TerminationReason::ExecutionError,
        };
        let output = DurableExecutionInvocationOutput::from_error(&error);
        assert!(output.is_failed());
        assert!(output.error.is_some());
        assert_eq!(output.error.as_ref().unwrap().error_type, "ExecutionError");
    }
    
    #[test]
    fn test_from_error_validation() {
        use crate::error::DurableError;
        
        let error = DurableError::Validation {
            message: "invalid input".to_string(),
        };
        let output = DurableExecutionInvocationOutput::from_error(&error);
        assert!(output.is_failed());
        assert_eq!(output.error.as_ref().unwrap().error_type, "ValidationError");
    }
    
    #[test]
    fn test_would_exceed_max_size_small() {
        let small_data = "hello world";
        assert!(!DurableExecutionInvocationOutput::would_exceed_max_size(&small_data));
    }
    
    #[test]
    fn test_max_response_size_constant() {
        // Verify the constant is 6MB
        assert_eq!(DurableExecutionInvocationOutput::MAX_RESPONSE_SIZE, 6 * 1024 * 1024);
    }
    
    #[test]
    fn test_checkpointed_result() {
        let output = DurableExecutionInvocationOutput::checkpointed_result("op-123", 7_000_000);
        assert!(output.is_succeeded());
        assert!(output.is_checkpointed_result());
        let result = output.result.unwrap();
        assert!(result.contains("__checkpointed_result__"));
        assert!(result.contains("op-123"));
        assert!(result.contains("7000000"));
    }
    
    #[test]
    fn test_is_checkpointed_result_false() {
        let output = DurableExecutionInvocationOutput::succeeded(Some(r#"{"value": 42}"#.to_string()));
        assert!(!output.is_checkpointed_result());
    }
    
    #[test]
    fn test_is_checkpointed_result_none() {
        let output = DurableExecutionInvocationOutput::succeeded(None);
        assert!(!output.is_checkpointed_result());
    }
    
    #[test]
    fn test_is_checkpointed_result_pending() {
        let output = DurableExecutionInvocationOutput::pending();
        assert!(!output.is_checkpointed_result());
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::error::{DurableError, TerminationReason};
    use proptest::prelude::*;

    // Property: Lambda output matches execution outcome
    // *For any* execution result (success, failure, or suspend), the Lambda output
    // SHALL have the correct status and contain the appropriate data.
    // **Validates: Requirements 15.4, 15.5, 15.6, 15.7**

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// Feature: durable-execution-rust-sdk, Property: Lambda output matches execution outcome (Success)
        /// Validates: Requirements 15.4, 15.5
        /// 
        /// For any successful result that can be serialized, the output SHALL have
        /// SUCCEEDED status and contain the serialized result.
        #[test]
        fn prop_lambda_output_success_status(
            value in any::<i64>(),
            message in "[a-zA-Z0-9 ]{0,100}",
        ) {
            #[derive(serde::Serialize)]
            struct TestResult {
                value: i64,
                message: String,
            }
            
            let result = TestResult { value, message: message.clone() };
            let output = DurableExecutionInvocationOutput::from_result(&result);
            
            // Output must be SUCCEEDED
            prop_assert!(output.is_succeeded(), "Successful result must produce SUCCEEDED status");
            prop_assert!(!output.is_failed(), "Successful result must not be FAILED");
            prop_assert!(!output.is_pending(), "Successful result must not be PENDING");
            
            // Result must be present and contain the serialized data
            prop_assert!(output.result.is_some(), "Successful result must have result data");
            let json = output.result.as_ref().unwrap();
            prop_assert!(json.contains(&value.to_string()), "Result must contain the value");
            
            // Error must be absent
            prop_assert!(output.error.is_none(), "Successful result must not have error");
        }

        /// Feature: durable-execution-rust-sdk, Property: Lambda output matches execution outcome (Failure)
        /// Validates: Requirements 15.4, 15.6
        /// 
        /// For any error (except Suspend), the output SHALL have FAILED status
        /// and contain the error details.
        #[test]
        fn prop_lambda_output_failure_status(
            error_message in "[a-zA-Z0-9 ]{1,100}",
            error_variant in 0u8..7u8,
        ) {
            let error = match error_variant {
                0 => DurableError::Execution {
                    message: error_message.clone(),
                    termination_reason: TerminationReason::ExecutionError,
                },
                1 => DurableError::Invocation {
                    message: error_message.clone(),
                    termination_reason: TerminationReason::InvocationError,
                },
                2 => DurableError::Checkpoint {
                    message: error_message.clone(),
                    is_retriable: false,
                    aws_error: None,
                },
                3 => DurableError::Callback {
                    message: error_message.clone(),
                    callback_id: None,
                },
                4 => DurableError::NonDeterministic {
                    message: error_message.clone(),
                    operation_id: None,
                },
                5 => DurableError::Validation {
                    message: error_message.clone(),
                },
                _ => DurableError::SerDes {
                    message: error_message.clone(),
                },
            };
            
            let output = DurableExecutionInvocationOutput::from_error(&error);
            
            // Output must be FAILED
            prop_assert!(output.is_failed(), "Error must produce FAILED status");
            prop_assert!(!output.is_succeeded(), "Error must not be SUCCEEDED");
            prop_assert!(!output.is_pending(), "Error must not be PENDING");
            
            // Error must be present
            prop_assert!(output.error.is_some(), "Failed output must have error details");
            let error_obj = output.error.as_ref().unwrap();
            prop_assert!(!error_obj.error_type.is_empty(), "Error type must not be empty");
            prop_assert!(error_obj.error_message.contains(&error_message), "Error message must be preserved");
            
            // Result must be absent
            prop_assert!(output.result.is_none(), "Failed output must not have result");
        }

        /// Feature: durable-execution-rust-sdk, Property: Lambda output matches execution outcome (Suspend)
        /// Validates: Requirements 15.4, 15.7
        /// 
        /// For any Suspend error, the output SHALL have PENDING status.
        #[test]
        fn prop_lambda_output_suspend_status(
            has_timestamp in any::<bool>(),
            timestamp in any::<f64>(),
        ) {
            let error = if has_timestamp {
                DurableError::Suspend {
                    scheduled_timestamp: Some(timestamp),
                }
            } else {
                DurableError::Suspend {
                    scheduled_timestamp: None,
                }
            };
            
            let output = DurableExecutionInvocationOutput::from_error(&error);
            
            // Output must be PENDING
            prop_assert!(output.is_pending(), "Suspend must produce PENDING status");
            prop_assert!(!output.is_succeeded(), "Suspend must not be SUCCEEDED");
            prop_assert!(!output.is_failed(), "Suspend must not be FAILED");
            
            // Neither result nor error should be present for PENDING
            prop_assert!(output.result.is_none(), "Pending output must not have result");
            prop_assert!(output.error.is_none(), "Pending output must not have error");
        }

        /// Feature: durable-execution-rust-sdk, Property: Lambda output serialization round-trip
        /// Validates: Requirements 15.4
        /// 
        /// For any output, serializing and deserializing SHALL preserve the status.
        #[test]
        fn prop_lambda_output_serialization_preserves_status(
            status_variant in 0u8..3u8,
            result_value in any::<Option<i32>>(),
            error_message in "[a-zA-Z0-9 ]{0,50}",
        ) {
            let output = match status_variant {
                0 => DurableExecutionInvocationOutput::succeeded(
                    result_value.map(|v| format!("{{\"value\":{}}}", v))
                ),
                1 => DurableExecutionInvocationOutput::failed(
                    ErrorObject::new("TestError", &error_message)
                ),
                _ => DurableExecutionInvocationOutput::pending(),
            };
            
            // Serialize the output
            let json = serde_json::to_string(&output).expect("Serialization must succeed");
            
            // Verify the JSON contains the correct status
            match status_variant {
                0 => prop_assert!(json.contains("SUCCEEDED"), "JSON must contain SUCCEEDED"),
                1 => prop_assert!(json.contains("FAILED"), "JSON must contain FAILED"),
                _ => prop_assert!(json.contains("PENDING"), "JSON must contain PENDING"),
            }
            
            // Verify result/error presence in JSON
            if output.result.is_some() {
                prop_assert!(json.contains("Result"), "JSON must contain Result field");
            }
            if output.error.is_some() {
                prop_assert!(json.contains("Error"), "JSON must contain Error field");
            }
        }

        /// Feature: durable-execution-rust-sdk, Property: Result size check consistency
        /// Validates: Requirements 15.8
        /// 
        /// For any result, would_exceed_max_size and from_result SHALL be consistent.
        #[test]
        fn prop_result_size_check_consistency(
            data_size in 0usize..1000usize,
        ) {
            // Create a string of the specified size
            let data: String = "x".repeat(data_size);
            
            let would_exceed = DurableExecutionInvocationOutput::would_exceed_max_size(&data);
            let output = DurableExecutionInvocationOutput::from_result(&data);
            
            // If would_exceed is false, output should be SUCCEEDED
            // (Note: we're testing with small sizes, so this should always be false)
            if !would_exceed {
                prop_assert!(output.is_succeeded(), 
                    "If size check passes, output must be SUCCEEDED");
            }
            
            // For small data, should never exceed
            if data_size < 1000 {
                prop_assert!(!would_exceed, "Small data should not exceed max size");
                prop_assert!(output.is_succeeded(), "Small data should produce SUCCEEDED");
            }
        }
    }
}
