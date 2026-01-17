//! Testing utilities for AWS Durable Execution SDK
//!
//! This crate provides tools for testing durable functions locally and against
//! deployed AWS Lambda functions.
//!
//! # Features
//!
//! - **LocalDurableTestRunner**: Execute and test durable functions in-process
//! - **CloudDurableTestRunner**: Test against deployed Lambda functions
//! - **MockDurableServiceClient**: Mock checkpoint client for unit testing
//! - **DurableOperation**: Inspect and interact with individual operations
//! - **Time Control**: Skip wait operations for faster test execution
//!
//! # Example
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

// Module declarations
pub mod checkpoint_server;
pub mod cloud_runner;
pub mod error;
pub mod local_runner;
pub mod mock_client;
pub mod operation;
pub mod test_result;
pub mod time_control;
pub mod types;

// Re-export core types from this crate
pub use checkpoint_server::{CheckpointWorkerManager, CheckpointWorkerParams};
pub use cloud_runner::{CloudDurableTestRunner, CloudTestRunnerConfig};
pub use error::TestError;
pub use local_runner::{LocalDurableTestRunner, TestEnvironmentConfig};
pub use mock_client::{CheckpointCall, GetOperationsCall, MockDurableServiceClient};
pub use operation::{
    CallbackDetails, CallbackSender, ContextDetails, DurableOperation, InvokeDetails, StepDetails,
    WaitDetails,
};
pub use test_result::{HistoryEvent, PrintConfig, TestResult};
pub use time_control::{TimeControl, TimeControlGuard};
pub use types::{ExecutionStatus, Invocation, TestResultError, WaitingOperationStatus};

// Re-export key types from the SDK for convenience
pub use aws_durable_execution_sdk::{
    // Client types
    client::NewExecutionState,
    // Operation types
    CallbackDetails as SdkCallbackDetails,
    ChainedInvokeDetails,
    CheckpointResponse,
    ContextDetails as SdkContextDetails,
    // Context and handlers
    DurableContext,
    // Error types
    DurableError,
    DurableResult,
    DurableServiceClient,
    // Duration
    Duration,
    ErrorObject,
    GetOperationsResponse,
    Operation,
    OperationStatus,
    OperationType,
    OperationUpdate,
    SharedDurableServiceClient,
    StepDetails as SdkStepDetails,
    WaitDetails as SdkWaitDetails,
};
