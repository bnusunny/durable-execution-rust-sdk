//! Service client for the AWS Durable Execution SDK.
//!
//! This module defines the `DurableServiceClient` trait and provides
//! a Lambda-based implementation for communicating with the AWS Lambda
//! durable execution service using the CheckpointDurableExecution and
//! GetDurableExecutionState REST APIs.

use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use aws_credential_types::provider::ProvideCredentials;
use aws_sigv4::http_request::{sign, SignableBody, SignableRequest, SigningSettings};
use aws_sigv4::sign::v4;
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
#[derive(Debug, Clone)]
pub struct LambdaClientConfig {
    /// AWS region for the Lambda service
    pub region: String,
    /// Optional custom endpoint URL (for testing)
    pub endpoint_url: Option<String>,
}

impl Default for LambdaClientConfig {
    fn default() -> Self {
        Self {
            region: "us-east-1".to_string(),
            endpoint_url: None,
        }
    }
}

impl LambdaClientConfig {
    /// Creates a new LambdaClientConfig with the specified region.
    pub fn with_region(region: impl Into<String>) -> Self {
        Self {
            region: region.into(),
            endpoint_url: None,
        }
    }

    /// Creates a new LambdaClientConfig from AWS SDK config.
    pub fn from_aws_config(config: &aws_config::SdkConfig) -> Self {
        Self {
            region: config.region().map(|r| r.to_string()).unwrap_or_else(|| "us-east-1".to_string()),
            endpoint_url: None,
        }
    }
}

/// Lambda-based implementation of the DurableServiceClient.
///
/// This client uses the AWS Lambda REST APIs (CheckpointDurableExecution and
/// GetDurableExecutionState) to communicate with the durable execution service.
pub struct LambdaDurableServiceClient {
    /// HTTP client for making requests
    http_client: reqwest::Client,
    /// AWS credentials provider
    credentials_provider: Arc<dyn ProvideCredentials>,
    /// Configuration for the client
    config: LambdaClientConfig,
}

impl LambdaDurableServiceClient {
    /// Creates a new LambdaDurableServiceClient from AWS config.
    pub async fn from_env() -> Self {
        let aws_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .load()
            .await;
        Self::from_aws_config(&aws_config)
    }

    /// Creates a new LambdaDurableServiceClient from AWS SDK config.
    pub fn from_aws_config(aws_config: &aws_config::SdkConfig) -> Self {
        let credentials_provider = aws_config
            .credentials_provider()
            .expect("No credentials provider configured")
            .clone();
        
        Self {
            http_client: reqwest::Client::new(),
            credentials_provider: Arc::from(credentials_provider),
            config: LambdaClientConfig::from_aws_config(aws_config),
        }
    }

    /// Creates a new LambdaDurableServiceClient with custom configuration.
    pub fn with_config(
        credentials_provider: Arc<dyn ProvideCredentials>,
        config: LambdaClientConfig,
    ) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            credentials_provider,
            config,
        }
    }

    /// Creates a new LambdaDurableServiceClient with a Lambda client (for backward compatibility).
    /// Note: This extracts the region from the Lambda client but uses direct HTTP calls.
    /// 
    /// IMPORTANT: This method requires that the Lambda client was created with credentials.
    /// If you're using this in a Lambda function, prefer using `from_env()` instead.
    pub fn new(_lambda_client: aws_sdk_lambda::Client) -> Self {
        // Note: We can't easily extract credentials from the Lambda client anymore
        // as the credentials_provider() method is deprecated and returns None.
        // This method is kept for backward compatibility but will panic if used.
        // Users should use from_env() or from_aws_config() instead.
        panic!(
            "LambdaDurableServiceClient::new() is deprecated. \
             Use LambdaDurableServiceClient::from_env() or from_aws_config() instead."
        );
    }

    /// Returns the Lambda service endpoint URL.
    fn endpoint_url(&self) -> String {
        self.config.endpoint_url.clone().unwrap_or_else(|| {
            format!("https://lambda.{}.amazonaws.com", self.config.region)
        })
    }

    /// Signs an HTTP request using AWS SigV4 and returns the signed headers.
    async fn sign_request(
        &self,
        method: &str,
        uri: &str,
        body: &[u8],
    ) -> Result<Vec<(String, String)>, DurableError> {
        let credentials = self
            .credentials_provider
            .provide_credentials()
            .await
            .map_err(|e| DurableError::Checkpoint {
                message: format!("Failed to get AWS credentials: {}", e),
                is_retriable: true,
                aws_error: None,
            })?;

        let identity = credentials.into();
        let signing_settings = SigningSettings::default();
        let signing_params = v4::SigningParams::builder()
            .identity(&identity)
            .region(&self.config.region)
            .name("lambda")
            .time(SystemTime::now())
            .settings(signing_settings)
            .build()
            .map_err(|e| DurableError::Checkpoint {
                message: format!("Failed to build signing params: {}", e),
                is_retriable: false,
                aws_error: None,
            })?;

        let signable_request = SignableRequest::new(
            method,
            uri,
            std::iter::empty::<(&str, &str)>(),
            SignableBody::Bytes(body),
        )
        .map_err(|e| DurableError::Checkpoint {
            message: format!("Failed to create signable request: {}", e),
            is_retriable: false,
            aws_error: None,
        })?;

        let (signing_instructions, _signature) = sign(signable_request, &signing_params.into())
            .map_err(|e| DurableError::Checkpoint {
                message: format!("Failed to sign request: {}", e),
                is_retriable: false,
                aws_error: None,
            })?
            .into_parts();

        // Build a temporary HTTP request to apply signing instructions
        let mut temp_request = http::Request::builder()
            .method(method)
            .uri(uri)
            .body(())
            .map_err(|e| DurableError::Checkpoint {
                message: format!("Failed to build temp request: {}", e),
                is_retriable: false,
                aws_error: None,
            })?;

        signing_instructions.apply_to_request_http1x(&mut temp_request);

        // Extract headers from the signed request
        let headers: Vec<(String, String)> = temp_request
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.to_string(),
                    value.to_str().unwrap_or("").to_string(),
                )
            })
            .collect();

        Ok(headers)
    }
}

/// Request payload for checkpoint operations.
#[derive(Debug, Clone, Serialize)]
struct CheckpointRequestBody {
    #[serde(rename = "CheckpointToken")]
    checkpoint_token: String,
    #[serde(rename = "Updates")]
    updates: Vec<OperationUpdate>,
}

/// Request payload for get_operations.
#[derive(Debug, Clone, Serialize)]
struct GetOperationsRequestBody {
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
        let request_body = CheckpointRequestBody {
            checkpoint_token: checkpoint_token.to_string(),
            updates: operations,
        };

        let body = serde_json::to_vec(&request_body).map_err(|e| DurableError::SerDes {
            message: format!("Failed to serialize checkpoint request: {}", e),
        })?;

        // URL-encode the durable execution ARN for the path
        let encoded_arn = urlencoding::encode(durable_execution_arn);
        let uri = format!(
            "{}/2025-12-01/durable-executions/{}/checkpoint",
            self.endpoint_url(),
            encoded_arn
        );

        // Sign the request
        let signed_headers = self.sign_request("POST", &uri, &body).await?;

        // Build and send the request
        let mut request = self
            .http_client
            .post(&uri)
            .header("Content-Type", "application/json")
            .body(body);

        for (name, value) in signed_headers {
            request = request.header(&name, &value);
        }

        let response = request.send().await.map_err(|e| DurableError::Checkpoint {
            message: format!("HTTP request failed: {}", e),
            is_retriable: e.is_timeout() || e.is_connect(),
            aws_error: None,
        })?;

        let status = response.status();
        let response_body = response.bytes().await.map_err(|e| DurableError::Checkpoint {
            message: format!("Failed to read response body: {}", e),
            is_retriable: true,
            aws_error: None,
        })?;

        if !status.is_success() {
            let error_message = String::from_utf8_lossy(&response_body);
            return Err(DurableError::Checkpoint {
                message: format!("Checkpoint API returned {}: {}", status, error_message),
                is_retriable: status.is_server_error() || status.as_u16() == 429,
                aws_error: Some(AwsError {
                    code: status.to_string(),
                    message: error_message.to_string(),
                    request_id: None,
                }),
            });
        }

        let checkpoint_response: CheckpointResponse =
            serde_json::from_slice(&response_body).map_err(|e| DurableError::SerDes {
                message: format!("Failed to deserialize checkpoint response: {}", e),
            })?;

        Ok(checkpoint_response)
    }

    async fn get_operations(
        &self,
        durable_execution_arn: &str,
        next_marker: &str,
    ) -> Result<GetOperationsResponse, DurableError> {
        let request_body = GetOperationsRequestBody {
            next_marker: next_marker.to_string(),
        };

        let body = serde_json::to_vec(&request_body).map_err(|e| DurableError::SerDes {
            message: format!("Failed to serialize get_operations request: {}", e),
        })?;

        // URL-encode the durable execution ARN for the path
        let encoded_arn = urlencoding::encode(durable_execution_arn);
        let uri = format!(
            "{}/2025-12-01/durable-executions/{}/state",
            self.endpoint_url(),
            encoded_arn
        );

        // Sign the request
        let signed_headers = self.sign_request("POST", &uri, &body).await?;

        // Build and send the request
        let mut request = self
            .http_client
            .post(&uri)
            .header("Content-Type", "application/json")
            .body(body);

        for (name, value) in signed_headers {
            request = request.header(&name, &value);
        }

        let response = request.send().await.map_err(|e| DurableError::Invocation {
            message: format!("HTTP request failed: {}", e),
            termination_reason: crate::error::TerminationReason::InvocationError,
        })?;

        let status = response.status();
        let response_body = response.bytes().await.map_err(|e| DurableError::Invocation {
            message: format!("Failed to read response body: {}", e),
            termination_reason: crate::error::TerminationReason::InvocationError,
        })?;

        if !status.is_success() {
            let error_message = String::from_utf8_lossy(&response_body);
            return Err(DurableError::Invocation {
                message: format!("GetDurableExecutionState API returned {}: {}", status, error_message),
                termination_reason: crate::error::TerminationReason::InvocationError,
            });
        }

        let operations_response: GetOperationsResponse =
            serde_json::from_slice(&response_body).map_err(|e| DurableError::SerDes {
                message: format!("Failed to deserialize get_operations response: {}", e),
            })?;

        Ok(operations_response)
    }
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
                    "Id": "op-1",
                    "Type": "STEP",
                    "Status": "SUCCEEDED"
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
        assert_eq!(config.region, "us-east-1");
        assert!(config.endpoint_url.is_none());
    }

    #[test]
    fn test_lambda_client_config_with_region() {
        let config = LambdaClientConfig::with_region("us-west-2");
        assert_eq!(config.region, "us-west-2");
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
    fn test_checkpoint_request_body_serialization() {
        let request = CheckpointRequestBody {
            checkpoint_token: "token-123".to_string(),
            updates: vec![OperationUpdate::start("op-1", OperationType::Step)],
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains(r#""CheckpointToken":"token-123""#));
        assert!(json.contains(r#""Updates""#));
    }

    #[test]
    fn test_get_operations_request_body_serialization() {
        let request = GetOperationsRequestBody {
            next_marker: "marker-123".to_string(),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains(r#""NextMarker":"marker-123""#));
    }
}
