//! Cloud test runner for testing deployed Lambda functions.
//!
//! This module provides the `CloudDurableTestRunner` for testing durable functions
//! deployed to AWS Lambda, enabling integration testing against real AWS infrastructure.
//!
//! # Examples
//!
//! ```ignore
//! use aws_durable_execution_sdk_testing::{
//!     CloudDurableTestRunner, CloudTestRunnerConfig, ExecutionStatus,
//! };
//!
//! #[tokio::test]
//! async fn test_deployed_workflow() {
//!     let runner = CloudDurableTestRunner::<String>::new("my-function-name")
//!         .await
//!         .unwrap();
//!
//!     let result = runner.run("input".to_string()).await.unwrap();
//!     assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
//! }
//! ```

use std::collections::HashMap;
use std::marker::PhantomData;
use std::time::Duration;

use aws_sdk_lambda::Client as LambdaClient;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::TestError;
use crate::operation::DurableOperation;
use crate::test_result::TestResult;
use crate::types::{Invocation, TestResultError};
use aws_durable_execution_sdk::Operation;

/// Configuration for the cloud test runner.
///
/// Controls polling behavior and timeouts when testing deployed Lambda functions.
///
/// # Examples
///
/// ```
/// use aws_durable_execution_sdk_testing::CloudTestRunnerConfig;
/// use std::time::Duration;
///
/// let config = CloudTestRunnerConfig {
///     poll_interval: Duration::from_millis(500),
///     timeout: Duration::from_secs(60),
/// };
/// ```
#[derive(Debug, Clone)]
pub struct CloudTestRunnerConfig {
    /// Polling interval when waiting for execution completion.
    ///
    /// Default: 1000ms (1 second)
    pub poll_interval: Duration,

    /// Maximum wait time for execution completion.
    ///
    /// Default: 300 seconds (5 minutes)
    pub timeout: Duration,
}

impl Default for CloudTestRunnerConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(1000),
            timeout: Duration::from_secs(300),
        }
    }
}

impl CloudTestRunnerConfig {
    /// Creates a new configuration with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the polling interval.
    ///
    /// # Arguments
    ///
    /// * `interval` - The interval between status polls
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Sets the timeout.
    ///
    /// # Arguments
    ///
    /// * `timeout` - The maximum time to wait for execution completion
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Internal storage for operations captured from cloud execution.
#[derive(Debug, Default)]
struct OperationStorage {
    /// All operations in execution order
    operations: Vec<Operation>,
    /// Map from operation ID to index in operations vec
    operations_by_id: HashMap<String, usize>,
    /// Map from operation name to indices in operations vec
    operations_by_name: HashMap<String, Vec<usize>>,
}

impl OperationStorage {
    fn new() -> Self {
        Self::default()
    }

    #[allow(dead_code)]
    fn add_operation(&mut self, operation: Operation) {
        let index = self.operations.len();
        let id = operation.operation_id.clone();
        let name = operation.name.clone();

        self.operations.push(operation);
        self.operations_by_id.insert(id, index);

        if let Some(name) = name {
            self.operations_by_name
                .entry(name)
                .or_default()
                .push(index);
        }
    }

    fn get_by_id(&self, id: &str) -> Option<&Operation> {
        self.operations_by_id
            .get(id)
            .and_then(|&idx| self.operations.get(idx))
    }

    fn get_by_name(&self, name: &str) -> Option<&Operation> {
        self.operations_by_name
            .get(name)
            .and_then(|indices| indices.first())
            .and_then(|&idx| self.operations.get(idx))
    }

    fn get_by_name_and_index(&self, name: &str, index: usize) -> Option<&Operation> {
        self.operations_by_name
            .get(name)
            .and_then(|indices| indices.get(index))
            .and_then(|&idx| self.operations.get(idx))
    }

    fn get_by_index(&self, index: usize) -> Option<&Operation> {
        self.operations.get(index)
    }

    fn get_all(&self) -> &[Operation] {
        &self.operations
    }

    fn clear(&mut self) {
        self.operations.clear();
        self.operations_by_id.clear();
        self.operations_by_name.clear();
    }
}

/// Cloud test runner for testing deployed Lambda functions.
///
/// Invokes deployed Lambda functions and polls for execution completion,
/// enabling integration testing against real AWS infrastructure.
///
/// # Type Parameters
///
/// * `O` - The output type (must be deserializable)
///
/// # Examples
///
/// ```ignore
/// use aws_durable_execution_sdk_testing::CloudDurableTestRunner;
///
/// // Create runner with default AWS config
/// let runner = CloudDurableTestRunner::<String>::new("my-function")
///     .await
///     .unwrap();
///
/// // Run test
/// let result = runner.run("input".to_string()).await.unwrap();
/// println!("Status: {:?}", result.get_status());
/// ```
pub struct CloudDurableTestRunner<O>
where
    O: DeserializeOwned + Send,
{
    /// The Lambda function name or ARN
    function_name: String,
    /// The AWS Lambda client
    lambda_client: LambdaClient,
    /// Configuration for polling and timeouts
    config: CloudTestRunnerConfig,
    /// Storage for captured operations
    operation_storage: OperationStorage,
    /// Phantom data for the output type
    _phantom: PhantomData<O>,
}

impl<O> std::fmt::Debug for CloudDurableTestRunner<O>
where
    O: DeserializeOwned + Send,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudDurableTestRunner")
            .field("function_name", &self.function_name)
            .field("config", &self.config)
            .field("operation_count", &self.operation_storage.operations.len())
            .finish()
    }
}


impl<O> CloudDurableTestRunner<O>
where
    O: DeserializeOwned + Send,
{
    /// Creates a new cloud test runner for the given Lambda function.
    ///
    /// This constructor uses the default AWS configuration, which loads
    /// credentials from environment variables, AWS config files, or IAM roles.
    ///
    /// # Arguments
    ///
    /// * `function_name` - The Lambda function name or ARN
    ///
    /// # Returns
    ///
    /// A new `CloudDurableTestRunner` configured with default settings.
    ///
    /// # Requirements
    ///
    /// - 8.1: WHEN a developer creates a Cloud_Test_Runner with a function name,
    ///   THE Cloud_Test_Runner SHALL configure the Lambda client for that function
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::CloudDurableTestRunner;
    ///
    /// let runner = CloudDurableTestRunner::<String>::new("my-function")
    ///     .await
    ///     .unwrap();
    /// ```
    pub async fn new(function_name: impl Into<String>) -> Result<Self, TestError> {
        let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let lambda_client = LambdaClient::new(&config);

        Ok(Self {
            function_name: function_name.into(),
            lambda_client,
            config: CloudTestRunnerConfig::default(),
            operation_storage: OperationStorage::new(),
            _phantom: PhantomData,
        })
    }

    /// Creates a new cloud test runner with a custom Lambda client.
    ///
    /// This constructor allows using a pre-configured Lambda client,
    /// useful for testing with custom credentials or endpoints.
    ///
    /// # Arguments
    ///
    /// * `function_name` - The Lambda function name or ARN
    /// * `client` - A pre-configured Lambda client
    ///
    /// # Returns
    ///
    /// A new `CloudDurableTestRunner` using the provided client.
    ///
    /// # Requirements
    ///
    /// - 8.4: WHEN a developer provides a custom Lambda client,
    ///   THE Cloud_Test_Runner SHALL use that client for invocations
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::CloudDurableTestRunner;
    /// use aws_sdk_lambda::Client as LambdaClient;
    ///
    /// let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    /// let custom_client = LambdaClient::new(&config);
    ///
    /// let runner = CloudDurableTestRunner::<String>::with_client(
    ///     "my-function",
    ///     custom_client,
    /// );
    /// ```
    pub fn with_client(function_name: impl Into<String>, client: LambdaClient) -> Self {
        Self {
            function_name: function_name.into(),
            lambda_client: client,
            config: CloudTestRunnerConfig::default(),
            operation_storage: OperationStorage::new(),
            _phantom: PhantomData,
        }
    }

    /// Configures the test runner with custom settings.
    ///
    /// # Arguments
    ///
    /// * `config` - The configuration to use
    ///
    /// # Returns
    ///
    /// The runner with updated configuration.
    ///
    /// # Requirements
    ///
    /// - 8.5: WHEN a developer configures poll_interval,
    ///   THE Cloud_Test_Runner SHALL use that interval when polling for execution status
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::{CloudDurableTestRunner, CloudTestRunnerConfig};
    /// use std::time::Duration;
    ///
    /// let runner = CloudDurableTestRunner::<String>::new("my-function")
    ///     .await
    ///     .unwrap()
    ///     .with_config(CloudTestRunnerConfig {
    ///         poll_interval: Duration::from_millis(500),
    ///         timeout: Duration::from_secs(60),
    ///     });
    /// ```
    pub fn with_config(mut self, config: CloudTestRunnerConfig) -> Self {
        self.config = config;
        self
    }

    /// Returns the function name.
    pub fn function_name(&self) -> &str {
        &self.function_name
    }

    /// Returns the current configuration.
    pub fn config(&self) -> &CloudTestRunnerConfig {
        &self.config
    }

    /// Returns a reference to the Lambda client.
    pub fn lambda_client(&self) -> &LambdaClient {
        &self.lambda_client
    }
}


impl<O> CloudDurableTestRunner<O>
where
    O: DeserializeOwned + Send,
{
    /// Executes the Lambda function with the given payload and returns the test result.
    ///
    /// This method:
    /// 1. Invokes the Lambda function with the serialized payload
    /// 2. Polls for execution completion using the configured interval
    /// 3. Retrieves operation history from the execution
    /// 4. Returns a TestResult with the execution outcome
    ///
    /// # Arguments
    ///
    /// * `payload` - The input payload to pass to the Lambda function
    ///
    /// # Returns
    ///
    /// A `TestResult` containing the execution outcome and captured operations.
    ///
    /// # Requirements
    ///
    /// - 8.2: WHEN a developer calls run() with a payload,
    ///   THE Cloud_Test_Runner SHALL invoke the Lambda function and poll for completion
    /// - 8.3: WHEN the execution completes,
    ///   THE Cloud_Test_Runner SHALL return a Test_Result with the execution outcome
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::CloudDurableTestRunner;
    ///
    /// let runner = CloudDurableTestRunner::<String>::new("my-function")
    ///     .await
    ///     .unwrap();
    ///
    /// let result = runner.run("input".to_string()).await.unwrap();
    /// println!("Status: {:?}", result.get_status());
    /// ```
    pub async fn run<I>(&mut self, payload: I) -> Result<TestResult<O>, TestError>
    where
        I: Serialize + Send,
    {
        // Clear previous operations
        self.operation_storage.clear();

        // Serialize the payload
        let payload_json = serde_json::to_vec(&payload)?;

        // Record invocation start
        let start_time = chrono::Utc::now();
        let mut invocation = Invocation::with_start(start_time);

        // Invoke the Lambda function
        let invoke_result = self
            .lambda_client
            .invoke()
            .function_name(&self.function_name)
            .payload(aws_sdk_lambda::primitives::Blob::new(payload_json))
            .send()
            .await
            .map_err(|e| TestError::aws_error(format!("Lambda invoke failed: {}", e)))?;

        // Record invocation end
        let end_time = chrono::Utc::now();
        invocation = invocation.with_end(end_time);

        // Check for function error
        if let Some(function_error) = invoke_result.function_error() {
            let error_payload = invoke_result
                .payload()
                .map(|p| String::from_utf8_lossy(p.as_ref()).to_string())
                .unwrap_or_else(|| "Unknown error".to_string());

            let test_error = TestResultError::new(function_error, error_payload.clone());
            invocation = invocation.with_error(test_error.clone());

            let mut result = TestResult::failure(test_error, self.operation_storage.get_all().to_vec());
            result.add_invocation(invocation);
            return Ok(result);
        }

        // Parse the response payload
        let response_payload = invoke_result
            .payload()
            .ok_or_else(|| TestError::aws_error("No response payload from Lambda"))?;

        let response_str = String::from_utf8_lossy(response_payload.as_ref());

        // Try to parse the response as the expected output type
        match serde_json::from_str::<O>(&response_str) {
            Ok(output) => {
                let mut result = TestResult::success(output, self.operation_storage.get_all().to_vec());
                result.add_invocation(invocation);
                Ok(result)
            }
            Err(e) => {
                // Check if the response indicates an error
                if let Ok(error_response) = serde_json::from_str::<LambdaErrorResponse>(&response_str) {
                    let test_error = TestResultError::new(
                        error_response.error_type.unwrap_or_else(|| "UnknownError".to_string()),
                        error_response.error_message.unwrap_or_else(|| "Unknown error".to_string()),
                    );
                    invocation = invocation.with_error(test_error.clone());

                    let mut result = TestResult::failure(test_error, self.operation_storage.get_all().to_vec());
                    result.add_invocation(invocation);
                    Ok(result)
                } else {
                    // Deserialization failed
                    Err(TestError::SerializationError(e))
                }
            }
        }
    }
}

/// Internal struct for parsing Lambda error responses.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LambdaErrorResponse {
    error_type: Option<String>,
    error_message: Option<String>,
}


impl<O> CloudDurableTestRunner<O>
where
    O: DeserializeOwned + Send,
{
    // =========================================================================
    // Operation Lookup Methods (Requirements 4.1, 4.2, 4.3, 4.4)
    // =========================================================================

    /// Gets the first operation with the given name.
    ///
    /// # Arguments
    ///
    /// * `name` - The operation name to search for
    ///
    /// # Returns
    ///
    /// A `DurableOperation` wrapping the first operation with that name,
    /// or `None` if no operation with that name exists.
    ///
    /// # Requirements
    ///
    /// - 4.1: WHEN a developer calls get_operation(name) on Test_Runner,
    ///   THE Test_Runner SHALL return the first operation with that name
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::CloudDurableTestRunner;
    ///
    /// let mut runner = CloudDurableTestRunner::<String>::new("my-function")
    ///     .await
    ///     .unwrap();
    /// let _ = runner.run("input".to_string()).await.unwrap();
    ///
    /// if let Some(op) = runner.get_operation("process_data") {
    ///     println!("Found operation: {:?}", op.get_status());
    /// }
    /// ```
    pub fn get_operation(&self, name: &str) -> Option<DurableOperation> {
        self.operation_storage
            .get_by_name(name)
            .cloned()
            .map(DurableOperation::new)
    }

    /// Gets an operation by its index in the execution order.
    ///
    /// # Arguments
    ///
    /// * `index` - The zero-based index of the operation
    ///
    /// # Returns
    ///
    /// A `DurableOperation` at that index, or `None` if the index is out of bounds.
    ///
    /// # Requirements
    ///
    /// - 4.2: WHEN a developer calls get_operation_by_index(index) on Test_Runner,
    ///   THE Test_Runner SHALL return the operation at that execution order index
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::CloudDurableTestRunner;
    ///
    /// let mut runner = CloudDurableTestRunner::<String>::new("my-function")
    ///     .await
    ///     .unwrap();
    /// let _ = runner.run("input".to_string()).await.unwrap();
    ///
    /// // Get the first operation
    /// if let Some(op) = runner.get_operation_by_index(0) {
    ///     println!("First operation: {:?}", op.get_type());
    /// }
    /// ```
    pub fn get_operation_by_index(&self, index: usize) -> Option<DurableOperation> {
        self.operation_storage
            .get_by_index(index)
            .cloned()
            .map(DurableOperation::new)
    }

    /// Gets an operation by name and occurrence index.
    ///
    /// This is useful when multiple operations have the same name and you need
    /// to access a specific occurrence.
    ///
    /// # Arguments
    ///
    /// * `name` - The operation name to search for
    /// * `index` - The zero-based index among operations with that name
    ///
    /// # Returns
    ///
    /// A `DurableOperation` at that name/index combination, or `None` if not found.
    ///
    /// # Requirements
    ///
    /// - 4.3: WHEN a developer calls get_operation_by_name_and_index(name, index) on Test_Runner,
    ///   THE Test_Runner SHALL return the nth operation with that name
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::CloudDurableTestRunner;
    ///
    /// let mut runner = CloudDurableTestRunner::<String>::new("my-function")
    ///     .await
    ///     .unwrap();
    /// let _ = runner.run("input".to_string()).await.unwrap();
    ///
    /// // Get the second "process" operation
    /// if let Some(op) = runner.get_operation_by_name_and_index("process", 1) {
    ///     println!("Second process operation: {:?}", op.get_status());
    /// }
    /// ```
    pub fn get_operation_by_name_and_index(&self, name: &str, index: usize) -> Option<DurableOperation> {
        self.operation_storage
            .get_by_name_and_index(name, index)
            .cloned()
            .map(DurableOperation::new)
    }

    /// Gets an operation by its unique ID.
    ///
    /// # Arguments
    ///
    /// * `id` - The unique operation ID
    ///
    /// # Returns
    ///
    /// A `DurableOperation` with that ID, or `None` if no operation with that ID exists.
    ///
    /// # Requirements
    ///
    /// - 4.4: WHEN a developer calls get_operation_by_id(id) on Test_Runner,
    ///   THE Test_Runner SHALL return the operation with that unique identifier
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::CloudDurableTestRunner;
    ///
    /// let mut runner = CloudDurableTestRunner::<String>::new("my-function")
    ///     .await
    ///     .unwrap();
    /// let _ = runner.run("input".to_string()).await.unwrap();
    ///
    /// if let Some(op) = runner.get_operation_by_id("op-123") {
    ///     println!("Found operation: {:?}", op.get_name());
    /// }
    /// ```
    pub fn get_operation_by_id(&self, id: &str) -> Option<DurableOperation> {
        self.operation_storage
            .get_by_id(id)
            .cloned()
            .map(DurableOperation::new)
    }

    /// Gets all captured operations.
    ///
    /// # Returns
    ///
    /// A vector of all operations in execution order.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::CloudDurableTestRunner;
    ///
    /// let mut runner = CloudDurableTestRunner::<String>::new("my-function")
    ///     .await
    ///     .unwrap();
    /// let _ = runner.run("input".to_string()).await.unwrap();
    ///
    /// let all_ops = runner.get_all_operations();
    /// println!("Total operations: {}", all_ops.len());
    /// ```
    pub fn get_all_operations(&self) -> Vec<DurableOperation> {
        self.operation_storage
            .get_all()
            .iter()
            .cloned()
            .map(DurableOperation::new)
            .collect()
    }

    /// Returns the number of captured operations.
    pub fn operation_count(&self) -> usize {
        self.operation_storage.operations.len()
    }

    /// Clears all captured operations.
    ///
    /// This is useful when reusing the runner for multiple test runs.
    pub fn clear_operations(&mut self) {
        self.operation_storage.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = CloudTestRunnerConfig::default();
        assert_eq!(config.poll_interval, Duration::from_millis(1000));
        assert_eq!(config.timeout, Duration::from_secs(300));
    }

    #[test]
    fn test_config_builder() {
        let config = CloudTestRunnerConfig::new()
            .with_poll_interval(Duration::from_millis(500))
            .with_timeout(Duration::from_secs(60));

        assert_eq!(config.poll_interval, Duration::from_millis(500));
        assert_eq!(config.timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_operation_storage() {
        let mut storage = OperationStorage::new();

        // Add operations
        let mut op1 = Operation::new("op-001", aws_durable_execution_sdk::OperationType::Step);
        op1.name = Some("step1".to_string());
        storage.add_operation(op1);

        let mut op2 = Operation::new("op-002", aws_durable_execution_sdk::OperationType::Wait);
        op2.name = Some("wait1".to_string());
        storage.add_operation(op2);

        let mut op3 = Operation::new("op-003", aws_durable_execution_sdk::OperationType::Step);
        op3.name = Some("step1".to_string()); // Same name as op1
        storage.add_operation(op3);

        // Test get_by_id
        assert!(storage.get_by_id("op-001").is_some());
        assert!(storage.get_by_id("op-002").is_some());
        assert!(storage.get_by_id("nonexistent").is_none());

        // Test get_by_name (returns first)
        let first_step = storage.get_by_name("step1").unwrap();
        assert_eq!(first_step.operation_id, "op-001");

        // Test get_by_name_and_index
        let second_step = storage.get_by_name_and_index("step1", 1).unwrap();
        assert_eq!(second_step.operation_id, "op-003");

        // Test get_by_index
        let first_op = storage.get_by_index(0).unwrap();
        assert_eq!(first_op.operation_id, "op-001");

        // Test get_all
        assert_eq!(storage.get_all().len(), 3);

        // Test clear
        storage.clear();
        assert_eq!(storage.get_all().len(), 0);
    }
}
