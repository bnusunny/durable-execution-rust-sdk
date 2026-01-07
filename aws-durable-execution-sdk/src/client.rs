//! Service client for the AWS Durable Execution SDK.
//!
//! This module defines the `DurableServiceClient` trait and provides
//! a Lambda-based implementation for communicating with the AWS Lambda
//! durable execution service.

use std::sync::Arc;

use async_trait::async_trait;
use aws_sdk_lambda::operation::RequestId;
use aws_sdk_lambda::Client as LambdaClient;
use serde::{Deserialize, Serialize};

use crate::error::{AwsError, DurableError};
use crate::operation::{Operation, OperationUpdate};

/// Trait for communicating with the durable execution service.
///
/// This trait abstracts the communication layer, allowing for different
/// implementations (e.g., Lambda client, mock client for testing).
#[async_trait]
pub trait DurableServiceClient: Send + Sync {
    /// Sends a batch of checkpoint operations to the service.
    ///
    /// # Arguments
    ///
    /// * `durable_execution_arn` - The ARN of the durable execution
    /// * `checkpoint_token` - The token for this checkpoint batch
    /// * `operations` - The operations to checkpoint
    ///
    /// # Returns
    ///
    /// A new checkpoint token on success, or an error on failure.
    async fn checkpoint(
        &self,
        durable_execution_arn: &str,
        checkpoint_token: &str,
        operations: Vec<OperationUpdate>,
    ) -> Result<CheckpointResponse, DurableError>;

    /// Retrieves additional operations for pagination.
    ///
    /// # Arguments
    ///
    /// * `durable_execution_arn` - The ARN of the durable execution
    /// * `next_marker` - The pagination marker from the previous response
    ///
    /// # Returns
    ///
    /// A list of operations and an optional next marker for further pagination.
    async fn get_operations(
        &self,
        durable_execution_arn: &str,
        next_marker: &str,
    ) -> Result<GetOperationsResponse, DurableError>;
}

/// Response from a checkpoint operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointResponse {
    /// The new checkpoint token to use for subsequent checkpoints
    #[serde(rename = "CheckpointToken")]
    pub checkpoint_token: String,
}

/// Response from a get_operations call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetOperationsResponse {
    /// The retrieved operations
    #[serde(rename = "Operations")]
    pub operations: Vec<Operation>,

    /// Marker for the next page of results, if any
    #[serde(rename = "NextMarker", skip_serializing_if = "Option::is_none")]
    pub next_marker: Option<String>,
}

/// Configuration for the Lambda durable service client.
#[derive(Debug, Clone, Default)]
pub struct LambdaClientConfig {
    /// The Lambda function ARN to call for checkpoint operations
    pub checkpoint_function_arn: Option<String>,
    /// The Lambda function ARN to call for get_operations
    pub get_operations_function_arn: Option<String>,
}

impl LambdaClientConfig {
    /// Creates a new LambdaClientConfig from AWS SDK config.
    ///
    /// This is a convenience method that creates a default configuration.
    /// The actual Lambda client will be created separately.
    pub fn from_aws_config(_config: &aws_config::SdkConfig) -> Self {
        Self::default()
    }
}

/// Lambda-based implementation of the DurableServiceClient.
///
/// This client uses the AWS Lambda SDK to communicate with the
/// durable execution service.
pub struct LambdaDurableServiceClient {
    /// The underlying Lambda client
    lambda_client: LambdaClient,
    /// Configuration for the client
    config: LambdaClientConfig,
}

impl LambdaDurableServiceClient {
    /// Creates a new LambdaDurableServiceClient with the given Lambda client.
    pub fn new(lambda_client: LambdaClient) -> Self {
        Self {
            lambda_client,
            config: LambdaClientConfig::default(),
        }
    }

    /// Creates a new LambdaDurableServiceClient with custom configuration.
    pub fn with_config(lambda_client: LambdaClient, config: LambdaClientConfig) -> Self {
        Self {
            lambda_client,
            config,
        }
    }

    /// Creates a new LambdaDurableServiceClient from AWS config.
    pub async fn from_env() -> Self {
        let aws_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .load()
            .await;
        let lambda_client = LambdaClient::new(&aws_config);
        Self::new(lambda_client)
    }

    /// Returns a reference to the underlying Lambda client.
    pub fn lambda_client(&self) -> &LambdaClient {
        &self.lambda_client
    }
}


/// Request payload for checkpoint operations.
#[derive(Debug, Clone, Serialize)]
struct CheckpointRequest {
    #[serde(rename = "DurableExecutionArn")]
    durable_execution_arn: String,
    #[serde(rename = "CheckpointToken")]
    checkpoint_token: String,
    #[serde(rename = "Operations")]
    operations: Vec<OperationUpdate>,
}

/// Request payload for get_operations.
#[derive(Debug, Clone, Serialize)]
struct GetOperationsRequest {
    #[serde(rename = "DurableExecutionArn")]
    durable_execution_arn: String,
    #[serde(rename = "NextMarker")]
    next_marker: String,
}

#[async_trait]
impl DurableServiceClient for LambdaDurableServiceClient {
    async fn checkpoint(
        &self,
        durable_execution_arn: &str,
        checkpoint_token: &str,
        operations: Vec<OperationUpdate>,
    ) -> Result<CheckpointResponse, DurableError> {
        let request = CheckpointRequest {
            durable_execution_arn: durable_execution_arn.to_string(),
            checkpoint_token: checkpoint_token.to_string(),
            operations,
        };

        let payload = serde_json::to_vec(&request).map_err(|e| DurableError::SerDes {
            message: format!("Failed to serialize checkpoint request: {}", e),
        })?;

        // Use the configured function ARN or derive from the durable execution ARN
        let function_name = self
            .config
            .checkpoint_function_arn
            .clone()
            .unwrap_or_else(|| derive_checkpoint_function_arn(durable_execution_arn));

        let response = self
            .lambda_client
            .invoke()
            .function_name(&function_name)
            .payload(aws_sdk_lambda::primitives::Blob::new(payload))
            .send()
            .await
            .map_err(|e| map_lambda_error(e, "checkpoint"))?;

        // Check for function error
        if let Some(function_error) = response.function_error() {
            return Err(DurableError::Checkpoint {
                message: format!("Lambda function error: {}", function_error),
                is_retriable: is_retriable_function_error(function_error),
                aws_error: None,
            });
        }

        // Parse the response payload
        let response_payload = response
            .payload()
            .ok_or_else(|| DurableError::Checkpoint {
                message: "Empty response from checkpoint function".to_string(),
                is_retriable: true,
                aws_error: None,
            })?;

        let checkpoint_response: CheckpointResponse =
            serde_json::from_slice(response_payload.as_ref()).map_err(|e| {
                DurableError::SerDes {
                    message: format!("Failed to deserialize checkpoint response: {}", e),
                }
            })?;

        Ok(checkpoint_response)
    }

    async fn get_operations(
        &self,
        durable_execution_arn: &str,
        next_marker: &str,
    ) -> Result<GetOperationsResponse, DurableError> {
        let request = GetOperationsRequest {
            durable_execution_arn: durable_execution_arn.to_string(),
            next_marker: next_marker.to_string(),
        };

        let payload = serde_json::to_vec(&request).map_err(|e| DurableError::SerDes {
            message: format!("Failed to serialize get_operations request: {}", e),
        })?;

        // Use the configured function ARN or derive from the durable execution ARN
        let function_name = self
            .config
            .get_operations_function_arn
            .clone()
            .unwrap_or_else(|| derive_get_operations_function_arn(durable_execution_arn));

        let response = self
            .lambda_client
            .invoke()
            .function_name(&function_name)
            .payload(aws_sdk_lambda::primitives::Blob::new(payload))
            .send()
            .await
            .map_err(|e| map_lambda_error(e, "get_operations"))?;

        // Check for function error
        if let Some(function_error) = response.function_error() {
            return Err(DurableError::Invocation {
                message: format!("Lambda function error: {}", function_error),
                termination_reason: crate::error::TerminationReason::InvocationError,
            });
        }

        // Parse the response payload
        let response_payload = response
            .payload()
            .ok_or_else(|| DurableError::Invocation {
                message: "Empty response from get_operations function".to_string(),
                termination_reason: crate::error::TerminationReason::InvocationError,
            })?;

        let operations_response: GetOperationsResponse =
            serde_json::from_slice(response_payload.as_ref()).map_err(|e| {
                DurableError::SerDes {
                    message: format!("Failed to deserialize get_operations response: {}", e),
                }
            })?;

        Ok(operations_response)
    }
}

/// Derives the checkpoint function ARN from the durable execution ARN.
fn derive_checkpoint_function_arn(durable_execution_arn: &str) -> String {
    // The checkpoint function is typically a Lambda extension or internal service
    // For now, we use a placeholder that would be replaced with actual service endpoint
    format!("{}:checkpoint", durable_execution_arn)
}

/// Derives the get_operations function ARN from the durable execution ARN.
fn derive_get_operations_function_arn(durable_execution_arn: &str) -> String {
    // Similar to checkpoint, this would be the actual service endpoint
    format!("{}:get_operations", durable_execution_arn)
}

/// Maps Lambda SDK errors to DurableError.
fn map_lambda_error(
    error: aws_sdk_lambda::error::SdkError<aws_sdk_lambda::operation::invoke::InvokeError>,
    operation: &str,
) -> DurableError {
    let (is_retriable, aws_error) = match &error {
        aws_sdk_lambda::error::SdkError::ServiceError(service_error) => {
            let err = service_error.err();
            let is_retriable = matches!(
                err,
                aws_sdk_lambda::operation::invoke::InvokeError::TooManyRequestsException(_)
                    | aws_sdk_lambda::operation::invoke::InvokeError::ServiceException(_)
            );
            let aws_error = Some(AwsError {
                code: format!("{:?}", err),
                message: err.to_string(),
                request_id: service_error
                    .raw()
                    .request_id()
                    .map(|s| s.to_string()),
            });
            (is_retriable, aws_error)
        }
        aws_sdk_lambda::error::SdkError::TimeoutError(_) => (true, None),
        aws_sdk_lambda::error::SdkError::DispatchFailure(_) => (true, None),
        _ => (false, None),
    };

    DurableError::Checkpoint {
        message: format!("Lambda {} failed: {}", operation, error),
        is_retriable,
        aws_error,
    }
}

/// Determines if a function error is retriable.
fn is_retriable_function_error(error: &str) -> bool {
    // Common retriable error patterns
    error.contains("ServiceException")
        || error.contains("TooManyRequests")
        || error.contains("Throttling")
        || error.contains("InternalError")
}

/// A mock implementation of DurableServiceClient for testing.
#[cfg(test)]
pub struct MockDurableServiceClient {
    checkpoint_responses: std::sync::Mutex<Vec<Result<CheckpointResponse, DurableError>>>,
    get_operations_responses: std::sync::Mutex<Vec<Result<GetOperationsResponse, DurableError>>>,
}

#[cfg(test)]
impl MockDurableServiceClient {
    pub fn new() -> Self {
        Self {
            checkpoint_responses: std::sync::Mutex::new(Vec::new()),
            get_operations_responses: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn with_checkpoint_response(self, response: Result<CheckpointResponse, DurableError>) -> Self {
        self.checkpoint_responses.lock().unwrap().push(response);
        self
    }

    pub fn with_get_operations_response(self, response: Result<GetOperationsResponse, DurableError>) -> Self {
        self.get_operations_responses.lock().unwrap().push(response);
        self
    }

    /// Adds multiple checkpoint responses at once.
    pub fn with_checkpoint_responses(self, count: usize) -> Self {
        let mut responses = self.checkpoint_responses.lock().unwrap();
        for i in 0..count {
            responses.push(Ok(CheckpointResponse {
                checkpoint_token: format!("token-{}", i),
            }));
        }
        drop(responses);
        self
    }
}

#[cfg(test)]
#[async_trait]
impl DurableServiceClient for MockDurableServiceClient {
    async fn checkpoint(
        &self,
        _durable_execution_arn: &str,
        _checkpoint_token: &str,
        _operations: Vec<OperationUpdate>,
    ) -> Result<CheckpointResponse, DurableError> {
        let mut responses = self.checkpoint_responses.lock().unwrap();
        if responses.is_empty() {
            Ok(CheckpointResponse {
                checkpoint_token: "mock-token".to_string(),
            })
        } else {
            responses.remove(0)
        }
    }

    async fn get_operations(
        &self,
        _durable_execution_arn: &str,
        _next_marker: &str,
    ) -> Result<GetOperationsResponse, DurableError> {
        let mut responses = self.get_operations_responses.lock().unwrap();
        if responses.is_empty() {
            Ok(GetOperationsResponse {
                operations: Vec::new(),
                next_marker: None,
            })
        } else {
            responses.remove(0)
        }
    }
}

/// Type alias for a shared DurableServiceClient.
pub type SharedDurableServiceClient = Arc<dyn DurableServiceClient>;


#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::OperationType;

    #[test]
    fn test_checkpoint_response_serialization() {
        let response = CheckpointResponse {
            checkpoint_token: "token-123".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(r#""CheckpointToken":"token-123""#));
    }

    #[test]
    fn test_checkpoint_response_deserialization() {
        let json = r#"{"CheckpointToken": "token-456"}"#;
        let response: CheckpointResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.checkpoint_token, "token-456");
    }

    #[test]
    fn test_get_operations_response_serialization() {
        let response = GetOperationsResponse {
            operations: vec![Operation::new("op-1", OperationType::Step)],
            next_marker: Some("marker-123".to_string()),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(r#""Operations""#));
        assert!(json.contains(r#""NextMarker":"marker-123""#));
    }

    #[test]
    fn test_get_operations_response_deserialization() {
        let json = r#"{
            "Operations": [
                {
                    "OperationId": "op-1",
                    "OperationType": "Step",
                    "Status": "Succeeded"
                }
            ],
            "NextMarker": "marker-456"
        }"#;
        let response: GetOperationsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.operations.len(), 1);
        assert_eq!(response.operations[0].operation_id, "op-1");
        assert_eq!(response.next_marker, Some("marker-456".to_string()));
    }

    #[test]
    fn test_get_operations_response_without_marker() {
        let json = r#"{
            "Operations": []
        }"#;
        let response: GetOperationsResponse = serde_json::from_str(json).unwrap();
        assert!(response.operations.is_empty());
        assert!(response.next_marker.is_none());
    }

    #[test]
    fn test_lambda_client_config_default() {
        let config = LambdaClientConfig::default();
        assert!(config.checkpoint_function_arn.is_none());
        assert!(config.get_operations_function_arn.is_none());
    }

    #[test]
    fn test_derive_checkpoint_function_arn() {
        let arn = "arn:aws:lambda:us-east-1:123456789012:function:my-function:durable:abc123";
        let checkpoint_arn = derive_checkpoint_function_arn(arn);
        assert!(checkpoint_arn.contains(arn));
        assert!(checkpoint_arn.contains(":checkpoint"));
    }

    #[test]
    fn test_derive_get_operations_function_arn() {
        let arn = "arn:aws:lambda:us-east-1:123456789012:function:my-function:durable:abc123";
        let get_ops_arn = derive_get_operations_function_arn(arn);
        assert!(get_ops_arn.contains(arn));
        assert!(get_ops_arn.contains(":get_operations"));
    }

    #[test]
    fn test_is_retriable_function_error() {
        assert!(is_retriable_function_error("ServiceException: Internal error"));
        assert!(is_retriable_function_error("TooManyRequests: Rate exceeded"));
        assert!(is_retriable_function_error("Throttling: Request throttled"));
        assert!(is_retriable_function_error("InternalError: Something went wrong"));
        assert!(!is_retriable_function_error("ValidationError: Invalid input"));
        assert!(!is_retriable_function_error("ResourceNotFound: Function not found"));
    }

    #[tokio::test]
    async fn test_mock_client_checkpoint() {
        let client = MockDurableServiceClient::new();
        let result = client
            .checkpoint(
                "arn:aws:lambda:us-east-1:123456789012:function:test",
                "token-123",
                vec![],
            )
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().checkpoint_token, "mock-token");
    }

    #[tokio::test]
    async fn test_mock_client_checkpoint_with_custom_response() {
        let client = MockDurableServiceClient::new().with_checkpoint_response(Ok(
            CheckpointResponse {
                checkpoint_token: "custom-token".to_string(),
            },
        ));
        let result = client
            .checkpoint(
                "arn:aws:lambda:us-east-1:123456789012:function:test",
                "token-123",
                vec![],
            )
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().checkpoint_token, "custom-token");
    }

    #[tokio::test]
    async fn test_mock_client_checkpoint_with_error() {
        let client = MockDurableServiceClient::new().with_checkpoint_response(Err(
            DurableError::checkpoint_retriable("Test error"),
        ));
        let result = client
            .checkpoint(
                "arn:aws:lambda:us-east-1:123456789012:function:test",
                "token-123",
                vec![],
            )
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().is_retriable());
    }

    #[tokio::test]
    async fn test_mock_client_get_operations() {
        let client = MockDurableServiceClient::new();
        let result = client
            .get_operations(
                "arn:aws:lambda:us-east-1:123456789012:function:test",
                "marker-123",
            )
            .await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.operations.is_empty());
        assert!(response.next_marker.is_none());
    }

    #[tokio::test]
    async fn test_mock_client_get_operations_with_custom_response() {
        let client = MockDurableServiceClient::new().with_get_operations_response(Ok(
            GetOperationsResponse {
                operations: vec![Operation::new("op-1", OperationType::Step)],
                next_marker: Some("next-marker".to_string()),
            },
        ));
        let result = client
            .get_operations(
                "arn:aws:lambda:us-east-1:123456789012:function:test",
                "marker-123",
            )
            .await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.operations.len(), 1);
        assert_eq!(response.next_marker, Some("next-marker".to_string()));
    }

    #[test]
    fn test_checkpoint_request_serialization() {
        let request = CheckpointRequest {
            durable_execution_arn: "arn:test".to_string(),
            checkpoint_token: "token-123".to_string(),
            operations: vec![OperationUpdate::start("op-1", OperationType::Step)],
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains(r#""DurableExecutionArn":"arn:test""#));
        assert!(json.contains(r#""CheckpointToken":"token-123""#));
        assert!(json.contains(r#""Operations""#));
    }

    #[test]
    fn test_get_operations_request_serialization() {
        let request = GetOperationsRequest {
            durable_execution_arn: "arn:test".to_string(),
            next_marker: "marker-123".to_string(),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains(r#""DurableExecutionArn":"arn:test""#));
        assert!(json.contains(r#""NextMarker":"marker-123""#));
    }
}
