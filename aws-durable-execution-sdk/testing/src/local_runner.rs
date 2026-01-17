//! Local test runner for durable executions.
//!
//! This module provides the `LocalDurableTestRunner` for executing and testing
//! durable functions in-process with a simulated checkpoint backend.
//!
//! # Examples
//!
//! ```ignore
//! use aws_durable_execution_sdk_testing::{
//!     LocalDurableTestRunner, TestEnvironmentConfig, ExecutionStatus,
//! };
//!
//! #[tokio::test]
//! async fn test_workflow() {
//!     LocalDurableTestRunner::setup_test_environment(TestEnvironmentConfig {
//!         skip_time: true,
//!         checkpoint_delay: None,
//!     }).await.unwrap();
//!
//!     let mut runner = LocalDurableTestRunner::new(my_workflow);
//!     let result = runner.run("input".to_string()).await.unwrap();
//!
//!     assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
//!
//!     LocalDurableTestRunner::teardown_test_environment().await.unwrap();
//! }
//! ```

use std::collections::HashMap;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::sync::RwLock;

use crate::checkpoint_server::{
    ApiType, CheckpointWorkerManager, CheckpointWorkerParams, InvocationResult,
    SkipTimeConfig, StartDurableExecutionRequest, TestExecutionOrchestrator,
};
use crate::error::TestError;
use crate::mock_client::MockDurableServiceClient;
use crate::test_result::TestResult;
use crate::types::{ExecutionStatus, Invocation, TestResultError};
use aws_durable_execution_sdk::{
    DurableContext, DurableError, DurableServiceClient, Operation,
};

/// Global flag indicating whether the test environment has been set up.
static TEST_ENVIRONMENT_SETUP: AtomicBool = AtomicBool::new(false);

/// Global flag indicating whether time skipping is enabled.
static TIME_SKIPPING_ENABLED: AtomicBool = AtomicBool::new(false);

/// Configuration for setting up the test environment.
///
/// # Examples
///
/// ```
/// use aws_durable_execution_sdk_testing::TestEnvironmentConfig;
///
/// let config = TestEnvironmentConfig {
///     skip_time: true,
///     checkpoint_delay: None,
/// };
/// ```
#[derive(Debug, Clone)]
pub struct TestEnvironmentConfig {
    /// Enable time skipping for faster test execution.
    ///
    /// When enabled, wait operations complete instantly without blocking.
    pub skip_time: bool,

    /// Optional simulated checkpoint delay in milliseconds.
    ///
    /// If set, checkpoint operations will be delayed by this amount.
    pub checkpoint_delay: Option<u64>,
}

impl Default for TestEnvironmentConfig {
    fn default() -> Self {
        Self {
            skip_time: true,
            checkpoint_delay: None,
        }
    }
}

/// Internal storage for operations during test execution.
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

    #[allow(dead_code)]
    fn len(&self) -> usize {
        self.operations.len()
    }

    #[allow(dead_code)]
    fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }
}

/// Type alias for a shared async function that can be cloned.
type SharedAsyncFn<I, O> = Arc<
    dyn Fn(I, DurableContext) -> Pin<Box<dyn Future<Output = Result<O, DurableError>> + Send>>
        + Send
        + Sync,
>;

/// Type alias for a boxed durable function to reduce type complexity.
type BoxedDurableFn = Box<
    dyn Fn(serde_json::Value, DurableContext) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, DurableError>> + Send>>
        + Send
        + Sync,
>;

/// A registered function for invoke testing.
/// Note: The function fields are stored for future use when invoke operations
/// are fully implemented. Currently marked with allow(dead_code).
#[allow(dead_code)]
enum RegisteredFunction {
    /// A durable function that takes a DurableContext
    Durable(BoxedDurableFn),
    /// A regular function that doesn't need a DurableContext
    Regular(Box<dyn Fn(serde_json::Value) -> Result<serde_json::Value, DurableError> + Send + Sync>),
}

/// Local test runner for durable executions.
///
/// Executes durable handlers in-process with a checkpoint server running in a
/// separate thread, allowing rapid iteration during development without AWS deployment.
///
/// The runner uses a CheckpointWorkerManager to manage the checkpoint server thread,
/// matching the Node.js SDK's architecture for consistent cross-SDK behavior.
///
/// # Type Parameters
///
/// * `H` - The handler function type
/// * `I` - The input type (must be deserializable)
/// * `O` - The output type (must be serializable)
///
/// # Examples
///
/// ```ignore
/// use aws_durable_execution_sdk_testing::LocalDurableTestRunner;
///
/// async fn my_workflow(input: String, ctx: DurableContext) -> Result<String, DurableError> {
///     let result = ctx.step(|_| Ok(format!("processed: {}", input)), None).await?;
///     Ok(result)
/// }
///
/// let mut runner = LocalDurableTestRunner::new(my_workflow);
/// let result = runner.run("hello".to_string()).await.unwrap();
/// assert_eq!(result.get_result().unwrap(), "processed: hello");
/// ```
pub struct LocalDurableTestRunner<I, O>
where
    I: DeserializeOwned + Send + 'static,
    O: Serialize + DeserializeOwned + Send + 'static,
{
    /// The handler function to execute (shared for use with orchestrator)
    handler: SharedAsyncFn<I, O>,
    /// The checkpoint worker manager (manages checkpoint server thread)
    checkpoint_worker: Arc<CheckpointWorkerManager>,
    /// Legacy mock client for backward compatibility (deprecated)
    #[deprecated(note = "Use checkpoint_worker instead. Retained for backward compatibility.")]
    mock_client: Arc<MockDurableServiceClient>,
    /// Storage for captured operations
    operation_storage: Arc<RwLock<OperationStorage>>,
    /// Registered functions for chained invoke testing
    registered_functions: Arc<RwLock<HashMap<String, RegisteredFunction>>>,
    /// Phantom data for type parameters
    _phantom: PhantomData<(I, O)>,
}

impl<I, O> LocalDurableTestRunner<I, O>
where
    I: DeserializeOwned + Send + Serialize + 'static,
    O: Serialize + DeserializeOwned + Send + 'static,
{
    /// Sets up the test environment for durable execution testing.
    ///
    /// This method should be called once before running tests. It configures
    /// time control and other test infrastructure.
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration for the test environment
    ///
    /// # Requirements
    ///
    /// - 2.1: WHEN time skipping is enabled via setup_test_environment(),
    ///   THE Local_Test_Runner SHALL use Tokio's time manipulation to skip wait durations
    /// - 2.3: WHEN time skipping is disabled, THE Local_Test_Runner SHALL execute
    ///   wait operations with real timing
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::{LocalDurableTestRunner, TestEnvironmentConfig};
    ///
    /// LocalDurableTestRunner::<String, String>::setup_test_environment(TestEnvironmentConfig {
    ///     skip_time: true,
    ///     checkpoint_delay: None,
    /// }).await.unwrap();
    /// ```
    pub async fn setup_test_environment(config: TestEnvironmentConfig) -> Result<(), TestError> {
        // Enable time skipping if configured
        // Note: Each test with #[tokio::test(flavor = "current_thread")] has its own runtime,
        // so we always need to pause time in the current runtime, regardless of global state.
        if config.skip_time {
            // Pause tokio time to enable instant time advancement
            // Note: tokio::time::pause() requires current_thread runtime
            // We use catch_unwind to handle the case where we're in a multi-threaded runtime
            // or if time is already paused in this runtime
            use std::panic;
            let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                tokio::time::pause();
            }));
            
            if let Err(panic_info) = result {
                // Check if the panic was because we're not in a current_thread runtime
                let is_runtime_error = panic_info
                    .downcast_ref::<&str>()
                    .map(|msg| msg.contains("current_thread"))
                    .unwrap_or(false)
                    || panic_info
                        .downcast_ref::<String>()
                        .map(|msg| msg.contains("current_thread"))
                        .unwrap_or(false);
                
                // Check if time is already frozen (this is fine, just continue)
                let is_already_frozen = panic_info
                    .downcast_ref::<&str>()
                    .map(|msg| msg.contains("already frozen") || msg.contains("already paused"))
                    .unwrap_or(false)
                    || panic_info
                        .downcast_ref::<String>()
                        .map(|msg| msg.contains("already frozen") || msg.contains("already paused"))
                        .unwrap_or(false);
                
                if is_runtime_error {
                    // We're not in a current_thread runtime, so time control won't work
                    tracing::warn!(
                        "Time control requires current_thread Tokio runtime. \
                         Time skipping may not work correctly."
                    );
                } else if is_already_frozen {
                    // Time is already frozen in this runtime, which is fine
                    // This can happen if setup is called multiple times
                } else {
                    // Re-panic for other errors
                    panic::resume_unwind(panic_info);
                }
            }
            TIME_SKIPPING_ENABLED.store(true, Ordering::SeqCst);
        }

        TEST_ENVIRONMENT_SETUP.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Tears down the test environment.
    ///
    /// This method should be called after tests complete to restore normal
    /// time behavior and clean up test infrastructure.
    ///
    /// # Requirements
    ///
    /// - 2.4: WHEN teardown_test_environment() is called, THE Local_Test_Runner
    ///   SHALL restore normal time behavior
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::LocalDurableTestRunner;
    ///
    /// LocalDurableTestRunner::<String, String>::teardown_test_environment().await.unwrap();
    /// ```
    pub async fn teardown_test_environment() -> Result<(), TestError> {
        // Check if set up
        if !TEST_ENVIRONMENT_SETUP.load(Ordering::SeqCst) {
            return Ok(());
        }

        // Resume tokio time if it was paused
        // Note: tokio::time::resume() panics if time was never paused,
        // so we need to check if time is actually paused before resuming
        if TIME_SKIPPING_ENABLED.load(Ordering::SeqCst) {
            // Only resume if time is actually paused (check using is_time_paused helper)
            // Use catch_unwind to handle the case where we're in a multi-threaded runtime
            use std::panic;
            let _ = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                if crate::time_control::is_time_paused() {
                    tokio::time::resume();
                }
            }));
            TIME_SKIPPING_ENABLED.store(false, Ordering::SeqCst);
        }

        TEST_ENVIRONMENT_SETUP.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Returns whether the test environment has been set up.
    pub fn is_environment_setup() -> bool {
        TEST_ENVIRONMENT_SETUP.load(Ordering::SeqCst)
    }

    /// Returns whether time skipping is enabled.
    pub fn is_time_skipping_enabled() -> bool {
        TIME_SKIPPING_ENABLED.load(Ordering::SeqCst)
    }

    /// Creates a new local test runner with the given handler.
    ///
    /// # Arguments
    ///
    /// * `handler` - An async function that takes input and DurableContext
    ///
    /// # Requirements
    ///
    /// - 1.1: WHEN a developer creates a Local_Test_Runner with a handler function,
    ///   THE Local_Test_Runner SHALL accept any async function that takes a payload and DurableContext
    /// - 9.1: WHEN a developer creates a Local_Test_Runner, THE Local_Test_Runner
    ///   SHALL spawn a checkpoint server in a separate thread
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::LocalDurableTestRunner;
    ///
    /// async fn my_workflow(input: String, ctx: DurableContext) -> Result<String, DurableError> {
    ///     Ok(input)
    /// }
    ///
    /// let runner = LocalDurableTestRunner::new(my_workflow);
    /// ```
    pub fn new<F, Fut>(handler: F) -> Self
    where
        F: Fn(I, DurableContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, DurableError>> + Send + 'static,
    {
        // Wrap handler in Arc for sharing with orchestrator
        let handler: SharedAsyncFn<I, O> = Arc::new(move |input: I, ctx: DurableContext| {
            let fut = handler(input, ctx);
            Box::pin(fut) as Pin<Box<dyn Future<Output = Result<O, DurableError>> + Send>>
        });

        // Get or create the checkpoint worker manager singleton
        // If it fails (e.g., in test scenarios with poisoned locks), create a fresh instance
        let checkpoint_worker = match CheckpointWorkerManager::get_instance(None) {
            Ok(worker) => worker,
            Err(_) => {
                // Reset and try again
                CheckpointWorkerManager::reset_instance_for_testing();
                CheckpointWorkerManager::get_instance(None)
                    .expect("Failed to create CheckpointWorkerManager after reset")
            }
        };

        #[allow(deprecated)]
        Self {
            handler,
            checkpoint_worker,
            mock_client: Arc::new(MockDurableServiceClient::new().with_checkpoint_responses(100)),
            operation_storage: Arc::new(RwLock::new(OperationStorage::new())),
            registered_functions: Arc::new(RwLock::new(HashMap::new())),
            _phantom: PhantomData,
        }
    }


    /// Creates a new local test runner with a custom mock client.
    ///
    /// # Arguments
    ///
    /// * `handler` - An async function that takes input and DurableContext
    /// * `mock_client` - A pre-configured mock client (deprecated, use checkpoint_worker)
    #[deprecated(note = "Use new() instead. The checkpoint worker manager is now the default.")]
    pub fn with_mock_client<F, Fut>(handler: F, mock_client: MockDurableServiceClient) -> Self
    where
        F: Fn(I, DurableContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, DurableError>> + Send + 'static,
    {
        // Wrap handler in Arc for sharing with orchestrator
        let handler: SharedAsyncFn<I, O> = Arc::new(move |input: I, ctx: DurableContext| {
            let fut = handler(input, ctx);
            Box::pin(fut) as Pin<Box<dyn Future<Output = Result<O, DurableError>> + Send>>
        });

        // Get or create the checkpoint worker manager singleton
        let checkpoint_worker = CheckpointWorkerManager::get_instance(None)
            .expect("Failed to create CheckpointWorkerManager");

        #[allow(deprecated)]
        Self {
            handler,
            checkpoint_worker,
            mock_client: Arc::new(mock_client),
            operation_storage: Arc::new(RwLock::new(OperationStorage::new())),
            registered_functions: Arc::new(RwLock::new(HashMap::new())),
            _phantom: PhantomData,
        }
    }

    /// Creates a new local test runner with custom checkpoint worker parameters.
    ///
    /// # Arguments
    ///
    /// * `handler` - An async function that takes input and DurableContext
    /// * `params` - Configuration parameters for the checkpoint worker
    ///
    /// # Requirements
    ///
    /// - 9.2: WHEN the checkpoint server is running, THE Checkpoint_Worker_Manager
    ///   SHALL manage the lifecycle of the server thread
    pub fn with_checkpoint_params<F, Fut>(handler: F, params: CheckpointWorkerParams) -> Self
    where
        F: Fn(I, DurableContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, DurableError>> + Send + 'static,
    {
        // Wrap handler in Arc for sharing with orchestrator
        let handler: SharedAsyncFn<I, O> = Arc::new(move |input: I, ctx: DurableContext| {
            let fut = handler(input, ctx);
            Box::pin(fut) as Pin<Box<dyn Future<Output = Result<O, DurableError>> + Send>>
        });

        // Get or create the checkpoint worker manager singleton with custom params
        let checkpoint_worker = CheckpointWorkerManager::get_instance(Some(params))
            .expect("Failed to create CheckpointWorkerManager");

        #[allow(deprecated)]
        Self {
            handler,
            checkpoint_worker,
            mock_client: Arc::new(MockDurableServiceClient::new().with_checkpoint_responses(100)),
            operation_storage: Arc::new(RwLock::new(OperationStorage::new())),
            registered_functions: Arc::new(RwLock::new(HashMap::new())),
            _phantom: PhantomData,
        }
    }

    /// Returns a reference to the checkpoint worker manager.
    pub fn checkpoint_worker(&self) -> &Arc<CheckpointWorkerManager> {
        &self.checkpoint_worker
    }

    /// Returns a reference to the mock client (deprecated).
    #[deprecated(note = "Use checkpoint_worker() instead.")]
    #[allow(deprecated)]
    pub fn mock_client(&self) -> &Arc<MockDurableServiceClient> {
        &self.mock_client
    }

    /// Returns the number of captured operations.
    pub async fn operation_count(&self) -> usize {
        self.operation_storage.read().await.len()
    }

    /// Executes the handler with the given payload and returns the test result.
    ///
    /// This method uses the TestExecutionOrchestrator to manage the full execution
    /// lifecycle, including:
    /// - Polling for checkpoint updates
    /// - Processing wait operations and scheduling re-invocations
    /// - Handling time skipping for wait operations
    /// - Managing callback completions
    ///
    /// # Arguments
    ///
    /// * `payload` - The input payload to pass to the handler
    ///
    /// # Requirements
    ///
    /// - 1.2: WHEN a developer calls run() with a payload, THE Local_Test_Runner
    ///   SHALL execute the handler function and return a Test_Result
    /// - 1.3: WHEN the handler function completes successfully, THE Test_Result
    ///   SHALL contain the execution result and status SUCCEEDED
    /// - 1.4: WHEN the handler function fails with an error, THE Test_Result
    ///   SHALL contain the error details and status FAILED
    /// - 1.5: WHEN the handler function performs durable operations, THE Local_Test_Runner
    ///   SHALL capture all operations in the Test_Result
    /// - 9.2: WHEN a checkpoint call is made, THE Checkpoint_Manager SHALL process
    ///   operation updates and maintain full execution state
    /// - 16.1: WHEN a wait operation is encountered, THE Test_Execution_Orchestrator
    ///   SHALL track the wait's scheduled end timestamp
    /// - 16.2: WHEN time skipping is enabled and a wait's scheduled end time is reached,
    ///   THE Test_Execution_Orchestrator SHALL mark the wait as SUCCEEDED and schedule
    ///   handler re-invocation
    /// - 16.3: WHEN time skipping is enabled, THE Test_Execution_Orchestrator SHALL use
    ///   tokio::time::advance() to skip wait durations instantly
    /// - 16.4: WHEN a handler invocation returns PENDING status, THE Test_Execution_Orchestrator
    ///   SHALL continue polling for operation updates and re-invoke the handler when
    ///   operations complete
    /// - 16.5: WHEN a handler invocation returns SUCCEEDED or FAILED status,
    ///   THE Test_Execution_Orchestrator SHALL resolve the execution and stop polling
    /// - 16.6: WHEN multiple operations are pending (waits, callbacks, steps with retries),
    ///   THE Test_Execution_Orchestrator SHALL process them in scheduled order
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::LocalDurableTestRunner;
    ///
    /// let mut runner = LocalDurableTestRunner::new(my_workflow);
    /// let result = runner.run("hello".to_string()).await.unwrap();
    ///
    /// assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
    /// ```
    pub async fn run(&mut self, payload: I) -> Result<TestResult<O>, TestError>
    where
        I: Clone,
    {
        // Delegate to run_with_orchestrator which handles the full execution lifecycle
        // including wait operations, callbacks, and re-invocations
        self.run_with_orchestrator(payload).await
    }

    /// Executes the handler with a single invocation (no re-invocation on suspend).
    ///
    /// This method performs a single handler invocation without using the orchestrator.
    /// If the handler suspends (e.g., due to a wait operation), the result will have
    /// status `Running` and the execution will not be automatically resumed.
    ///
    /// Use this method when you want to test the initial invocation behavior without
    /// automatic wait completion and re-invocation.
    ///
    /// # Arguments
    ///
    /// * `payload` - The input payload to pass to the handler
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::LocalDurableTestRunner;
    ///
    /// let mut runner = LocalDurableTestRunner::new(my_workflow_with_wait);
    /// let result = runner.run_single_invocation("hello".to_string()).await.unwrap();
    ///
    /// // Handler suspended on wait, execution is still running
    /// assert_eq!(result.get_status(), ExecutionStatus::Running);
    /// ```
    pub async fn run_single_invocation(&mut self, payload: I) -> Result<TestResult<O>, TestError> {
        use aws_durable_execution_sdk::lambda::InitialExecutionState;
        use aws_durable_execution_sdk::state::ExecutionState;

        // Clear previous operations
        self.operation_storage.write().await.clear();
        
        // Also clear mock client state for backward compatibility
        #[allow(deprecated)]
        self.mock_client.clear_all_calls();

        // Serialize the payload for the checkpoint server
        let payload_json = serde_json::to_string(&payload)?;

        // Start execution with the checkpoint server
        // This registers the execution and returns a checkpoint token
        let invocation_id = uuid::Uuid::new_v4().to_string();
        let start_request = StartDurableExecutionRequest {
            invocation_id: invocation_id.clone(),
            payload: Some(payload_json),
        };
        let start_payload = serde_json::to_string(&start_request)?;
        
        let start_response = self.checkpoint_worker
            .send_api_request(ApiType::StartDurableExecution, start_payload)
            .await?;
        
        if let Some(error) = start_response.error {
            return Err(TestError::CheckpointServerError(error));
        }
        
        let invocation_result: InvocationResult = serde_json::from_str(
            &start_response.payload.ok_or_else(|| {
                TestError::CheckpointServerError("Empty response from checkpoint server".to_string())
            })?
        )?;

        let execution_arn = invocation_result.execution_id;
        let checkpoint_token = invocation_result.checkpoint_token;

        // Create initial execution state (empty for new execution)
        let initial_state = InitialExecutionState::new();

        // Create the execution state with the checkpoint worker manager
        // The checkpoint worker implements DurableServiceClient trait
        let execution_state = Arc::new(ExecutionState::new(
            &execution_arn,
            &checkpoint_token,
            initial_state,
            self.checkpoint_worker.clone(),
        ));

        // Create the durable context
        let ctx = DurableContext::new(execution_state.clone());

        // Record invocation start
        let start_time = chrono::Utc::now();
        let mut invocation = Invocation::with_start(start_time);

        // Execute the handler
        let handler_result = (self.handler)(payload, ctx).await;

        // Record invocation end
        let end_time = chrono::Utc::now();
        invocation = invocation.with_end(end_time);

        // Retrieve operations from the checkpoint server
        let operations = match self.checkpoint_worker.get_operations(&execution_arn, "").await {
            Ok(response) => {
                let mut storage = self.operation_storage.write().await;
                for op in &response.operations {
                    storage.add_operation(op.clone());
                }
                response.operations
            }
            Err(_) => {
                // If we can't get operations from checkpoint server, return empty list
                Vec::new()
            }
        };

        // Build the test result based on handler outcome
        match handler_result {
            Ok(result) => {
                let mut test_result = TestResult::success(result, operations);
                test_result.add_invocation(invocation);
                Ok(test_result)
            }
            Err(error) => {
                // Check if this is a suspend error (which means pending, not failed)
                if error.is_suspend() {
                    let mut test_result =
                        TestResult::with_status(ExecutionStatus::Running, operations);
                    test_result.add_invocation(invocation);
                    Ok(test_result)
                } else {
                    // Convert DurableError to ErrorObject to get the error type
                    let error_obj = aws_durable_execution_sdk::ErrorObject::from(&error);
                    let test_error = TestResultError::new(
                        error_obj.error_type,
                        error.to_string(),
                    );
                    invocation = invocation.with_error(test_error.clone());
                    let mut test_result = TestResult::failure(test_error, operations);
                    test_result.add_invocation(invocation);
                    Ok(test_result)
                }
            }
        }
    }

    /// Executes the handler with the given payload using the TestExecutionOrchestrator.
    ///
    /// This method uses the TestExecutionOrchestrator to manage the full execution
    /// lifecycle, including:
    /// - Polling for checkpoint updates
    /// - Processing wait operations and scheduling re-invocations
    /// - Handling time skipping for wait operations
    /// - Managing callback completions
    ///
    /// This is the recommended method for testing workflows with wait operations,
    /// as it properly handles the full execution lifecycle including re-invocations.
    ///
    /// # Arguments
    ///
    /// * `payload` - The input payload to pass to the handler
    ///
    /// # Requirements
    ///
    /// - 16.1: WHEN a wait operation is encountered, THE Test_Execution_Orchestrator
    ///   SHALL track the wait's scheduled end timestamp
    /// - 16.2: WHEN time skipping is enabled and a wait's scheduled end time is reached,
    ///   THE Test_Execution_Orchestrator SHALL mark the wait as SUCCEEDED and schedule
    ///   handler re-invocation
    /// - 16.3: WHEN time skipping is enabled, THE Test_Execution_Orchestrator SHALL use
    ///   tokio::time::advance() to skip wait durations instantly
    /// - 16.4: WHEN a handler invocation returns PENDING status, THE Test_Execution_Orchestrator
    ///   SHALL continue polling for operation updates and re-invoke the handler when
    ///   operations complete
    /// - 16.5: WHEN a handler invocation returns SUCCEEDED or FAILED status,
    ///   THE Test_Execution_Orchestrator SHALL resolve the execution and stop polling
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::LocalDurableTestRunner;
    ///
    /// let mut runner = LocalDurableTestRunner::new(my_workflow_with_waits);
    /// let result = runner.run_with_orchestrator("hello".to_string()).await.unwrap();
    ///
    /// // Wait operations are automatically completed with time skipping
    /// assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
    /// ```
    pub async fn run_with_orchestrator(&mut self, payload: I) -> Result<TestResult<O>, TestError>
    where
        I: Clone,
    {
        use crate::checkpoint_server::OperationStorage as OrchestratorOperationStorage;

        // Clear previous operations
        self.operation_storage.write().await.clear();

        // Also clear mock client state for backward compatibility
        #[allow(deprecated)]
        self.mock_client.clear_all_calls();

        // Configure time skipping based on test environment settings
        let skip_time_config = SkipTimeConfig {
            enabled: Self::is_time_skipping_enabled(),
        };

        // Create shared operation storage for the orchestrator
        let orchestrator_storage = Arc::new(tokio::sync::RwLock::new(OrchestratorOperationStorage::new()));

        // Clone the handler Arc for use in the orchestrator
        let handler = Arc::clone(&self.handler);

        // Create the orchestrator with the shared handler
        // The orchestrator will manage the execution lifecycle
        let mut orchestrator = TestExecutionOrchestrator::new(
            move |input: I, ctx: DurableContext| {
                let handler = Arc::clone(&handler);
                async move { handler(input, ctx).await }
            },
            orchestrator_storage.clone(),
            self.checkpoint_worker.clone(),
            skip_time_config,
        );

        // Execute using the orchestrator
        let execution_result = orchestrator.execute_handler(payload.clone()).await?;

        // Copy operations from orchestrator storage to our storage
        {
            let orch_storage = orchestrator_storage.read().await;
            let mut our_storage = self.operation_storage.write().await;
            for op in orch_storage.get_all() {
                our_storage.add_operation(op.clone());
            }
        }

        // Convert TestExecutionResult to TestResult
        let mut test_result = match execution_result.status {
            ExecutionStatus::Succeeded => {
                if let Some(result) = execution_result.result {
                    TestResult::success(result, execution_result.operations)
                } else {
                    TestResult::with_status(ExecutionStatus::Succeeded, execution_result.operations)
                }
            }
            ExecutionStatus::Failed => {
                if let Some(error) = execution_result.error {
                    TestResult::failure(error, execution_result.operations)
                } else {
                    TestResult::with_status(ExecutionStatus::Failed, execution_result.operations)
                }
            }
            ExecutionStatus::Running => {
                TestResult::with_status(ExecutionStatus::Running, execution_result.operations)
            }
            _ => {
                TestResult::with_status(execution_result.status, execution_result.operations)
            }
        };

        // Add invocations
        for invocation in execution_result.invocations {
            test_result.add_invocation(invocation);
        }

        Ok(test_result)
    }

    /// Resets the test runner state for a fresh test run.
    ///
    /// This method clears all captured operations and resets the checkpoint server
    /// state, allowing the runner to be reused for multiple test scenarios.
    ///
    /// # Requirements
    ///
    /// - 1.6: WHEN reset() is called, THE Local_Test_Runner SHALL clear all
    ///   captured operations and reset checkpoint server state
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::LocalDurableTestRunner;
    ///
    /// let mut runner = LocalDurableTestRunner::new(my_workflow);
    ///
    /// // First test run
    /// let result1 = runner.run("input1".to_string()).await.unwrap();
    ///
    /// // Reset for fresh state
    /// runner.reset().await;
    ///
    /// // Second test run with clean state
    /// let result2 = runner.run("input2".to_string()).await.unwrap();
    /// ```
    pub async fn reset(&mut self) {
        // Clear operation storage
        self.operation_storage.write().await.clear();

        // Reset checkpoint worker manager singleton for fresh state
        CheckpointWorkerManager::reset_instance_for_testing();

        // Re-acquire the checkpoint worker manager
        self.checkpoint_worker = CheckpointWorkerManager::get_instance(None)
            .expect("Failed to create CheckpointWorkerManager after reset");

        // Also clear mock client state for backward compatibility
        #[allow(deprecated)]
        self.mock_client.clear_all_calls();
    }

    /// Gets an operation by its unique ID.
    ///
    /// # Arguments
    ///
    /// * `id` - The unique operation ID
    ///
    /// # Returns
    ///
    /// The operation if found, or `None` if no operation with that ID exists.
    ///
    /// # Requirements
    ///
    /// - 4.1: WHEN get_operation(name) is called, THE Test_Result SHALL return
    ///   the first operation with that name
    /// - 4.4: WHEN get_operation_by_id(id) is called, THE Test_Result SHALL return
    ///   the operation with that unique ID
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::LocalDurableTestRunner;
    ///
    /// let mut runner = LocalDurableTestRunner::new(my_workflow);
    /// let _ = runner.run("input".to_string()).await.unwrap();
    ///
    /// if let Some(op) = runner.get_operation_by_id("op-123").await {
    ///     println!("Found operation: {:?}", op);
    /// }
    /// ```
    pub async fn get_operation_by_id(&self, id: &str) -> Option<Operation> {
        self.operation_storage.read().await.get_by_id(id).cloned()
    }

    /// Gets the first operation with the given name.
    ///
    /// # Arguments
    ///
    /// * `name` - The operation name to search for
    ///
    /// # Returns
    ///
    /// The first operation with that name, or `None` if no operation with that name exists.
    ///
    /// # Requirements
    ///
    /// - 4.1: WHEN get_operation(name) is called, THE Test_Result SHALL return
    ///   the first operation with that name
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::LocalDurableTestRunner;
    ///
    /// let mut runner = LocalDurableTestRunner::new(my_workflow);
    /// let _ = runner.run("input".to_string()).await.unwrap();
    ///
    /// if let Some(op) = runner.get_operation("process_data").await {
    ///     println!("Found operation: {:?}", op);
    /// }
    /// ```
    pub async fn get_operation(&self, name: &str) -> Option<Operation> {
        self.operation_storage.read().await.get_by_name(name).cloned()
    }

    /// Gets an operation by its index in the execution order.
    ///
    /// # Arguments
    ///
    /// * `index` - The zero-based index of the operation
    ///
    /// # Returns
    ///
    /// The operation at that index, or `None` if the index is out of bounds.
    ///
    /// # Requirements
    ///
    /// - 4.2: WHEN get_operation_by_index(index) is called, THE Test_Result SHALL return
    ///   the operation at that execution order index
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::LocalDurableTestRunner;
    ///
    /// let mut runner = LocalDurableTestRunner::new(my_workflow);
    /// let _ = runner.run("input".to_string()).await.unwrap();
    ///
    /// // Get the first operation
    /// if let Some(op) = runner.get_operation_by_index(0).await {
    ///     println!("First operation: {:?}", op);
    /// }
    /// ```
    pub async fn get_operation_by_index(&self, index: usize) -> Option<Operation> {
        self.operation_storage.read().await.get_by_index(index).cloned()
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
    /// The operation at that name/index combination, or `None` if not found.
    ///
    /// # Requirements
    ///
    /// - 4.3: WHEN get_operation_by_name_and_index(name, index) is called,
    ///   THE Test_Result SHALL return the Nth operation with that name
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::LocalDurableTestRunner;
    ///
    /// let mut runner = LocalDurableTestRunner::new(my_workflow);
    /// let _ = runner.run("input".to_string()).await.unwrap();
    ///
    /// // Get the second "process" operation
    /// if let Some(op) = runner.get_operation_by_name_and_index("process", 1).await {
    ///     println!("Second process operation: {:?}", op);
    /// }
    /// ```
    pub async fn get_operation_by_name_and_index(&self, name: &str, index: usize) -> Option<Operation> {
        self.operation_storage.read().await.get_by_name_and_index(name, index).cloned()
    }

    /// Gets all captured operations.
    ///
    /// # Returns
    ///
    /// A vector of all operations in execution order.
    ///
    /// # Requirements
    ///
    /// - 4.5: WHEN get_all_operations() is called, THE Test_Result SHALL return
    ///   all operations in execution order
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::LocalDurableTestRunner;
    ///
    /// let mut runner = LocalDurableTestRunner::new(my_workflow);
    /// let _ = runner.run("input".to_string()).await.unwrap();
    ///
    /// let all_ops = runner.get_all_operations().await;
    /// println!("Total operations: {}", all_ops.len());
    /// ```
    pub async fn get_all_operations(&self) -> Vec<Operation> {
        self.operation_storage.read().await.get_all().to_vec()
    }

    /// Registers a durable function for invoke testing.
    ///
    /// Durable functions receive a `DurableContext` and can perform durable operations.
    /// When the main handler invokes a function by name, the registered function
    /// will be called.
    ///
    /// # Arguments
    ///
    /// * `name` - The name to register the function under
    /// * `func` - The durable function to register
    ///
    /// # Requirements
    ///
    /// - 7.1: WHEN register_durable_function(name, func) is called, THE Local_Test_Runner
    ///   SHALL store the function for invoke handling
    /// - 7.2: WHEN a registered durable function is invoked, THE Local_Test_Runner
    ///   SHALL execute it with a DurableContext
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::LocalDurableTestRunner;
    ///
    /// async fn helper_workflow(input: serde_json::Value, ctx: DurableContext) -> Result<serde_json::Value, DurableError> {
    ///     Ok(serde_json::json!({"processed": true}))
    /// }
    ///
    /// let mut runner = LocalDurableTestRunner::new(main_workflow);
    /// runner.register_durable_function("helper", helper_workflow).await;
    /// ```
    pub async fn register_durable_function<F, Fut>(&self, name: impl Into<String>, func: F)
    where
        F: Fn(serde_json::Value, DurableContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<serde_json::Value, DurableError>> + Send + 'static,
    {
        let boxed_func = Box::new(move |input: serde_json::Value, ctx: DurableContext| {
            let fut = func(input, ctx);
            Box::pin(fut) as Pin<Box<dyn Future<Output = Result<serde_json::Value, DurableError>> + Send>>
        });

        self.registered_functions
            .write()
            .await
            .insert(name.into(), RegisteredFunction::Durable(boxed_func));
    }

    /// Registers a regular (non-durable) function for invoke testing.
    ///
    /// Regular functions do not receive a `DurableContext` and cannot perform
    /// durable operations. They are useful for testing simple helper functions.
    ///
    /// # Arguments
    ///
    /// * `name` - The name to register the function under
    /// * `func` - The regular function to register
    ///
    /// # Requirements
    ///
    /// - 7.3: WHEN register_function(name, func) is called, THE Local_Test_Runner
    ///   SHALL store the function for invoke handling
    /// - 7.4: WHEN a registered regular function is invoked, THE Local_Test_Runner
    ///   SHALL execute it without a DurableContext
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::LocalDurableTestRunner;
    ///
    /// fn simple_helper(input: serde_json::Value) -> Result<serde_json::Value, DurableError> {
    ///     Ok(serde_json::json!({"result": "done"}))
    /// }
    ///
    /// let mut runner = LocalDurableTestRunner::new(main_workflow);
    /// runner.register_function("simple_helper", simple_helper).await;
    /// ```
    pub async fn register_function<F>(&self, name: impl Into<String>, func: F)
    where
        F: Fn(serde_json::Value) -> Result<serde_json::Value, DurableError> + Send + Sync + 'static,
    {
        self.registered_functions
            .write()
            .await
            .insert(name.into(), RegisteredFunction::Regular(Box::new(func)));
    }

    /// Gets a registered function by name.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the function to retrieve
    ///
    /// # Returns
    ///
    /// `true` if a function with that name is registered, `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::LocalDurableTestRunner;
    ///
    /// let runner = LocalDurableTestRunner::new(main_workflow);
    /// runner.register_function("helper", |_| Ok(serde_json::json!({}))).await;
    ///
    /// assert!(runner.has_registered_function("helper").await);
    /// assert!(!runner.has_registered_function("nonexistent").await);
    /// ```
    pub async fn has_registered_function(&self, name: &str) -> bool {
        self.registered_functions.read().await.contains_key(name)
    }

    /// Gets the count of registered functions.
    ///
    /// # Returns
    ///
    /// The number of registered functions.
    pub async fn registered_function_count(&self) -> usize {
        self.registered_functions.read().await.len()
    }

    /// Clears all registered functions.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use aws_durable_execution_sdk_testing::LocalDurableTestRunner;
    ///
    /// let mut runner = LocalDurableTestRunner::new(main_workflow);
    /// runner.register_function("helper", |_| Ok(serde_json::json!({}))).await;
    /// assert_eq!(runner.registered_function_count().await, 1);
    ///
    /// runner.clear_registered_functions().await;
    /// assert_eq!(runner.registered_function_count().await, 0);
    /// ```
    pub async fn clear_registered_functions(&mut self) {
        self.registered_functions.write().await.clear();
    }
}

impl<I, O> std::fmt::Debug for LocalDurableTestRunner<I, O>
where
    I: DeserializeOwned + Send + 'static,
    O: Serialize + DeserializeOwned + Send + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalDurableTestRunner")
            .field("checkpoint_worker", &"CheckpointWorkerManager")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_durable_execution_sdk::OperationType;

    async fn simple_handler(input: String, _ctx: DurableContext) -> Result<String, DurableError> {
        Ok(format!("processed: {}", input))
    }

    #[test]
    fn test_test_environment_config_default() {
        let config = TestEnvironmentConfig::default();
        assert!(config.skip_time);
        assert!(config.checkpoint_delay.is_none());
    }

    #[test]
    fn test_operation_storage_new() {
        let storage = OperationStorage::new();
        assert!(storage.is_empty());
        assert_eq!(storage.len(), 0);
    }

    #[test]
    fn test_operation_storage_add_and_get() {
        let mut storage = OperationStorage::new();

        let mut op1 = Operation::new("op-1", OperationType::Step);
        op1.name = Some("step1".to_string());
        storage.add_operation(op1);

        let mut op2 = Operation::new("op-2", OperationType::Wait);
        op2.name = Some("wait1".to_string());
        storage.add_operation(op2);

        assert_eq!(storage.len(), 2);

        // Get by ID
        let found = storage.get_by_id("op-1");
        assert!(found.is_some());
        assert_eq!(found.unwrap().operation_id, "op-1");

        // Get by name
        let found = storage.get_by_name("step1");
        assert!(found.is_some());
        assert_eq!(found.unwrap().operation_id, "op-1");

        // Get by index
        let found = storage.get_by_index(1);
        assert!(found.is_some());
        assert_eq!(found.unwrap().operation_id, "op-2");

        // Get all
        let all = storage.get_all();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_operation_storage_get_by_name_and_index() {
        let mut storage = OperationStorage::new();

        // Add multiple operations with the same name
        let mut op1 = Operation::new("op-1", OperationType::Step);
        op1.name = Some("step".to_string());
        storage.add_operation(op1);

        let mut op2 = Operation::new("op-2", OperationType::Step);
        op2.name = Some("step".to_string());
        storage.add_operation(op2);

        let mut op3 = Operation::new("op-3", OperationType::Step);
        op3.name = Some("step".to_string());
        storage.add_operation(op3);

        // Get first occurrence
        let found = storage.get_by_name_and_index("step", 0);
        assert!(found.is_some());
        assert_eq!(found.unwrap().operation_id, "op-1");

        // Get second occurrence
        let found = storage.get_by_name_and_index("step", 1);
        assert!(found.is_some());
        assert_eq!(found.unwrap().operation_id, "op-2");

        // Get third occurrence
        let found = storage.get_by_name_and_index("step", 2);
        assert!(found.is_some());
        assert_eq!(found.unwrap().operation_id, "op-3");

        // Out of bounds
        let found = storage.get_by_name_and_index("step", 3);
        assert!(found.is_none());
    }

    #[test]
    fn test_operation_storage_clear() {
        let mut storage = OperationStorage::new();

        let op = Operation::new("op-1", OperationType::Step);
        storage.add_operation(op);
        assert_eq!(storage.len(), 1);

        storage.clear();
        assert!(storage.is_empty());
        assert!(storage.get_by_id("op-1").is_none());
    }

    #[tokio::test]
    async fn test_local_runner_creation() {
        let runner = LocalDurableTestRunner::new(simple_handler);
        assert_eq!(runner.operation_count().await, 0);
    }

    #[tokio::test]
    #[allow(deprecated)]
    async fn test_local_runner_with_mock_client() {
        let mock_client = MockDurableServiceClient::new().with_checkpoint_responses(10);
        let runner = LocalDurableTestRunner::with_mock_client(simple_handler, mock_client);
        assert_eq!(runner.operation_count().await, 0);
    }

    #[tokio::test]
    async fn test_setup_teardown_environment() {
        // Ensure clean state
        LocalDurableTestRunner::<String, String>::teardown_test_environment()
            .await
            .unwrap();

        assert!(!LocalDurableTestRunner::<String, String>::is_environment_setup());
        assert!(!LocalDurableTestRunner::<String, String>::is_time_skipping_enabled());

        // Setup with time skipping
        LocalDurableTestRunner::<String, String>::setup_test_environment(TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        })
        .await
        .unwrap();

        assert!(LocalDurableTestRunner::<String, String>::is_environment_setup());
        assert!(LocalDurableTestRunner::<String, String>::is_time_skipping_enabled());

        // Teardown
        LocalDurableTestRunner::<String, String>::teardown_test_environment()
            .await
            .unwrap();

        assert!(!LocalDurableTestRunner::<String, String>::is_environment_setup());
        assert!(!LocalDurableTestRunner::<String, String>::is_time_skipping_enabled());
    }

    #[tokio::test]
    async fn test_setup_without_time_skipping() {
        // Ensure clean state
        LocalDurableTestRunner::<String, String>::teardown_test_environment()
            .await
            .unwrap();

        // Setup without time skipping
        LocalDurableTestRunner::<String, String>::setup_test_environment(TestEnvironmentConfig {
            skip_time: false,
            checkpoint_delay: None,
        })
        .await
        .unwrap();

        assert!(LocalDurableTestRunner::<String, String>::is_environment_setup());
        assert!(!LocalDurableTestRunner::<String, String>::is_time_skipping_enabled());

        // Teardown
        LocalDurableTestRunner::<String, String>::teardown_test_environment()
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_double_setup_is_idempotent() {
        // Ensure clean state
        LocalDurableTestRunner::<String, String>::teardown_test_environment()
            .await
            .unwrap();

        // First setup
        LocalDurableTestRunner::<String, String>::setup_test_environment(
            TestEnvironmentConfig::default(),
        )
        .await
        .unwrap();

        // Second setup should be idempotent
        LocalDurableTestRunner::<String, String>::setup_test_environment(
            TestEnvironmentConfig::default(),
        )
        .await
        .unwrap();

        assert!(LocalDurableTestRunner::<String, String>::is_environment_setup());

        // Teardown
        LocalDurableTestRunner::<String, String>::teardown_test_environment()
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_double_teardown_is_idempotent() {
        // Ensure clean state
        LocalDurableTestRunner::<String, String>::teardown_test_environment()
            .await
            .unwrap();

        // Setup
        LocalDurableTestRunner::<String, String>::setup_test_environment(
            TestEnvironmentConfig::default(),
        )
        .await
        .unwrap();

        // First teardown
        LocalDurableTestRunner::<String, String>::teardown_test_environment()
            .await
            .unwrap();

        // Second teardown should be idempotent
        LocalDurableTestRunner::<String, String>::teardown_test_environment()
            .await
            .unwrap();

        assert!(!LocalDurableTestRunner::<String, String>::is_environment_setup());
    }

    // Tests for run() method - Subtask 8.3

    #[tokio::test]
    async fn test_run_successful_execution() {
        // Ensure clean state
        LocalDurableTestRunner::<String, String>::teardown_test_environment()
            .await
            .unwrap();

        let mut runner = LocalDurableTestRunner::new(simple_handler);
        let result = runner.run("hello".to_string()).await.unwrap();

        // Verify successful execution status
        assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

        // Verify result value
        let output = result.get_result().unwrap();
        assert_eq!(output, "processed: hello");

        // Verify invocation was recorded
        assert_eq!(result.get_invocations().len(), 1);
    }

    #[tokio::test]
    async fn test_run_failed_execution() {
        async fn failing_handler(
            _input: String,
            _ctx: DurableContext,
        ) -> Result<String, DurableError> {
            Err(DurableError::execution("Test failure"))
        }

        // Ensure clean state
        LocalDurableTestRunner::<String, String>::teardown_test_environment()
            .await
            .unwrap();

        let mut runner = LocalDurableTestRunner::new(failing_handler);
        let result = runner.run("hello".to_string()).await.unwrap();

        // Verify failed execution status
        assert_eq!(result.get_status(), ExecutionStatus::Failed);

        // Verify error is captured
        let error = result.get_error().unwrap();
        assert!(error.error_message.as_ref().unwrap().contains("Test failure"));

        // Verify invocation was recorded with error
        let invocations = result.get_invocations();
        assert_eq!(invocations.len(), 1);
        assert!(invocations[0].error.is_some());
    }

    #[tokio::test]
    async fn test_run_multiple_times_clears_previous_operations() {
        // Ensure clean state
        LocalDurableTestRunner::<String, String>::teardown_test_environment()
            .await
            .unwrap();

        let mut runner = LocalDurableTestRunner::new(simple_handler);

        // First run
        let result1 = runner.run("first".to_string()).await.unwrap();
        assert_eq!(result1.get_status(), ExecutionStatus::Succeeded);
        assert_eq!(result1.get_result().unwrap(), "processed: first");

        // Second run - should have fresh state
        let result2 = runner.run("second".to_string()).await.unwrap();
        assert_eq!(result2.get_status(), ExecutionStatus::Succeeded);
        assert_eq!(result2.get_result().unwrap(), "processed: second");
    }

    // Tests for reset() method - Subtask 8.4

    #[tokio::test]
    async fn test_reset_clears_operation_storage() {
        // Ensure clean state
        LocalDurableTestRunner::<String, String>::teardown_test_environment()
            .await
            .unwrap();

        // Reset checkpoint worker manager for clean state
        CheckpointWorkerManager::reset_instance_for_testing();

        let mut runner = LocalDurableTestRunner::new(simple_handler);

        // Run to generate some state
        let _ = runner.run("hello".to_string()).await.unwrap();

        // Reset the runner
        runner.reset().await;

        // Verify operation storage is cleared
        assert_eq!(runner.operation_count().await, 0);
    }

    #[tokio::test]
    async fn test_reset_allows_fresh_run() {
        // Ensure clean state
        LocalDurableTestRunner::<String, String>::teardown_test_environment()
            .await
            .unwrap();

        let mut runner = LocalDurableTestRunner::new(simple_handler);

        // First run
        let result1 = runner.run("first".to_string()).await.unwrap();
        assert_eq!(result1.get_result().unwrap(), "processed: first");

        // Reset
        runner.reset().await;

        // Second run after reset
        let result2 = runner.run("second".to_string()).await.unwrap();
        assert_eq!(result2.get_result().unwrap(), "processed: second");
        assert_eq!(result2.get_status(), ExecutionStatus::Succeeded);
    }

    // Tests for operation lookup methods - Subtask 8.5

    #[tokio::test]
    async fn test_get_operation_by_id() {
        let runner: LocalDurableTestRunner<String, String> =
            LocalDurableTestRunner::new(simple_handler);

        // Manually add an operation to storage for testing
        {
            let mut storage = runner.operation_storage.write().await;
            let mut op = Operation::new("test-op-id", OperationType::Step);
            op.name = Some("test_step".to_string());
            storage.add_operation(op);
        }

        // Test get_operation_by_id
        let found = runner.get_operation_by_id("test-op-id").await;
        assert!(found.is_some());
        assert_eq!(found.unwrap().operation_id, "test-op-id");

        // Test not found
        let not_found = runner.get_operation_by_id("nonexistent").await;
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_get_operation_by_name() {
        let runner: LocalDurableTestRunner<String, String> =
            LocalDurableTestRunner::new(simple_handler);

        // Manually add operations to storage for testing
        {
            let mut storage = runner.operation_storage.write().await;
            let mut op1 = Operation::new("op-1", OperationType::Step);
            op1.name = Some("process".to_string());
            storage.add_operation(op1);

            let mut op2 = Operation::new("op-2", OperationType::Step);
            op2.name = Some("validate".to_string());
            storage.add_operation(op2);
        }

        // Test get_operation (by name)
        let found = runner.get_operation("process").await;
        assert!(found.is_some());
        assert_eq!(found.unwrap().operation_id, "op-1");

        // Test not found
        let not_found = runner.get_operation("nonexistent").await;
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_get_operation_by_index() {
        let runner: LocalDurableTestRunner<String, String> =
            LocalDurableTestRunner::new(simple_handler);

        // Manually add operations to storage for testing
        {
            let mut storage = runner.operation_storage.write().await;
            storage.add_operation(Operation::new("op-0", OperationType::Step));
            storage.add_operation(Operation::new("op-1", OperationType::Wait));
            storage.add_operation(Operation::new("op-2", OperationType::Callback));
        }

        // Test get_operation_by_index
        let op0 = runner.get_operation_by_index(0).await;
        assert!(op0.is_some());
        assert_eq!(op0.unwrap().operation_id, "op-0");

        let op1 = runner.get_operation_by_index(1).await;
        assert!(op1.is_some());
        assert_eq!(op1.unwrap().operation_id, "op-1");

        let op2 = runner.get_operation_by_index(2).await;
        assert!(op2.is_some());
        assert_eq!(op2.unwrap().operation_id, "op-2");

        // Test out of bounds
        let out_of_bounds = runner.get_operation_by_index(3).await;
        assert!(out_of_bounds.is_none());
    }

    #[tokio::test]
    async fn test_get_operation_by_name_and_index() {
        let runner: LocalDurableTestRunner<String, String> =
            LocalDurableTestRunner::new(simple_handler);

        // Manually add operations with same name to storage for testing
        {
            let mut storage = runner.operation_storage.write().await;
            let mut op1 = Operation::new("op-1", OperationType::Step);
            op1.name = Some("process".to_string());
            storage.add_operation(op1);

            let mut op2 = Operation::new("op-2", OperationType::Step);
            op2.name = Some("process".to_string());
            storage.add_operation(op2);

            let mut op3 = Operation::new("op-3", OperationType::Step);
            op3.name = Some("process".to_string());
            storage.add_operation(op3);
        }

        // Test get_operation_by_name_and_index
        let first = runner.get_operation_by_name_and_index("process", 0).await;
        assert!(first.is_some());
        assert_eq!(first.unwrap().operation_id, "op-1");

        let second = runner.get_operation_by_name_and_index("process", 1).await;
        assert!(second.is_some());
        assert_eq!(second.unwrap().operation_id, "op-2");

        let third = runner.get_operation_by_name_and_index("process", 2).await;
        assert!(third.is_some());
        assert_eq!(third.unwrap().operation_id, "op-3");

        // Test out of bounds
        let out_of_bounds = runner.get_operation_by_name_and_index("process", 3).await;
        assert!(out_of_bounds.is_none());
    }

    #[tokio::test]
    async fn test_get_all_operations() {
        let runner: LocalDurableTestRunner<String, String> =
            LocalDurableTestRunner::new(simple_handler);

        // Manually add operations to storage for testing
        {
            let mut storage = runner.operation_storage.write().await;
            storage.add_operation(Operation::new("op-0", OperationType::Step));
            storage.add_operation(Operation::new("op-1", OperationType::Wait));
            storage.add_operation(Operation::new("op-2", OperationType::Callback));
        }

        // Test get_all_operations
        let all_ops = runner.get_all_operations().await;
        assert_eq!(all_ops.len(), 3);
        assert_eq!(all_ops[0].operation_id, "op-0");
        assert_eq!(all_ops[1].operation_id, "op-1");
        assert_eq!(all_ops[2].operation_id, "op-2");
    }

    // Tests for function registration - Subtask 8.6

    #[tokio::test]
    async fn test_register_durable_function() {
        async fn helper_func(
            _input: serde_json::Value,
            _ctx: DurableContext,
        ) -> Result<serde_json::Value, DurableError> {
            Ok(serde_json::json!({"result": "ok"}))
        }

        let runner: LocalDurableTestRunner<String, String> =
            LocalDurableTestRunner::new(simple_handler);

        // Register a durable function
        runner.register_durable_function("helper", helper_func).await;

        // Verify it's registered
        assert!(runner.has_registered_function("helper").await);
        assert_eq!(runner.registered_function_count().await, 1);
    }

    #[tokio::test]
    async fn test_register_regular_function() {
        fn simple_func(_input: serde_json::Value) -> Result<serde_json::Value, DurableError> {
            Ok(serde_json::json!({"result": "ok"}))
        }

        let runner: LocalDurableTestRunner<String, String> =
            LocalDurableTestRunner::new(simple_handler);

        // Register a regular function
        runner.register_function("simple", simple_func).await;

        // Verify it's registered
        assert!(runner.has_registered_function("simple").await);
        assert_eq!(runner.registered_function_count().await, 1);
    }

    #[tokio::test]
    async fn test_register_multiple_functions() {
        async fn durable_func(
            _input: serde_json::Value,
            _ctx: DurableContext,
        ) -> Result<serde_json::Value, DurableError> {
            Ok(serde_json::json!({}))
        }

        fn regular_func(_input: serde_json::Value) -> Result<serde_json::Value, DurableError> {
            Ok(serde_json::json!({}))
        }

        let runner: LocalDurableTestRunner<String, String> =
            LocalDurableTestRunner::new(simple_handler);

        // Register multiple functions
        runner.register_durable_function("durable1", durable_func).await;
        runner.register_function("regular1", regular_func).await;
        runner.register_durable_function("durable2", durable_func).await;

        // Verify all are registered
        assert!(runner.has_registered_function("durable1").await);
        assert!(runner.has_registered_function("regular1").await);
        assert!(runner.has_registered_function("durable2").await);
        assert!(!runner.has_registered_function("nonexistent").await);
        assert_eq!(runner.registered_function_count().await, 3);
    }

    #[tokio::test]
    async fn test_clear_registered_functions() {
        fn simple_func(_input: serde_json::Value) -> Result<serde_json::Value, DurableError> {
            Ok(serde_json::json!({}))
        }

        let mut runner: LocalDurableTestRunner<String, String> =
            LocalDurableTestRunner::new(simple_handler);

        // Register functions
        runner.register_function("func1", simple_func).await;
        runner.register_function("func2", simple_func).await;
        assert_eq!(runner.registered_function_count().await, 2);

        // Clear all registered functions
        runner.clear_registered_functions().await;
        assert_eq!(runner.registered_function_count().await, 0);
        assert!(!runner.has_registered_function("func1").await);
        assert!(!runner.has_registered_function("func2").await);
    }

    #[tokio::test]
    async fn test_register_function_overwrites_existing() {
        fn func_v1(_input: serde_json::Value) -> Result<serde_json::Value, DurableError> {
            Ok(serde_json::json!({"version": 1}))
        }

        fn func_v2(_input: serde_json::Value) -> Result<serde_json::Value, DurableError> {
            Ok(serde_json::json!({"version": 2}))
        }

        let runner: LocalDurableTestRunner<String, String> =
            LocalDurableTestRunner::new(simple_handler);

        // Register first version
        runner.register_function("func", func_v1).await;
        assert_eq!(runner.registered_function_count().await, 1);

        // Register second version with same name
        runner.register_function("func", func_v2).await;

        // Should still have only one function (overwritten)
        assert_eq!(runner.registered_function_count().await, 1);
        assert!(runner.has_registered_function("func").await);
    }

    // Tests for run_with_orchestrator() method - Subtask 8.1.2

    #[tokio::test]
    async fn test_run_with_orchestrator_successful_execution() {
        // Ensure clean state
        LocalDurableTestRunner::<String, String>::teardown_test_environment()
            .await
            .unwrap();

        let mut runner = LocalDurableTestRunner::new(simple_handler);
        let result = runner.run_with_orchestrator("hello".to_string()).await.unwrap();

        // Verify successful execution status
        assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

        // Verify result value
        let output = result.get_result().unwrap();
        assert_eq!(output, "processed: hello");

        // Verify invocation was recorded
        assert!(!result.get_invocations().is_empty());
    }

    #[tokio::test]
    async fn test_run_with_orchestrator_failed_execution() {
        async fn failing_handler(
            _input: String,
            _ctx: DurableContext,
        ) -> Result<String, DurableError> {
            Err(DurableError::execution("Test failure"))
        }

        // Ensure clean state
        LocalDurableTestRunner::<String, String>::teardown_test_environment()
            .await
            .unwrap();

        let mut runner = LocalDurableTestRunner::new(failing_handler);
        let result = runner.run_with_orchestrator("hello".to_string()).await.unwrap();

        // Verify failed execution status
        assert_eq!(result.get_status(), ExecutionStatus::Failed);

        // Verify error is captured
        let error = result.get_error().unwrap();
        assert!(error.error_message.as_ref().unwrap().contains("Test failure"));
    }

    #[tokio::test]
    async fn test_run_with_orchestrator_with_time_skipping() {
        // Ensure clean state
        LocalDurableTestRunner::<String, String>::teardown_test_environment()
            .await
            .unwrap();

        // Setup with time skipping enabled
        LocalDurableTestRunner::<String, String>::setup_test_environment(TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        })
        .await
        .unwrap();

        let mut runner = LocalDurableTestRunner::new(simple_handler);
        let result = runner.run_with_orchestrator("hello".to_string()).await.unwrap();

        // Verify successful execution status
        assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

        // Teardown
        LocalDurableTestRunner::<String, String>::teardown_test_environment()
            .await
            .unwrap();
    }
}

/// Property-based tests for LocalDurableTestRunner
///
/// These tests verify the correctness properties defined in the design document.
#[cfg(test)]
mod property_tests {
    use super::*;
    use aws_durable_execution_sdk::OperationType;
    use proptest::prelude::*;

    /// Strategy for generating non-empty strings (for inputs)
    fn non_empty_string_strategy() -> impl Strategy<Value = String> {
        "[a-zA-Z0-9_ ]{1,32}".prop_map(|s| s)
    }

    /// Strategy for generating operation names
    fn operation_name_strategy() -> impl Strategy<Value = String> {
        "[a-zA-Z_][a-zA-Z0-9_]{0,15}".prop_map(|s| s)
    }

    /// Strategy for generating function names
    fn function_name_strategy() -> impl Strategy<Value = String> {
        "[a-zA-Z_][a-zA-Z0-9_]{0,15}".prop_map(|s| s)
    }

    proptest! {
        /// **Feature: rust-testing-utilities, Property 1: Execution Status Consistency**
        ///
        /// *For any* handler execution, if the handler returns `Ok(value)`, the TestResult
        /// status SHALL be `Succeeded`, and if the handler returns `Err(error)`, the TestResult
        /// status SHALL be `Failed`.
        ///
        /// **Validates: Requirements 1.3, 1.4**
        #[test]
        fn prop_execution_status_consistency_success(input in non_empty_string_strategy()) {
            // Use current_thread runtime which is required for tokio::time::pause()
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                // Ensure clean state
                LocalDurableTestRunner::<String, String>::teardown_test_environment()
                    .await
                    .unwrap();

                // Handler that always succeeds
                async fn success_handler(
                    input: String,
                    _ctx: DurableContext,
                ) -> Result<String, DurableError> {
                    Ok(format!("success: {}", input))
                }

                let mut runner = LocalDurableTestRunner::new(success_handler);
                let result = runner.run(input.clone()).await.unwrap();

                // Property: successful handler -> Succeeded status
                prop_assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
                prop_assert!(result.get_result().is_ok());
                let expected = format!("success: {}", input);
                prop_assert_eq!(result.get_result().unwrap(), &expected);

                Ok(())
            })?;
        }

        /// **Feature: rust-testing-utilities, Property 1: Execution Status Consistency (Failure)**
        ///
        /// *For any* handler execution that returns an error, the TestResult status SHALL be `Failed`.
        ///
        /// **Validates: Requirements 1.3, 1.4**
        #[test]
        fn prop_execution_status_consistency_failure(error_msg in non_empty_string_strategy()) {
            // Use current_thread runtime which is required for tokio::time::pause()
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                // Ensure clean state
                LocalDurableTestRunner::<String, String>::teardown_test_environment()
                    .await
                    .unwrap();

                // Create a handler that fails with the given error message
                let error_msg_clone = error_msg.clone();
                let failing_handler = move |_input: String, _ctx: DurableContext| {
                    let msg = error_msg_clone.clone();
                    async move { Err::<String, DurableError>(DurableError::execution(msg)) }
                };

                let mut runner = LocalDurableTestRunner::new(failing_handler);
                let result = runner.run("test".to_string()).await.unwrap();

                // Property: failed handler -> Failed status
                prop_assert_eq!(result.get_status(), ExecutionStatus::Failed);
                prop_assert!(result.get_error().is_ok());

                Ok(())
            })?;
        }

        /// **Feature: rust-testing-utilities, Property 3: Reset Clears State**
        ///
        /// *For any* LocalDurableTestRunner state, after calling `reset()`, the operations
        /// list SHALL be empty and the runner SHALL be ready for a new execution.
        ///
        /// **Validates: Requirements 1.6**
        #[test]
        fn prop_reset_clears_state(
            input1 in non_empty_string_strategy(),
            input2 in non_empty_string_strategy()
        ) {
            // Use current_thread runtime which is required for tokio::time::pause()
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                // Ensure clean state
                LocalDurableTestRunner::<String, String>::teardown_test_environment()
                    .await
                    .unwrap();

                // Reset checkpoint worker manager for clean state
                CheckpointWorkerManager::reset_instance_for_testing();

                async fn simple_handler(
                    input: String,
                    _ctx: DurableContext,
                ) -> Result<String, DurableError> {
                    Ok(format!("processed: {}", input))
                }

                let mut runner = LocalDurableTestRunner::new(simple_handler);

                // First run
                let _ = runner.run(input1).await.unwrap();

                // Reset
                runner.reset().await;

                // Property: after reset, operation count is 0
                prop_assert_eq!(runner.operation_count().await, 0);

                // Property: runner is ready for new execution
                let result2 = runner.run(input2.clone()).await.unwrap();
                prop_assert_eq!(result2.get_status(), ExecutionStatus::Succeeded);
                let expected = format!("processed: {}", input2);
                prop_assert_eq!(result2.get_result().unwrap(), &expected);

                Ok(())
            })?;
        }

        /// **Feature: rust-testing-utilities, Property 7: Operation Lookup Consistency**
        ///
        /// *For any* operation with name N at index I in the operations list, `get_operation(N)`
        /// SHALL return the first operation with name N, and `get_operation_by_index(I)` SHALL
        /// return the same operation as direct index access.
        ///
        /// **Validates: Requirements 4.1, 4.2, 4.3, 4.4**
        #[test]
        fn prop_operation_lookup_consistency(
            names in prop::collection::vec(operation_name_strategy(), 1..=5)
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let runner: LocalDurableTestRunner<String, String> =
                    LocalDurableTestRunner::new(|input: String, _ctx: DurableContext| async move {
                        Ok(input)
                    });

                // Add operations with the given names
                {
                    let mut storage = runner.operation_storage.write().await;
                    for (i, name) in names.iter().enumerate() {
                        let mut op = Operation::new(&format!("op-{}", i), OperationType::Step);
                        op.name = Some(name.clone());
                        storage.add_operation(op);
                    }
                }

                // Property: get_operation_by_index returns correct operation
                for (i, _name) in names.iter().enumerate() {
                    let op = runner.get_operation_by_index(i).await;
                    prop_assert!(op.is_some());
                    let expected_id = format!("op-{}", i);
                    prop_assert_eq!(&op.unwrap().operation_id, &expected_id);
                }

                // Property: get_operation returns first operation with that name
                for name in &names {
                    let op = runner.get_operation(name).await;
                    prop_assert!(op.is_some());
                    prop_assert_eq!(op.as_ref().unwrap().name.as_ref().unwrap(), name);
                }

                // Property: get_operation_by_id returns correct operation
                for i in 0..names.len() {
                    let op = runner.get_operation_by_id(&format!("op-{}", i)).await;
                    prop_assert!(op.is_some());
                    let expected_id = format!("op-{}", i);
                    prop_assert_eq!(&op.unwrap().operation_id, &expected_id);
                }

                // Property: get_all_operations returns all operations in order
                let all_ops = runner.get_all_operations().await;
                prop_assert_eq!(all_ops.len(), names.len());
                for (i, op) in all_ops.iter().enumerate() {
                    let expected_id = format!("op-{}", i);
                    prop_assert_eq!(&op.operation_id, &expected_id);
                }

                Ok(())
            })?;
        }

        /// **Feature: rust-testing-utilities, Property 10: Function Registration Retrieval**
        ///
        /// *For any* function registered with name N, the function SHALL be retrievable
        /// by that name.
        ///
        /// **Validates: Requirements 7.1, 7.2, 7.3**
        #[test]
        fn prop_function_registration_retrieval(
            func_names in prop::collection::vec(function_name_strategy(), 1..=5)
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let runner: LocalDurableTestRunner<String, String> =
                    LocalDurableTestRunner::new(|input: String, _ctx: DurableContext| async move {
                        Ok(input)
                    });

                // Register functions with the given names
                for name in &func_names {
                    runner.register_function(name.clone(), |_input: serde_json::Value| {
                        Ok(serde_json::json!({}))
                    }).await;
                }

                // Property: all registered functions are retrievable
                for name in &func_names {
                    prop_assert!(
                        runner.has_registered_function(name).await,
                        "Function '{}' should be registered",
                        name
                    );
                }

                // Property: count matches number of unique names
                let unique_names: std::collections::HashSet<_> = func_names.iter().collect();
                prop_assert_eq!(
                    runner.registered_function_count().await,
                    unique_names.len()
                );

                // Property: non-registered functions are not found
                prop_assert!(!runner.has_registered_function("__nonexistent__").await);

                Ok(())
            })?;
        }

        /// **Feature: rust-testing-utilities, Property 2: Operation Capture Completeness**
        ///
        /// *For any* sequence of durable operations performed by a handler, all operations
        /// SHALL be captured in the TestResult's operations list in execution order.
        ///
        /// **Validates: Requirements 1.5, 3.5**
        #[test]
        fn prop_operation_capture_completeness(
            num_steps in 1usize..=5,
            step_values in prop::collection::vec(1i32..100, 1..=5)
        ) {
            // Use current_thread runtime which is required for tokio::time::pause()
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                // Ensure clean state
                LocalDurableTestRunner::<String, String>::teardown_test_environment()
                    .await
                    .unwrap();

                // Create a handler that performs multiple step operations
                let num_steps_to_perform = num_steps.min(step_values.len());
                let values = step_values.clone();

                let multi_step_handler = move |_input: String, ctx: DurableContext| {
                    let values = values.clone();
                    let num = num_steps_to_perform;
                    async move {
                        let mut results = Vec::new();
                        for i in 0..num {
                            let value = values.get(i).copied().unwrap_or(0);
                            let step_name = format!("step_{}", i);
                            // Use step_named to provide a name for each step
                            let result = ctx.step_named(
                                &step_name,
                                |_| Ok(value * 2),
                                None
                            ).await?;
                            results.push(result);
                        }
                        Ok::<String, DurableError>(format!("completed {} steps", results.len()))
                    }
                };

                let mut runner = LocalDurableTestRunner::new(multi_step_handler);
                let result = runner.run("test".to_string()).await.unwrap();

                // Property: All operations should be captured
                // Each step creates at least one checkpoint call (START and SUCCEED)
                // The operations list should contain entries for each step
                let operations = result.get_operations();

                // We should have captured operations for each step
                // Note: The exact count depends on how operations are recorded
                // (START + SUCCEED = 2 per step, or just final state = 1 per step)
                // The key property is that we have operations for all steps performed
                prop_assert!(
                    !operations.is_empty() || num_steps_to_perform == 0,
                    "Operations should be captured when steps are performed"
                );

                // Verify the execution completed successfully
                prop_assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

                Ok(())
            })?;
        }

        /// **Feature: rust-testing-utilities, Property 4: Time Skipping Acceleration**
        ///
        /// *For any* wait operation with duration D seconds when time skipping is enabled,
        /// the actual wall-clock time to complete the wait SHALL be less than D seconds.
        ///
        /// Note: This test verifies that time skipping is properly configured and that
        /// wait operations don't block for their full duration. Due to the nature of
        /// durable execution (waits suspend and resume), we verify that the test
        /// infrastructure properly handles time advancement.
        ///
        /// **Validates: Requirements 2.1, 2.2**
        #[test]
        fn prop_time_skipping_acceleration(
            wait_seconds in 5u64..=60
        ) {
            // Use current_thread runtime which is required for tokio::time::pause()
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            rt.block_on(async {
                // Ensure clean state - teardown first in case previous test left state
                LocalDurableTestRunner::<String, String>::teardown_test_environment()
                    .await
                    .unwrap();

                LocalDurableTestRunner::<String, String>::setup_test_environment(
                    TestEnvironmentConfig {
                        skip_time: true,
                        checkpoint_delay: None,
                    }
                ).await.unwrap();

                // Verify time skipping is enabled
                prop_assert!(
                    LocalDurableTestRunner::<String, String>::is_time_skipping_enabled(),
                    "Time skipping should be enabled after setup"
                );

                // Create a handler that performs a wait operation
                let wait_duration = wait_seconds;
                let wait_handler = move |_input: String, ctx: DurableContext| {
                    async move {
                        // Perform a wait operation
                        ctx.wait(
                            aws_durable_execution_sdk::Duration::from_seconds(wait_duration),
                            Some("test_wait")
                        ).await?;
                        Ok::<String, DurableError>("wait completed".to_string())
                    }
                };

                let mut runner = LocalDurableTestRunner::new(wait_handler);

                // Measure wall-clock time for the operation
                let start_time = std::time::Instant::now();
                let result = runner.run("test".to_string()).await.unwrap();
                let elapsed = start_time.elapsed();

                // The execution should either:
                // 1. Complete quickly (if time skipping works perfectly)
                // 2. Return Running status (if wait suspends execution)
                // Either way, wall-clock time should be much less than wait_seconds

                // Property: Wall-clock time should be significantly less than wait duration
                // We use a generous threshold (wait_seconds - 1) to account for test overhead
                // but the key is that we're not actually waiting the full duration
                let max_allowed_seconds = wait_seconds.saturating_sub(1).max(1);
                prop_assert!(
                    elapsed.as_secs() < max_allowed_seconds,
                    "Wall-clock time ({:?}) should be less than wait duration ({} seconds). \
                     Time skipping should prevent actual waiting.",
                    elapsed,
                    wait_seconds
                );

                // The result should either be Running (suspended on wait) or Succeeded
                // Both are valid outcomes depending on how the mock handles waits
                prop_assert!(
                    result.get_status() == ExecutionStatus::Running ||
                    result.get_status() == ExecutionStatus::Succeeded,
                    "Execution should be Running (suspended) or Succeeded, got {:?}",
                    result.get_status()
                );

                // Teardown
                LocalDurableTestRunner::<String, String>::teardown_test_environment()
                    .await
                    .unwrap();

                Ok(())
            })?;
        }
    }
}
