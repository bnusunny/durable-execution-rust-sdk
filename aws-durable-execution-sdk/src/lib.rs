//! # AWS Durable Execution SDK for Lambda Rust Runtime
//!
//! This SDK enables Rust developers to build reliable, long-running workflows
//! in AWS Lambda with automatic checkpointing, replay, and state management.
//!
//! ## Overview
//!
//! The AWS Durable Execution SDK provides a framework for building workflows that can
//! survive Lambda function restarts, timeouts, and failures. It automatically checkpoints
//! the state of your workflow, allowing it to resume exactly where it left off after
//! any interruption.
//!
//! ### Key Features
//!
//! - **Automatic Checkpointing**: Every operation is automatically checkpointed, ensuring
//!   your workflow can resume from the last completed step.
//! - **Replay Mechanism**: When a function resumes, completed operations return their
//!   checkpointed results instantly without re-execution.
//! - **Concurrent Operations**: Process collections in parallel with configurable
//!   concurrency limits and failure tolerance.
//! - **External Integration**: Wait for callbacks from external systems with configurable
//!   timeouts.
//! - **Type Safety**: Full Rust type safety with generics and trait-based abstractions.
//!
//! ## Getting Started
//!
//! Add the SDK to your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! aws-durable-execution-sdk = "0.1"
//! tokio = { version = "1.0", features = ["full"] }
//! serde = { version = "1.0", features = ["derive"] }
//! ```
//!
//! ### Basic Workflow Example
//!
//! Here's a simple workflow that processes an order:
//!
//! ```rust,ignore
//! use aws_durable_execution_sdk::{durable_execution, DurableContext, DurableError, Duration};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Deserialize)]
//! struct OrderEvent {
//!     order_id: String,
//!     amount: f64,
//! }
//!
//! #[derive(Serialize)]
//! struct OrderResult {
//!     status: String,
//!     order_id: String,
//! }
//!
//! #[durable_execution]
//! async fn process_order(event: OrderEvent, ctx: DurableContext) -> Result<OrderResult, DurableError> {
//!     // Step 1: Validate the order (checkpointed automatically)
//!     let is_valid: bool = ctx.step(|_step_ctx| {
//!         // Validation logic here
//!         Ok(true)
//!     }, None).await?;
//!
//!     if !is_valid {
//!         return Err(DurableError::execution("Invalid order"));
//!     }
//!
//!     // Step 2: Process payment (checkpointed automatically)
//!     let payment_id: String = ctx.step(|_step_ctx| {
//!         // Payment processing logic here
//!         Ok("pay_123".to_string())
//!     }, None).await?;
//!
//!     // Step 3: Wait for payment confirmation (suspends Lambda, resumes later)
//!     ctx.wait(Duration::from_seconds(5), Some("payment_confirmation")).await?;
//!
//!     // Step 4: Complete the order
//!     Ok(OrderResult {
//!         status: "completed".to_string(),
//!         order_id: event.order_id,
//!     })
//! }
//! ```
//!
//! ## Core Concepts
//!
//! ### DurableContext
//!
//! The [`DurableContext`] is the main interface for durable operations. It provides:
//!
//! - [`step`](DurableContext::step): Execute and checkpoint a unit of work
//! - [`wait`](DurableContext::wait): Pause execution for a specified duration
//! - [`create_callback`](DurableContext::create_callback): Wait for external systems to signal completion
//! - [`invoke`](DurableContext::invoke): Call other durable Lambda functions
//! - [`map`](DurableContext::map): Process collections in parallel
//! - [`parallel`](DurableContext::parallel): Execute multiple operations concurrently
//! - [`run_in_child_context`](DurableContext::run_in_child_context): Create isolated nested workflows
//!
//! ### Steps
//!
//! Steps are the fundamental unit of work in durable executions. Each step is
//! automatically checkpointed, allowing the workflow to resume from the last
//! completed step after interruptions.
//!
//! ```rust,ignore
//! // Simple step
//! let result: i32 = ctx.step(|_| Ok(42), None).await?;
//!
//! // Named step for better debugging
//! let result: String = ctx.step_named("fetch_data", |_| {
//!     Ok("data".to_string())
//! }, None).await?;
//!
//! // Step with custom configuration
//! use aws_durable_execution_sdk::{StepConfig, StepSemantics};
//!
//! let config = StepConfig {
//!     step_semantics: StepSemantics::AtMostOncePerRetry,
//!     ..Default::default()
//! };
//! let result: i32 = ctx.step(|_| Ok(42), Some(config)).await?;
//! ```
//!
//! ### Step Semantics
//!
//! The SDK supports two execution semantics for steps:
//!
//! - **AtLeastOncePerRetry** (default): Checkpoint after execution. The step may
//!   execute multiple times if interrupted, but the result is always checkpointed.
//! - **AtMostOncePerRetry**: Checkpoint before execution. Guarantees the step
//!   executes at most once per retry, useful for non-idempotent operations.
//!
//! ### Wait Operations
//!
//! Wait operations suspend the Lambda execution and resume after the specified
//! duration. This is efficient because it doesn't block Lambda resources.
//!
//! ```rust,ignore
//! use aws_durable_execution_sdk::Duration;
//!
//! // Wait for 5 seconds
//! ctx.wait(Duration::from_seconds(5), None).await?;
//!
//! // Wait for 1 hour with a name
//! ctx.wait(Duration::from_hours(1), Some("wait_for_approval")).await?;
//! ```
//!
//! ### Callbacks
//!
//! Callbacks allow external systems to signal your workflow. Create a callback,
//! share the callback ID with an external system, and wait for the result.
//!
//! ```rust,ignore
//! use aws_durable_execution_sdk::CallbackConfig;
//!
//! // Create a callback with 24-hour timeout
//! let callback = ctx.create_callback::<ApprovalResponse>(Some(CallbackConfig {
//!     timeout: Duration::from_hours(24),
//!     ..Default::default()
//! })).await?;
//!
//! // Share callback.callback_id with external system
//! notify_approver(&callback.callback_id).await?;
//!
//! // Wait for the callback result (suspends until callback is received)
//! let approval = callback.result().await?;
//! ```
//!
//! ### Parallel Processing
//!
//! Process collections in parallel with configurable concurrency and failure tolerance:
//!
//! ```rust,ignore
//! use aws_durable_execution_sdk::{MapConfig, CompletionConfig};
//!
//! // Process items with max 5 concurrent executions
//! let results = ctx.map(
//!     vec![1, 2, 3, 4, 5],
//!     |child_ctx, item, index| async move {
//!         child_ctx.step(|_| Ok(item * 2), None).await
//!     },
//!     Some(MapConfig {
//!         max_concurrency: Some(5),
//!         completion_config: CompletionConfig::all_successful(),
//!         ..Default::default()
//!     }),
//! ).await?;
//!
//! // Get all successful results
//! let values = results.get_results()?;
//! ```
//!
//! ### Parallel Branches
//!
//! Execute multiple independent operations concurrently:
//!
//! ```rust,ignore
//! use aws_durable_execution_sdk::ParallelConfig;
//!
//! let results = ctx.parallel(
//!     vec![
//!         |ctx| Box::pin(async move { ctx.step(|_| Ok("a"), None).await }),
//!         |ctx| Box::pin(async move { ctx.step(|_| Ok("b"), None).await }),
//!         |ctx| Box::pin(async move { ctx.step(|_| Ok("c"), None).await }),
//!     ],
//!     None,
//! ).await?;
//! ```
//!
//! ## Configuration Types
//!
//! The SDK provides type-safe configuration for all operations:
//!
//! - [`StepConfig`]: Configure retry strategy, execution semantics, and serialization
//! - [`CallbackConfig`]: Configure timeout and heartbeat for callbacks
//! - [`InvokeConfig`]: Configure timeout and serialization for function invocations
//! - [`MapConfig`]: Configure concurrency, batching, and completion criteria for map operations
//! - [`ParallelConfig`]: Configure concurrency and completion criteria for parallel operations
//! - [`CompletionConfig`]: Define success/failure criteria for concurrent operations
//!
//! ### Completion Configuration
//!
//! Control when concurrent operations complete:
//!
//! ```rust
//! use aws_durable_execution_sdk::CompletionConfig;
//!
//! // Complete when first task succeeds
//! let first = CompletionConfig::first_successful();
//!
//! // Wait for all tasks to complete (regardless of success/failure)
//! let all = CompletionConfig::all_completed();
//!
//! // Require all tasks to succeed (zero failure tolerance)
//! let strict = CompletionConfig::all_successful();
//!
//! // Custom: require at least 3 successes
//! let custom = CompletionConfig::with_min_successful(3);
//! ```
//!
//! ## Error Handling
//!
//! The SDK provides a comprehensive error hierarchy through [`DurableError`]:
//!
//! - **Execution**: Errors that return FAILED status without Lambda retry
//! - **Invocation**: Errors that trigger Lambda retry
//! - **Checkpoint**: Checkpoint failures (retriable or non-retriable)
//! - **Callback**: Callback-specific failures
//! - **NonDeterministic**: Replay mismatches (operation type changed between runs)
//! - **Validation**: Invalid configuration or arguments
//! - **SerDes**: Serialization/deserialization failures
//! - **Suspend**: Signal to pause execution and return control to Lambda
//!
//! ```rust,ignore
//! use aws_durable_execution_sdk::DurableError;
//!
//! // Create specific error types
//! let exec_error = DurableError::execution("Something went wrong");
//! let validation_error = DurableError::validation("Invalid input");
//!
//! // Check error properties
//! if error.is_retriable() {
//!     // Handle retriable error
//! }
//! ```
//!
//! ## Custom Serialization
//!
//! The SDK uses JSON serialization by default, but you can provide custom
//! serializers by implementing the [`SerDes`] trait:
//!
//! ```rust,ignore
//! use aws_durable_execution_sdk::serdes::{SerDes, SerDesContext, SerDesError};
//!
//! struct MyCustomSerDes;
//!
//! impl SerDes<MyType> for MyCustomSerDes {
//!     fn serialize(&self, value: &MyType, context: &SerDesContext) -> Result<String, SerDesError> {
//!         // Custom serialization logic
//!         Ok(format!("{:?}", value))
//!     }
//!
//!     fn deserialize(&self, data: &str, context: &SerDesContext) -> Result<MyType, SerDesError> {
//!         // Custom deserialization logic
//!         todo!()
//!     }
//! }
//! ```
//!
//! ## Logging
//!
//! The SDK integrates with the `tracing` crate for structured logging. All operations
//! automatically include execution context (ARN, operation ID, parent ID) in log messages.
//!
//! You can also provide a custom logger by implementing the [`Logger`] trait:
//!
//! ```rust,ignore
//! use aws_durable_execution_sdk::{Logger, LogInfo};
//!
//! struct MyLogger;
//!
//! impl Logger for MyLogger {
//!     fn debug(&self, message: &str, info: &LogInfo) {
//!         println!("[DEBUG] {}: {:?}", message, info);
//!     }
//!     fn info(&self, message: &str, info: &LogInfo) {
//!         println!("[INFO] {}: {:?}", message, info);
//!     }
//!     fn warn(&self, message: &str, info: &LogInfo) {
//!         println!("[WARN] {}: {:?}", message, info);
//!     }
//!     fn error(&self, message: &str, info: &LogInfo) {
//!         println!("[ERROR] {}: {:?}", message, info);
//!     }
//! }
//! ```
//!
//! ## Duration Type
//!
//! The SDK provides a [`Duration`] type with convenient constructors:
//!
//! ```rust
//! use aws_durable_execution_sdk::Duration;
//!
//! let five_seconds = Duration::from_seconds(5);
//! let two_minutes = Duration::from_minutes(2);
//! let one_hour = Duration::from_hours(1);
//! let one_day = Duration::from_days(1);
//!
//! assert_eq!(five_seconds.to_seconds(), 5);
//! assert_eq!(two_minutes.to_seconds(), 120);
//! assert_eq!(one_hour.to_seconds(), 3600);
//! assert_eq!(one_day.to_seconds(), 86400);
//! ```
//!
//! ## Thread Safety
//!
//! The SDK is designed for use in async Rust with Tokio. All core types are
//! `Send + Sync` and can be safely shared across async tasks:
//!
//! - [`DurableContext`] uses `Arc` for shared state
//! - [`ExecutionState`] uses `RwLock` and atomic operations for thread-safe access
//! - Operation ID generation uses atomic counters
//!
//! ## Best Practices
//!
//! 1. **Keep steps small and focused**: Each step should do one thing well.
//!    This makes debugging easier and reduces the impact of failures.
//!
//! 2. **Use named operations**: Named steps and waits make logs and debugging
//!    much easier to understand.
//!
//! 3. **Handle errors appropriately**: Use `DurableError::execution` for errors
//!    that should fail the workflow, and `DurableError::invocation` for errors
//!    that should trigger a retry.
//!
//! 4. **Consider idempotency**: For operations that may be retried, ensure they
//!    are idempotent or use `AtMostOncePerRetry` semantics.
//!
//! 5. **Use appropriate concurrency limits**: When using `map` or `parallel`,
//!    set `max_concurrency` to avoid overwhelming downstream services.
//!
//! 6. **Set reasonable timeouts**: Always configure timeouts for callbacks and
//!    invocations to prevent workflows from hanging indefinitely.
//!
//! ## Module Organization
//!
//! - [`client`]: Lambda service client for checkpoint operations
//! - [`concurrency`]: Concurrent execution types (BatchResult, ConcurrentExecutor)
//! - [`config`]: Configuration types for all operations
//! - [`context`]: DurableContext and operation identifier types
//! - [`duration`]: Duration type with convenient constructors
//! - [`error`]: Error types and error handling
//! - [`handlers`]: Operation handlers (step, wait, callback, etc.)
//! - [`lambda`]: Lambda integration types (input/output)
//! - [`operation`]: Operation types and status enums
//! - [`serdes`]: Serialization/deserialization system
//! - [`state`]: Execution state and checkpointing system

pub mod client;
pub mod concurrency;
pub mod config;
pub mod context;
pub mod duration;
pub mod error;
pub mod handlers;
pub mod lambda;
pub mod operation;
pub mod serdes;
pub mod state;

// Re-export main types at crate root
pub use client::{
    CheckpointResponse, DurableServiceClient, GetOperationsResponse, LambdaClientConfig,
    LambdaDurableServiceClient, SharedDurableServiceClient,
};
pub use config::*;
pub use context::{
    DurableContext, LogInfo, Logger, OperationIdGenerator, OperationIdentifier, TracingLogger,
    generate_operation_id, WaitForConditionConfig, WaitForConditionContext,
};
pub use duration::Duration;
pub use error::{AwsError, DurableError, ErrorObject, TerminationReason};
pub use lambda::{
    DurableExecutionInvocationInput, DurableExecutionInvocationOutput, InitialExecutionState,
    InvocationStatus,
};
pub use operation::{
    Operation, OperationAction, OperationStatus, OperationType, OperationUpdate,
    WaitOptions, StepOptions, CallbackOptions, ChainedInvokeOptions, ContextOptions,
    ExecutionDetails, StepDetails, WaitDetails, CallbackDetails, ChainedInvokeDetails, ContextDetails,
};
pub use serdes::{JsonSerDes, SerDes, SerDesContext, SerDesError};
pub use state::{
    CheckpointBatcher, CheckpointBatcherConfig, CheckpointRequest, CheckpointSender,
    CheckpointedResult, ExecutionState, ReplayStatus, create_checkpoint_queue,
};

// Re-export concurrency types
pub use concurrency::{
    BatchItem, BatchItemStatus, BatchResult, CompletionReason, ConcurrentExecutor,
    ExecutionCounters,
};

// Re-export handlers
pub use handlers::{
    StepContext, step_handler,
    wait_handler,
    Callback, callback_handler,
    invoke_handler,
    child_handler,
    map_handler,
    parallel_handler,
};

// Re-export macro if enabled
#[cfg(feature = "macros")]
pub use aws_durable_execution_sdk_macros::durable_execution;
