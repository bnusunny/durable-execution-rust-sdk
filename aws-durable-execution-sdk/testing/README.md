# AWS Durable Execution SDK Testing Utilities

Testing utilities for the AWS Durable Execution SDK for Rust. This crate provides tools to test durable functions locally and against deployed AWS Lambda functions.

## Features

- **LocalDurableTestRunner** - Execute and test durable functions in-process with a simulated checkpoint backend
- **CloudDurableTestRunner** - Test against deployed Lambda functions in AWS
- **MockDurableServiceClient** - Mock checkpoint client for unit testing
- **Time Control** - Skip wait operations for faster test execution
- **Operation Inspection** - Inspect individual operations by name, index, or ID

## Installation

Add the testing crate as a dev dependency in your `Cargo.toml`:

```toml
[dev-dependencies]
aws-durable-execution-sdk-testing = "0.1.0-alpha1"
```

## Quick Start

### Local Testing

```rust
use aws_durable_execution_sdk::{durable_execution, DurableContext, DurableError, Duration};
use aws_durable_execution_sdk_testing::{
    LocalDurableTestRunner, TestEnvironmentConfig, ExecutionStatus,
};

// Define your durable workflow
async fn my_workflow(input: String, ctx: DurableContext) -> Result<String, DurableError> {
    let result = ctx.step(|_| Ok(format!("processed: {}", input)), None).await?;
    ctx.wait(Duration::from_seconds(5), Some("delay")).await?;
    Ok(result)
}

#[tokio::test]
async fn test_workflow() {
    // Set up test environment with time skipping enabled
    LocalDurableTestRunner::<String, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        }
    ).await.unwrap();

    // Create runner and execute
    let mut runner = LocalDurableTestRunner::new(my_workflow);
    let result = runner.run("hello".to_string()).await.unwrap();

    // Assert on results
    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
    assert_eq!(result.get_result().unwrap(), "processed: hello");

    // Inspect operations
    let ops = result.get_operations();
    assert_eq!(ops.len(), 2);

    // Clean up
    LocalDurableTestRunner::<String, String>::teardown_test_environment().await.unwrap();
}
```

### Cloud Testing

```rust
use aws_durable_execution_sdk_testing::{
    CloudDurableTestRunner, CloudTestRunnerConfig, ExecutionStatus,
};
use std::time::Duration;

#[tokio::test]
async fn test_deployed_workflow() {
    // Create runner for deployed Lambda function
    let mut runner = CloudDurableTestRunner::<String>::new("my-function-name")
        .await
        .unwrap()
        .with_config(CloudTestRunnerConfig {
            poll_interval: Duration::from_millis(500),
            timeout: Duration::from_secs(60),
        });

    // Execute and verify
    let result = runner.run("input".to_string()).await.unwrap();
    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
}
```

## API Reference

### LocalDurableTestRunner

The primary component for local testing. Executes durable handlers in-process with a simulated checkpoint backend.

```rust
use aws_durable_execution_sdk_testing::{LocalDurableTestRunner, TestEnvironmentConfig};

// Create a runner
let mut runner = LocalDurableTestRunner::new(my_workflow);

// Or with a custom mock client
let mock_client = MockDurableServiceClient::new()
    .with_checkpoint_responses(10);
let mut runner = LocalDurableTestRunner::with_mock_client(my_workflow, mock_client);

// Execute the workflow
let result = runner.run(input).await?;

// Reset for reuse between tests
runner.reset().await;
```

#### Test Environment Setup

```rust
// Set up before tests (typically in test setup)
LocalDurableTestRunner::<I, O>::setup_test_environment(TestEnvironmentConfig {
    skip_time: true,        // Skip wait operations instantly
    checkpoint_delay: None, // Optional simulated checkpoint delay
}).await?;

// Tear down after tests
LocalDurableTestRunner::<I, O>::teardown_test_environment().await?;
```

#### Operation Lookup

```rust
// Get operation by name (returns first match)
let op = runner.get_operation("my-step").await;

// Get operation by execution order index
let op = runner.get_operation_by_index(0).await;

// Get nth operation with a specific name
let op = runner.get_operation_by_name_and_index("my-step", 1).await;

// Get operation by unique ID
let op = runner.get_operation_by_id("op-123").await;

// Get all operations
let ops = runner.get_all_operations().await;
```

#### Function Registration (for Chained Invokes)

```rust
// Register a durable function
runner.register_durable_function("helper", |input, ctx| async move {
    // Durable function implementation
    Ok(serde_json::json!({"result": "done"}))
}).await;

// Register a regular function
runner.register_function("simple_helper", |input| {
    Ok(serde_json::json!({"result": "done"}))
}).await;
```

### CloudDurableTestRunner

For integration testing against deployed Lambda functions.

```rust
use aws_durable_execution_sdk_testing::{
    CloudDurableTestRunner, CloudTestRunnerConfig,
};
use aws_sdk_lambda::Client as LambdaClient;

// Create with default AWS config
let runner = CloudDurableTestRunner::<String>::new("function-name").await?;

// Create with custom Lambda client
let custom_client = LambdaClient::new(&aws_config);
let runner = CloudDurableTestRunner::<String>::with_client("function-name", custom_client);

// Configure polling behavior
let runner = runner.with_config(CloudTestRunnerConfig {
    poll_interval: Duration::from_millis(500),
    timeout: Duration::from_secs(60),
});

// Execute
let result = runner.run(payload).await?;
```

### TestResult

Contains execution results and provides inspection methods.

```rust
use aws_durable_execution_sdk_testing::{TestResult, ExecutionStatus, PrintConfig};
use aws_durable_execution_sdk::OperationStatus;

// Get execution status
let status = result.get_status();
assert_eq!(status, ExecutionStatus::Succeeded);

// Get result value (returns error if execution failed)
let value = result.get_result()?;

// Get error details (returns error if execution succeeded)
let error = result.get_error()?;

// Get all operations
let ops = result.get_operations();

// Filter operations by status
let succeeded = result.get_operations_by_status(OperationStatus::Succeeded);
let failed = result.get_operations_by_status(OperationStatus::Failed);

// Get invocation details
let invocations = result.get_invocations();

// Get history events
let events = result.get_history_events();

// Print operations table for debugging
result.print();

// Print with custom columns
result.print_with_config(PrintConfig::all());
result.print_with_config(PrintConfig::minimal());
```

#### ExecutionStatus

```rust
pub enum ExecutionStatus {
    Running,    // Execution is in progress
    Succeeded,  // Completed successfully
    Failed,     // Failed with an error
    Cancelled,  // Was cancelled
    TimedOut,   // Timed out
}
```

### DurableOperation

Represents a single operation with type-specific inspection methods.

```rust
use aws_durable_execution_sdk_testing::{DurableOperation, WaitingOperationStatus};
use aws_durable_execution_sdk::OperationType;

// Basic properties
let id = op.get_id();
let name = op.get_name();
let op_type = op.get_type();
let status = op.get_status();
let start_time = op.get_start_timestamp();
let end_time = op.get_end_timestamp();

// Check operation type
if op.is_callback() {
    // Handle callback operation
}

// Check completion status
if op.is_completed() {
    // Operation has finished
}
```

#### Type-Specific Details

```rust
// Step operation details
if op.get_type() == OperationType::Step {
    let details = op.get_step_details::<MyResultType>()?;
    println!("Attempt: {:?}", details.attempt);
    println!("Result: {:?}", details.result);
    println!("Error: {:?}", details.error);
}

// Wait operation details
if op.get_type() == OperationType::Wait {
    let details = op.get_wait_details()?;
    println!("Wait seconds: {:?}", details.wait_seconds);
    println!("Scheduled end: {:?}", details.scheduled_end_timestamp);
}

// Callback operation details
if op.get_type() == OperationType::Callback {
    let details = op.get_callback_details::<MyResultType>()?;
    println!("Callback ID: {:?}", details.callback_id);
    println!("Result: {:?}", details.result);
}

// Invoke operation details
if op.get_type() == OperationType::Invoke {
    let details = op.get_invoke_details::<MyResultType>()?;
    println!("Result: {:?}", details.result);
}

// Context operation details
if op.get_type() == OperationType::Context {
    let details = op.get_context_details::<MyResultType>()?;
    println!("Result: {:?}", details.result);
}
```

#### Callback Interaction

```rust
// Send callback responses (only for callback operations)
op.send_callback_success(r#"{"status": "approved"}"#).await?;
op.send_callback_failure(&TestResultError::new("Rejected", "Request denied")).await?;
op.send_callback_heartbeat().await?;
```

#### Async Waiting

```rust
// Wait for operation to reach a specific status
op.wait_for_data(WaitingOperationStatus::Started).await?;
op.wait_for_data(WaitingOperationStatus::Completed).await?;

// For callbacks, wait until ready to receive responses
op.wait_for_data(WaitingOperationStatus::Submitted).await?;
```

### MockDurableServiceClient

A mock implementation of the checkpoint client for unit testing.

```rust
use aws_durable_execution_sdk_testing::{MockDurableServiceClient, DurableServiceClient};
use aws_durable_execution_sdk::{CheckpointResponse, Operation, OperationType, DurableError};

// Create with default responses
let client = MockDurableServiceClient::new();

// Configure custom checkpoint responses
let client = MockDurableServiceClient::new()
    .with_checkpoint_response(Ok(CheckpointResponse::new("token-1")))
    .with_checkpoint_response(Ok(CheckpointResponse::new("token-2")))
    .with_checkpoint_response(Err(DurableError::checkpoint_retriable("Temp error")));

// Add multiple default responses
let client = MockDurableServiceClient::new()
    .with_checkpoint_responses(10);

// Add response with operations (e.g., for callback IDs)
let op = Operation::new("callback-1", OperationType::Callback);
let client = MockDurableServiceClient::new()
    .with_checkpoint_response_with_operations("token-1", vec![op]);

// Verify calls made to the mock
let calls = client.get_checkpoint_calls();
assert_eq!(calls.len(), 1);
assert_eq!(calls[0].checkpoint_token, "token-123");

// Clear recorded calls for reuse
client.clear_checkpoint_calls();
client.clear_all_calls();
```

### Error Handling

```rust
use aws_durable_execution_sdk_testing::TestError;

match result {
    Err(TestError::ExecutionFailed(e)) => {
        // Handler execution failed
    }
    Err(TestError::OperationNotFound(name)) => {
        // Operation with given name not found
    }
    Err(TestError::OperationTypeMismatch { expected, found }) => {
        // Wrong operation type for requested details
    }
    Err(TestError::NotCallbackOperation) => {
        // Callback method called on non-callback operation
    }
    Err(TestError::FunctionNotRegistered(name)) => {
        // Invoked function not registered
    }
    Err(TestError::WaitTimeout(op)) => {
        // Timeout waiting for operation
    }
    Err(TestError::ExecutionCompletedEarly(op, status)) => {
        // Execution completed before operation reached target status
    }
    Err(TestError::SerializationError(e)) => {
        // JSON serialization/deserialization error
    }
    Err(TestError::AwsError(msg)) => {
        // AWS SDK error
    }
    Err(TestError::EnvironmentNotSetUp) => {
        // Test environment not initialized
    }
    _ => {}
}
```

## Testing Patterns

### Testing Workflows with Steps

```rust
#[tokio::test]
async fn test_multi_step_workflow() {
    LocalDurableTestRunner::<_, _>::setup_test_environment(
        TestEnvironmentConfig::default()
    ).await.unwrap();

    let mut runner = LocalDurableTestRunner::new(my_workflow);
    let result = runner.run(input).await.unwrap();

    // Verify all steps completed
    let ops = result.get_operations();
    let succeeded = result.get_operations_by_status(OperationStatus::Succeeded);
    assert_eq!(succeeded.len(), ops.len());

    // Verify specific step results
    if let Some(op) = runner.get_operation("process-data").await {
        let details = DurableOperation::new(op).get_step_details::<MyResult>().unwrap();
        assert!(details.result.is_some());
    }

    LocalDurableTestRunner::<_, _>::teardown_test_environment().await.unwrap();
}
```

### Testing Callbacks

```rust
#[tokio::test]
async fn test_callback_workflow() {
    LocalDurableTestRunner::<_, _>::setup_test_environment(
        TestEnvironmentConfig::default()
    ).await.unwrap();

    let mut runner = LocalDurableTestRunner::new(approval_workflow);
    
    // Start execution (will suspend at callback)
    let result = runner.run(request).await.unwrap();
    
    // Find the callback operation
    if let Some(op) = runner.get_operation("approval-callback").await {
        let durable_op = DurableOperation::new(op);
        
        // Send approval response
        durable_op.send_callback_success(r#"{"approved": true}"#).await.unwrap();
    }

    LocalDurableTestRunner::<_, _>::teardown_test_environment().await.unwrap();
}
```

### Testing Error Handling

```rust
#[tokio::test]
async fn test_error_handling() {
    LocalDurableTestRunner::<_, _>::setup_test_environment(
        TestEnvironmentConfig::default()
    ).await.unwrap();

    let mut runner = LocalDurableTestRunner::new(failing_workflow);
    let result = runner.run(bad_input).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Failed);
    
    let error = result.get_error().unwrap();
    assert_eq!(error.error_type, Some("ValidationError".to_string()));

    LocalDurableTestRunner::<_, _>::teardown_test_environment().await.unwrap();
}
```

### Testing with Chained Invokes

```rust
#[tokio::test]
async fn test_chained_invokes() {
    LocalDurableTestRunner::<_, _>::setup_test_environment(
        TestEnvironmentConfig::default()
    ).await.unwrap();

    let mut runner = LocalDurableTestRunner::new(orchestrator_workflow);
    
    // Register helper functions
    runner.register_durable_function("process-item", process_item_workflow).await;
    runner.register_function("validate", validate_fn).await;

    let result = runner.run(items).await.unwrap();
    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    LocalDurableTestRunner::<_, _>::teardown_test_environment().await.unwrap();
}
```

## License

This project is licensed under the Apache-2.0 License.
