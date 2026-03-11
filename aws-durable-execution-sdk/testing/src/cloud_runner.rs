//! Cloud test runner for testing deployed Lambda functions.
//!
//! This module provides the `CloudDurableTestRunner` for testing durable functions
//! deployed to AWS Lambda, enabling integration testing against real AWS infrastructure.
//!
//! # Examples
//!
//! ```ignore
//! use durable_execution_sdk_testing::{
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
use std::sync::Arc;
use std::time::{Duration, Instant};

use aws_sdk_lambda::Client as LambdaClient;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::sync::RwLock;

use crate::error::TestError;
use crate::history_poller::{HistoryApiClient, HistoryPage, HistoryPoller};
use crate::operation::{CallbackSender, DurableOperation};
use crate::operation_handle::{OperationHandle, OperationMatcher};
use crate::test_result::TestResult;
use crate::types::{ExecutionStatus, TestResultError};
use durable_execution_sdk::{
    DurableServiceClient, LambdaDurableServiceClient, Operation, OperationStatus, OperationType,
};

/// Configuration for the cloud test runner.
///
/// Controls polling behavior and timeouts when testing deployed Lambda functions.
///
/// # Examples
///
/// ```
/// use durable_execution_sdk_testing::CloudTestRunnerConfig;
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
            self.operations_by_name.entry(name).or_default().push(index);
        }
    }

    /// If an operation with the same `operation_id` already exists, update it
    /// in place; otherwise append it via `add_operation`.
    #[allow(dead_code)]
    pub(crate) fn add_or_update(&mut self, operation: Operation) {
        if let Some(&idx) = self.operations_by_id.get(&operation.operation_id) {
            self.operations[idx] = operation;
        } else {
            self.add_operation(operation);
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

/// Real implementation of [`HistoryApiClient`] that calls the durable execution
/// state API via a [`LambdaDurableServiceClient`].
///
/// Wraps a `LambdaClient` and creates an internal service client to make
/// signed HTTP calls to the `GetDurableExecutionHistory` API endpoint.
pub struct LambdaHistoryApiClient {
    service_client: LambdaDurableServiceClient,
}

impl LambdaHistoryApiClient {
    /// Creates a new `LambdaHistoryApiClient` from an AWS SDK config.
    ///
    /// The config is used to construct a `LambdaDurableServiceClient` that
    /// makes signed HTTP calls to the durable execution state API.
    ///
    /// # Arguments
    ///
    /// * `aws_config` - The AWS SDK configuration (same one used to create the `LambdaClient`)
    pub fn from_aws_config(aws_config: &aws_config::SdkConfig) -> Result<Self, TestError> {
        Ok(Self {
            service_client: LambdaDurableServiceClient::from_aws_config(aws_config).map_err(
                |e| TestError::aws_error(format!("Failed to create service client: {}", e)),
            )?,
        })
    }

    /// Creates a new `LambdaHistoryApiClient` from an existing `LambdaDurableServiceClient`.
    ///
    /// Useful when a service client is already available.
    pub fn from_service_client(service_client: LambdaDurableServiceClient) -> Self {
        Self { service_client }
    }

    /// Maps an [`OperationStatus`] to an [`ExecutionStatus`] for terminal detection.
    fn map_terminal_status(status: &OperationStatus) -> Option<ExecutionStatus> {
        match status {
            OperationStatus::Succeeded => Some(ExecutionStatus::Succeeded),
            OperationStatus::Failed => Some(ExecutionStatus::Failed),
            OperationStatus::Cancelled => Some(ExecutionStatus::Cancelled),
            OperationStatus::TimedOut => Some(ExecutionStatus::TimedOut),
            _ => None,
        }
    }
}

#[async_trait::async_trait]
impl HistoryApiClient for LambdaHistoryApiClient {
    /// Retrieves a single page of execution history by calling the durable execution state API.
    ///
    /// Detects terminal state by examining EXECUTION-type operations: when an execution
    /// operation has a terminal status (Succeeded, Failed, Cancelled, TimedOut), the page
    /// is marked as terminal with the corresponding status, result, and error.
    async fn get_history(&self, arn: &str, marker: Option<&str>) -> Result<HistoryPage, TestError> {
        let marker_str = marker.unwrap_or("");

        let response = self
            .service_client
            .get_operations(arn, marker_str)
            .await
            .map_err(|e| {
                TestError::aws_error(format!("GetDurableExecutionHistory failed: {}", e))
            })?;

        // Detect terminal state from EXECUTION-type operations
        let mut is_terminal = false;
        let mut terminal_status = None;
        let mut terminal_result = None;
        let mut terminal_error = None;

        for op in &response.operations {
            if op.operation_type == OperationType::Execution {
                if let Some(exec_status) = Self::map_terminal_status(&op.status) {
                    is_terminal = true;
                    terminal_status = Some(exec_status);
                    terminal_result = op.result.clone();
                    if let Some(ref err) = op.error {
                        terminal_error =
                            Some(TestResultError::new(&err.error_type, &err.error_message));
                    }
                    break;
                }
            }
        }

        Ok(HistoryPage {
            events: Vec::new(), // The state API returns operations, not raw history events
            operations: response.operations,
            next_marker: response.next_marker,
            is_terminal,
            terminal_status,
            terminal_result,
            terminal_error,
        })
    }
}

/// Sends callback signals (success, failure, heartbeat) to a durable execution
/// via the AWS Lambda API.
///
/// This bridges the [`CallbackSender`] trait (used by [`OperationHandle`]) with
/// the Lambda durable execution callback APIs, enabling handles to send callback
/// responses during cloud test execution.
pub(crate) struct CloudCallbackSender {
    /// The AWS Lambda client used to send callback API calls
    client: LambdaClient,
    /// The ARN of the durable execution to send callbacks to
    _durable_execution_arn: String,
}

impl CloudCallbackSender {
    /// Creates a new `CloudCallbackSender`.
    ///
    /// # Arguments
    ///
    /// * `client` - The AWS Lambda client
    /// * `durable_execution_arn` - The ARN of the durable execution
    pub fn new(client: LambdaClient, durable_execution_arn: String) -> Self {
        Self {
            client,
            _durable_execution_arn: durable_execution_arn,
        }
    }
}

#[async_trait::async_trait]
impl CallbackSender for CloudCallbackSender {
    /// Sends a success response for a callback operation.
    ///
    /// Calls the `SendDurableExecutionCallbackSuccess` Lambda API with the
    /// callback ID and result payload.
    ///
    /// # Arguments
    ///
    /// * `callback_id` - The unique callback identifier
    /// * `result` - The success result payload (serialized as bytes)
    async fn send_success(&self, callback_id: &str, result: &str) -> Result<(), TestError> {
        self.client
            .send_durable_execution_callback_success()
            .callback_id(callback_id)
            .result(aws_sdk_lambda::primitives::Blob::new(result.as_bytes()))
            .send()
            .await
            .map_err(|e| {
                TestError::aws_error(format!(
                    "SendDurableExecutionCallbackSuccess failed for callback '{}': {}",
                    callback_id, e
                ))
            })?;
        Ok(())
    }

    /// Sends a failure response for a callback operation.
    ///
    /// Calls the `SendDurableExecutionCallbackFailure` Lambda API with the
    /// callback ID and error details.
    ///
    /// # Arguments
    ///
    /// * `callback_id` - The unique callback identifier
    /// * `error` - The error information to send
    async fn send_failure(
        &self,
        callback_id: &str,
        error: &TestResultError,
    ) -> Result<(), TestError> {
        let error_object = aws_sdk_lambda::types::ErrorObject::builder()
            .set_error_type(error.error_type.clone())
            .set_error_message(error.error_message.clone())
            .build();

        self.client
            .send_durable_execution_callback_failure()
            .callback_id(callback_id)
            .error(error_object)
            .send()
            .await
            .map_err(|e| {
                TestError::aws_error(format!(
                    "SendDurableExecutionCallbackFailure failed for callback '{}': {}",
                    callback_id, e
                ))
            })?;
        Ok(())
    }

    /// Sends a heartbeat for a callback operation.
    ///
    /// Calls the `SendDurableExecutionCallbackHeartbeat` Lambda API with the
    /// callback ID to keep the callback active.
    ///
    /// # Arguments
    ///
    /// * `callback_id` - The unique callback identifier
    async fn send_heartbeat(&self, callback_id: &str) -> Result<(), TestError> {
        self.client
            .send_durable_execution_callback_heartbeat()
            .callback_id(callback_id)
            .send()
            .await
            .map_err(|e| {
                TestError::aws_error(format!(
                    "SendDurableExecutionCallbackHeartbeat failed for callback '{}': {}",
                    callback_id, e
                ))
            })?;
        Ok(())
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
/// use durable_execution_sdk_testing::CloudDurableTestRunner;
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
    /// The AWS SDK configuration (stored for creating service clients during run)
    aws_config: Option<aws_config::SdkConfig>,
    /// Configuration for polling and timeouts
    config: CloudTestRunnerConfig,
    /// Storage for captured operations
    operation_storage: OperationStorage,
    /// Pre-registered operation handles for lazy population during execution
    handles: Vec<OperationHandle>,
    /// Shared operations list for child operation enumeration across handles
    all_operations: Arc<RwLock<Vec<Operation>>>,
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
            .field("handle_count", &self.handles.len())
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
    /// # Examples
    ///
    /// ```ignore
    /// use durable_execution_sdk_testing::CloudDurableTestRunner;
    ///
    /// let runner = CloudDurableTestRunner::<String>::new("my-function")
    ///     .await
    ///     .unwrap();
    /// ```
    pub async fn new(function_name: impl Into<String>) -> Result<Self, TestError> {
        let aws_cfg = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let lambda_client = LambdaClient::new(&aws_cfg);

        Ok(Self {
            function_name: function_name.into(),
            lambda_client,
            aws_config: Some(aws_cfg),
            config: CloudTestRunnerConfig::default(),
            operation_storage: OperationStorage::new(),
            handles: Vec::new(),
            all_operations: Arc::new(RwLock::new(Vec::new())),
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
    /// # Examples
    ///
    /// ```ignore
    /// use durable_execution_sdk_testing::CloudDurableTestRunner;
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
            aws_config: None,
            config: CloudTestRunnerConfig::default(),
            operation_storage: OperationStorage::new(),
            handles: Vec::new(),
            all_operations: Arc::new(RwLock::new(Vec::new())),
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
    /// # Examples
    ///
    /// ```ignore
    /// use durable_execution_sdk_testing::{CloudDurableTestRunner, CloudTestRunnerConfig};
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
    /// Runs the durable function and polls for execution completion.
    ///
    /// This method invokes the Lambda function, then polls the
    /// `GetDurableExecutionHistory` API until the execution reaches a terminal
    /// state or the configured timeout elapses. During polling, operations are
    /// stored in `OperationStorage` and waiting `OperationHandle` instances are
    /// notified.
    ///
    /// # Arguments
    ///
    /// * `payload` - The input payload to send to the Lambda function
    ///
    /// # Returns
    ///
    /// A `TestResult` reflecting the full execution outcome, including all
    /// operations and history events collected during polling.
    /// # Examples
    ///
    /// ```ignore
    /// use durable_execution_sdk_testing::CloudDurableTestRunner;
    ///
    /// let mut runner = CloudDurableTestRunner::<String>::new("my-function")
    ///     .await
    ///     .unwrap();
    ///
    /// let result = runner.run("input").await.unwrap();
    /// println!("Status: {:?}", result.get_status());
    /// ```
    pub async fn run<I>(&mut self, payload: I) -> Result<TestResult<O>, TestError>
    where
        I: Serialize + Send,
    {
        // Requirement 3.4: Clear storage at start of each run
        self.operation_storage.clear();

        // Requirement 8.1, 8.4: Invoke Lambda and extract ARN
        let arn = self.invoke_lambda(&payload).await?;

        // Requirement 1.1: Create HistoryPoller with the ARN
        let history_client = self.create_history_client()?;
        let mut poller = HistoryPoller::new(history_client, arn.clone(), self.config.poll_interval);

        // Requirement 6.4: Create CloudCallbackSender and configure handles
        let callback_sender: Arc<dyn CallbackSender> = Arc::new(CloudCallbackSender::new(
            self.lambda_client.clone(),
            arn.clone(),
        ));
        for handle in &self.handles {
            let mut sender = handle.callback_sender.write().await;
            *sender = Some(callback_sender.clone());
        }

        // Requirement 7.2: Use configured timeout
        let deadline = Instant::now() + self.config.timeout;
        let mut all_events = Vec::new();

        loop {
            // Requirement 1.4: Check timeout
            if Instant::now() >= deadline {
                return Ok(TestResult::with_status(
                    ExecutionStatus::TimedOut,
                    self.operation_storage.get_all().to_vec(),
                ));
            }

            // Requirement 7.1: Wait poll_interval between cycles
            tokio::time::sleep(self.config.poll_interval).await;

            let poll_result = poller.poll_once().await?;

            // Requirement 3.1: Populate OperationStorage (deduplicated)
            for op in &poll_result.operations {
                self.operation_storage.add_or_update(op.clone());
            }

            // Requirement 5.5: Notify waiting OperationHandles
            self.notify_handles().await;

            // Requirement 9.1: Collect history events
            all_events.extend(poll_result.events);

            // Requirement 1.2, 1.5: Check terminal state
            if let Some(terminal) = poll_result.terminal {
                let mut result = match terminal.status {
                    ExecutionStatus::Succeeded => {
                        // Requirement 8.2: Parse result from terminal event
                        let output: O =
                            serde_json::from_str(terminal.result.as_deref().unwrap_or("null"))?;
                        TestResult::success(output, self.operation_storage.get_all().to_vec())
                    }
                    status => {
                        // Requirement 8.3: Parse error from terminal event
                        let mut r = TestResult::with_status(
                            status,
                            self.operation_storage.get_all().to_vec(),
                        );
                        if let Some(err) = terminal.error {
                            r.set_error(err);
                        }
                        r
                    }
                };
                // Requirement 9.1, 9.2, 9.3: Include all history events
                result.set_history_events(all_events);
                return Ok(result);
            }
        }
    }

    /// Invokes the Lambda function and extracts the `DurableExecutionArn` from the response.
    async fn invoke_lambda<I: Serialize>(&self, payload: &I) -> Result<String, TestError> {
        let payload_json = serde_json::to_vec(payload)?;

        let invoke_result = self
            .lambda_client
            .invoke()
            .function_name(&self.function_name)
            .payload(aws_sdk_lambda::primitives::Blob::new(payload_json))
            .send()
            .await
            .map_err(|e| TestError::aws_error(format!("Lambda invoke failed: {}", e)))?;

        // Check for function error
        if let Some(function_error) = invoke_result.function_error() {
            let error_payload = invoke_result
                .payload()
                .map(|p| String::from_utf8_lossy(p.as_ref()).to_string())
                .unwrap_or_else(|| "Unknown error".to_string());

            return Err(TestError::aws_error(format!(
                "Lambda function error ({}): {}",
                function_error, error_payload
            )));
        }

        // Parse the response to extract DurableExecutionArn
        let response_payload = invoke_result
            .payload()
            .ok_or_else(|| TestError::aws_error("No response payload from Lambda"))?;

        let response_str = String::from_utf8_lossy(response_payload.as_ref());

        // Try to parse as JSON and extract the ARN
        let response_json: serde_json::Value = serde_json::from_str(&response_str)
            .map_err(|e| TestError::aws_error(format!("Failed to parse Lambda response: {}", e)))?;

        let arn = response_json
            .get("DurableExecutionArn")
            .or_else(|| response_json.get("durableExecutionArn"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                TestError::aws_error(format!(
                    "Lambda response missing DurableExecutionArn: {}",
                    response_str
                ))
            })?;

        Ok(arn.to_string())
    }

    /// Creates a `LambdaHistoryApiClient` from the stored AWS config.
    fn create_history_client(&self) -> Result<LambdaHistoryApiClient, TestError> {
        match &self.aws_config {
            Some(cfg) => LambdaHistoryApiClient::from_aws_config(cfg),
            None => {
                // Fallback: create a service client from a default config.
                // This path is used when with_client() was called without an SdkConfig.
                let service_client = LambdaDurableServiceClient::from_aws_config(
                    &aws_config::SdkConfig::builder().build(),
                )
                .map_err(|e| {
                    TestError::aws_error(format!("Failed to create service client: {}", e))
                })?;
                Ok(LambdaHistoryApiClient::from_service_client(service_client))
            }
        }
    }
}

impl<O> CloudDurableTestRunner<O>
where
    O: DeserializeOwned + Send,
{
    // =========================================================================
    // Operation Handle Methods (Requirements 5.1, 5.2, 5.3, 5.4)
    // =========================================================================

    /// Returns a lazy `OperationHandle` that populates with the first operation
    /// matching the given name during execution.
    ///
    /// # Arguments
    ///
    /// * `name` - The operation name to match against
    /// # Examples
    ///
    /// ```ignore
    /// let handle = runner.get_operation_handle("my-callback");
    /// // handle is unpopulated until run() executes and produces a matching operation
    /// ```
    pub fn get_operation_handle(&mut self, name: &str) -> OperationHandle {
        let handle = OperationHandle::new(
            OperationMatcher::ByName(name.to_string()),
            self.all_operations.clone(),
        );
        self.handles.push(handle.clone());
        handle
    }

    /// Returns a lazy `OperationHandle` that populates with the operation
    /// at the given execution order index.
    ///
    /// # Arguments
    ///
    /// * `index` - The zero-based execution order index
    /// # Examples
    ///
    /// ```ignore
    /// let handle = runner.get_operation_handle_by_index(0);
    /// // handle populates with the first operation created during execution
    /// ```
    pub fn get_operation_handle_by_index(&mut self, index: usize) -> OperationHandle {
        let handle = OperationHandle::new(
            OperationMatcher::ByIndex(index),
            self.all_operations.clone(),
        );
        self.handles.push(handle.clone());
        handle
    }

    /// Returns a lazy `OperationHandle` that populates with the nth operation
    /// matching the given name during execution.
    ///
    /// # Arguments
    ///
    /// * `name` - The operation name to match against
    /// * `index` - The zero-based index among operations with that name
    /// # Examples
    ///
    /// ```ignore
    /// let handle = runner.get_operation_handle_by_name_and_index("process", 1);
    /// // handle populates with the second "process" operation during execution
    /// ```
    pub fn get_operation_handle_by_name_and_index(
        &mut self,
        name: &str,
        index: usize,
    ) -> OperationHandle {
        let handle = OperationHandle::new(
            OperationMatcher::ByNameAndIndex(name.to_string(), index),
            self.all_operations.clone(),
        );
        self.handles.push(handle.clone());
        handle
    }

    /// Returns a lazy `OperationHandle` that populates with the operation
    /// matching the given unique ID.
    ///
    /// # Arguments
    ///
    /// * `id` - The unique operation ID to match against
    /// # Examples
    ///
    /// ```ignore
    /// let handle = runner.get_operation_handle_by_id("op-abc-123");
    /// // handle populates with the operation whose ID matches during execution
    /// ```
    pub fn get_operation_handle_by_id(&mut self, id: &str) -> OperationHandle {
        let handle = OperationHandle::new(
            OperationMatcher::ById(id.to_string()),
            self.all_operations.clone(),
        );
        self.handles.push(handle.clone());
        handle
    }

    /// Notifies all registered operation handles with matching operation data
    /// from the operation storage, and updates the shared `all_operations` list.
    ///
    /// All matched operations are collected first, then the shared `all_operations`
    /// list and individual handles are updated together to ensure consistency.
    pub(crate) async fn notify_handles(&self) {
        // Phase 1: Collect all matched operations (no async locks held)
        let matched: Vec<_> = self
            .handles
            .iter()
            .map(|handle| {
                let matched_op = match &handle.matcher {
                    OperationMatcher::ByName(name) => {
                        self.operation_storage.get_by_name(name).cloned()
                    }
                    OperationMatcher::ByIndex(idx) => {
                        self.operation_storage.get_by_index(*idx).cloned()
                    }
                    OperationMatcher::ById(id) => self.operation_storage.get_by_id(id).cloned(),
                    OperationMatcher::ByNameAndIndex(name, idx) => self
                        .operation_storage
                        .get_by_name_and_index(name, *idx)
                        .cloned(),
                };
                (handle, matched_op)
            })
            .collect();

        // Phase 2: Update shared all_operations first, then handles atomically
        let mut all_ops = self.all_operations.write().await;
        *all_ops = self.operation_storage.get_all().to_vec();
        drop(all_ops);

        // Phase 3: Update handles and send notifications
        for (handle, matched_op) in matched {
            if let Some(op) = matched_op {
                let status = op.status;
                let mut inner = handle.inner.write().await;
                *inner = Some(op);
                drop(inner);
                let _ = handle.status_tx.send(Some(status));
            }
        }
    }
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
    /// # Examples
    ///
    /// ```ignore
    /// use durable_execution_sdk_testing::CloudDurableTestRunner;
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
        let all_ops = self.cached_all_operations();
        self.operation_storage
            .get_by_name(name)
            .cloned()
            .map(|op| DurableOperation::new(op).with_operations(Arc::clone(&all_ops)))
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
    /// # Examples
    ///
    /// ```ignore
    /// use durable_execution_sdk_testing::CloudDurableTestRunner;
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
        let all_ops = self.cached_all_operations();
        self.operation_storage
            .get_by_index(index)
            .cloned()
            .map(|op| DurableOperation::new(op).with_operations(Arc::clone(&all_ops)))
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
    /// # Examples
    ///
    /// ```ignore
    /// use durable_execution_sdk_testing::CloudDurableTestRunner;
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
    pub fn get_operation_by_name_and_index(
        &self,
        name: &str,
        index: usize,
    ) -> Option<DurableOperation> {
        let all_ops = self.cached_all_operations();
        self.operation_storage
            .get_by_name_and_index(name, index)
            .cloned()
            .map(|op| DurableOperation::new(op).with_operations(Arc::clone(&all_ops)))
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
    /// # Examples
    ///
    /// ```ignore
    /// use durable_execution_sdk_testing::CloudDurableTestRunner;
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
        let all_ops = self.cached_all_operations();
        self.operation_storage
            .get_by_id(id)
            .cloned()
            .map(|op| DurableOperation::new(op).with_operations(Arc::clone(&all_ops)))
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
    /// use durable_execution_sdk_testing::CloudDurableTestRunner;
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
        let all_ops = self.cached_all_operations();
        self.operation_storage
            .get_all()
            .iter()
            .cloned()
            .map(|op| DurableOperation::new(op).with_operations(Arc::clone(&all_ops)))
            .collect()
    }

    /// Creates a shared snapshot of all operations, avoiding repeated Vec clones
    /// across multiple lookup calls.
    fn cached_all_operations(&self) -> Arc<Vec<Operation>> {
        Arc::new(self.operation_storage.get_all().to_vec())
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
        let mut op1 = Operation::new("op-001", durable_execution_sdk::OperationType::Step);
        op1.name = Some("step1".to_string());
        storage.add_operation(op1);

        let mut op2 = Operation::new("op-002", durable_execution_sdk::OperationType::Wait);
        op2.name = Some("wait1".to_string());
        storage.add_operation(op2);

        let mut op3 = Operation::new("op-003", durable_execution_sdk::OperationType::Step);
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
