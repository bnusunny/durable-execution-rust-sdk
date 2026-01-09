# Design Document: AWS Durable Execution SDK for Rust

## Usage Example

Here's how developers would use the SDK in a Rust Lambda function:

```rust
use aws_durable_execution_sdk::{
    durable_execution, DurableContext, StepConfig, Duration, DurableError,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct OrderEvent {
    order_id: String,
    amount: f64,
}

#[derive(Serialize, Deserialize)]
struct ValidationResult {
    order_id: String,
    valid: bool,
}

#[derive(Serialize, Deserialize)]
struct PaymentResult {
    transaction_id: String,
    status: String,
}

#[derive(Serialize, Deserialize)]
struct FulfillmentResult {
    tracking_number: String,
}

#[derive(Serialize)]
struct OrderResult {
    status: String,
    order_id: String,
    transaction_id: String,
    tracking_number: String,
}

// The #[durable_execution] macro transforms this into a Lambda handler
// that integrates with AWS Lambda's durable execution service
#[durable_execution]
async fn process_order(event: OrderEvent, ctx: DurableContext) -> Result<OrderResult, DurableError> {
    let order_id = event.order_id.clone();
    let amount = event.amount;

    // Step 1: Validate the order (checkpointed automatically)
    let validation: ValidationResult = ctx.step(|_step_ctx| {
        // Validation logic here
        Ok(ValidationResult {
            order_id: order_id.clone(),
            valid: true,
        })
    }, None).await?;

    if !validation.valid {
        return Err(DurableError::Execution {
            message: "Invalid order".to_string(),
            termination_reason: TerminationReason::ExecutionError,
        });
    }

    // Step 2: Charge payment (checkpointed automatically)
    let payment: PaymentResult = ctx.step(|_step_ctx| {
        // Payment processing logic here
        Ok(PaymentResult {
            transaction_id: "txn_123".to_string(),
            status: "completed".to_string(),
        })
    }, None).await?;

    // Step 3: Wait for payment confirmation (suspends Lambda, resumes later)
    ctx.wait(Duration::from_seconds(5), Some("payment_confirmation")).await?;

    // Step 4: Fulfill the order (checkpointed automatically)
    let fulfillment: FulfillmentResult = ctx.step(|_step_ctx| {
        // Fulfillment logic here
        Ok(FulfillmentResult {
            tracking_number: "TRK123456".to_string(),
        })
    }, None).await?;

    Ok(OrderResult {
        status: "completed".to_string(),
        order_id,
        transaction_id: payment.transaction_id,
        tracking_number: fulfillment.tracking_number,
    })
}
```

## Overview

This document describes the technical design for implementing the AWS Durable Execution SDK for the Lambda Rust Runtime, conforming to the AWS Lambda Durable Functions Language SDK Specification v1.2. The SDK enables Rust developers to build reliable, long-running workflows in AWS Lambda with automatic checkpointing, replay, and state management.

The design follows idiomatic Rust patterns including:
- Trait-based abstractions for extensibility (SerDes, Logger)
- Result-based error handling with a comprehensive error hierarchy
- Async/await with Tokio for concurrent operations
- Zero-cost abstractions where possible
- Strong type safety with generics

## Architecture

The SDK follows a layered architecture:

```
┌─────────────────────────────────────────────────────────────┐
│                    User Handler Code                         │
│              #[durable_execution] async fn handler()         │
├─────────────────────────────────────────────────────────────┤
│                     DurableContext                           │
│    step() | wait() | callback() | map() | parallel()        │
├─────────────────────────────────────────────────────────────┤
│                   Operation Handlers                         │
│  StepHandler | WaitHandler | CallbackHandler | MapHandler   │
├─────────────────────────────────────────────────────────────┤
│                    ExecutionState                            │
│         Checkpoint Management | Replay | State Tracking      │
├─────────────────────────────────────────────────────────────┤
│                  Checkpointing System                        │
│      Background Task | Batching | Sync/Async Checkpoints    │
├─────────────────────────────────────────────────────────────┤
│                   Lambda Service Client                      │
│              aws-sdk-lambda | API Calls                      │
└─────────────────────────────────────────────────────────────┘
```

## Components and Interfaces

### DurableContext

The main interface for user code. Provides all durable operations.

```rust
pub struct DurableContext {
    state: Arc<ExecutionState>,
    lambda_context: Option<lambda_runtime::Context>,
    parent_id: Option<String>,
    step_counter: Arc<AtomicU64>,
    logger: Arc<dyn Logger>,
    original_input: Option<String>,
}

impl DurableContext {
    // Core operations
    pub async fn step<T, F>(&self, func: F, config: Option<StepConfig>) -> Result<T, DurableError>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce(StepContext) -> Result<T, Box<dyn std::error::Error>> + Send;

    pub async fn wait(&self, duration: Duration, name: Option<&str>) -> Result<(), DurableError>;

    pub async fn cancel_wait(&self, operation_id: &str) -> Result<(), DurableError>;

    pub async fn create_callback<T>(&self, config: Option<CallbackConfig>) -> Result<Callback<T>, DurableError>
    where
        T: DeserializeOwned;

    pub async fn invoke<P, R>(&self, function_name: &str, payload: P, config: Option<InvokeConfig<P, R>>) -> Result<R, DurableError>
    where
        P: Serialize,
        R: DeserializeOwned;

    pub async fn map<T, U, F>(&self, inputs: Vec<T>, func: F, config: Option<MapConfig>) -> Result<BatchResult<U>, DurableError>
    where
        T: Serialize + DeserializeOwned + Send + Sync,
        U: Serialize + DeserializeOwned + Send,
        F: Fn(&DurableContext, T, usize) -> Result<U, Box<dyn std::error::Error>> + Send + Sync;

    pub async fn parallel<T, F>(&self, functions: Vec<F>, config: Option<ParallelConfig>) -> Result<BatchResult<T>, DurableError>
    where
        T: Serialize + DeserializeOwned + Send,
        F: Fn(&DurableContext) -> Result<T, Box<dyn std::error::Error>> + Send + Sync;

    pub async fn run_in_child_context<T, F>(&self, func: F, config: Option<ContextConfig>) -> Result<T, DurableError>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce(&DurableContext) -> Result<T, Box<dyn std::error::Error>> + Send;

    pub async fn wait_for_condition<T, F>(&self, check: F, config: WaitForConditionConfig<T>) -> Result<T, DurableError>
    where
        T: Serialize + DeserializeOwned + Clone,
        F: Fn(T, &WaitForConditionContext) -> Result<T, Box<dyn std::error::Error>> + Send + Sync;

    // Promise combinators
    pub async fn all<T>(&self, futures: Vec<impl Future<Output = Result<T, DurableError>>>) -> Result<Vec<T>, DurableError>;
    pub async fn all_settled<T>(&self, futures: Vec<impl Future<Output = Result<T, DurableError>>>) -> BatchResult<T>;
    pub async fn race<T>(&self, futures: Vec<impl Future<Output = Result<T, DurableError>>>) -> Result<T, DurableError>;
    pub async fn any<T>(&self, futures: Vec<impl Future<Output = Result<T, DurableError>>>) -> Result<T, DurableError>;

    // Access to original input
    pub fn get_original_input<T: DeserializeOwned>(&self) -> Result<T, DurableError>;

    // Internal helpers
    fn create_operation_id(&self) -> String;
    fn create_child_context(&self, parent_id: String) -> DurableContext;
}
```

### ExecutionState

Manages checkpoint state, replay, and communication with the Lambda service.

```rust
pub struct ExecutionState {
    durable_execution_arn: String,
    checkpoint_token: RwLock<String>,
    operations: RwLock<HashMap<String, Operation>>,
    service_client: Arc<dyn DurableServiceClient>,
    replay_status: AtomicU8, // ReplayStatus enum
    checkpoint_queue: mpsc::Sender<CheckpointRequest>,
    parent_done_lock: Mutex<HashSet<String>>,
    execution_operation: Option<Operation>,
    checkpointing_mode: CheckpointingMode,
}

#[derive(Clone, Copy, Default)]
pub enum CheckpointingMode {
    Eager,      // Checkpoint after every operation
    #[default]
    Batched,    // Group operations per checkpoint
    Optimistic, // Execute multiple operations before checkpointing
}

impl ExecutionState {
    pub fn get_checkpoint_result(&self, operation_id: &str) -> CheckpointedResult;
    pub async fn create_checkpoint(&self, operation: OperationUpdate, is_sync: bool) -> Result<(), CheckpointError>;
    pub fn track_replay(&self, operation_id: &str);
    pub fn mark_parent_done(&self, parent_id: &str);
    pub fn is_orphaned(&self, parent_id: &str) -> bool;
    pub fn get_original_input(&self) -> Option<&str>;
    pub fn is_operation_ready(&self, operation_id: &str) -> bool;
}
```

### Checkpointing System

Background task that batches and sends checkpoints to the Lambda service.

```rust
pub struct CheckpointBatcher {
    config: CheckpointBatcherConfig,
    queue_rx: mpsc::Receiver<CheckpointRequest>,
    service_client: Arc<dyn DurableServiceClient>,
    state: Arc<ExecutionState>,
}

#[derive(Clone)]
pub struct CheckpointBatcherConfig {
    pub max_batch_size_bytes: usize,    // 750KB default
    pub max_batch_time: Duration,        // 1 second default
    pub max_batch_operations: usize,     // unlimited default
}

pub struct CheckpointRequest {
    operation: OperationUpdate,
    completion: Option<oneshot::Sender<Result<(), CheckpointError>>>,
}

impl CheckpointBatcher {
    pub async fn run(&mut self) {
        // Collect operations until batch limits reached
        // Ensure execution order: child ops after parent CONTEXT starts
        // EXECUTION completion must be last
        // Send batch to Lambda service
        // Signal completion to waiting callers
    }
}
```

### Operation Handlers

Each operation type has a dedicated handler module.

```rust
// Step handler
pub async fn step_handler<T, F>(
    func: F,
    state: &ExecutionState,
    op_id: &OperationIdentifier,
    config: &StepConfig,
    logger: &dyn Logger,
) -> Result<T, DurableError>
where
    T: Serialize + DeserializeOwned,
    F: FnOnce(StepContext) -> Result<T, Box<dyn std::error::Error>>;

// Wait handler
pub async fn wait_handler(
    seconds: u64,
    state: &ExecutionState,
    op_id: &OperationIdentifier,
) -> Result<(), DurableError>;

// Wait cancel handler
pub async fn wait_cancel_handler(
    state: &ExecutionState,
    op_id: &str,
) -> Result<(), DurableError>;

// Callback handler
pub async fn callback_handler(
    state: &ExecutionState,
    op_id: &OperationIdentifier,
    config: &CallbackConfig,
) -> Result<String, DurableError>;

// Invoke handler
pub async fn invoke_handler<P, R>(
    function_name: &str,
    payload: &P,
    state: &ExecutionState,
    op_id: &OperationIdentifier,
    config: &InvokeConfig<P, R>,
) -> Result<R, DurableError>
where
    P: Serialize,
    R: DeserializeOwned;

// Map handler
pub async fn map_handler<T, U, F>(
    items: Vec<T>,
    func: F,
    config: &MapConfig,
    state: &ExecutionState,
    context: &DurableContext,
    op_id: &OperationIdentifier,
) -> Result<BatchResult<U>, DurableError>;

// Parallel handler
pub async fn parallel_handler<T, F>(
    callables: Vec<F>,
    config: &ParallelConfig,
    state: &ExecutionState,
    context: &DurableContext,
    op_id: &OperationIdentifier,
) -> Result<BatchResult<T>, DurableError>;

// Child context handler
pub async fn child_handler<T, F>(
    func: F,
    state: &ExecutionState,
    context: &DurableContext,
    op_id: &OperationIdentifier,
    config: &ContextConfig,
) -> Result<T, DurableError>
where
    T: Serialize + DeserializeOwned,
    F: FnOnce(&DurableContext) -> Result<T, Box<dyn std::error::Error>>;
```

### Concurrency System

Handles map and parallel operations with configurable concurrency.

```rust
pub struct ConcurrentExecutor<T> {
    max_concurrency: Option<usize>,
    completion_config: CompletionConfig,
    counters: ExecutionCounters,
    completion_notify: Arc<Notify>,
}

pub struct ExecutionCounters {
    total_tasks: AtomicUsize,
    success_count: AtomicUsize,
    failure_count: AtomicUsize,
    completed_count: AtomicUsize,
}

pub struct ExecutableWithState<T> {
    index: usize,
    status: AtomicU8, // BranchStatus enum
    result: Mutex<Option<Result<T, DurableError>>>,
    suspend_until: AtomicU64,
}

impl<T> ConcurrentExecutor<T> {
    pub async fn execute<F, Fut>(&self, tasks: Vec<F>) -> BatchResult<T>
    where
        F: FnOnce(usize) -> Fut + Send,
        Fut: Future<Output = Result<T, DurableError>> + Send;
}
```

### SerDes System

Trait-based serialization with default JSON implementation.

```rust
pub trait SerDes<T>: Send + Sync {
    fn serialize(&self, value: &T, context: &SerDesContext) -> Result<String, SerDesError>;
    fn deserialize(&self, data: &str, context: &SerDesContext) -> Result<T, SerDesError>;
}

pub struct SerDesContext {
    pub operation_id: String,
    pub durable_execution_arn: String,
}

pub struct JsonSerDes<T>(PhantomData<T>);

impl<T: Serialize + DeserializeOwned> SerDes<T> for JsonSerDes<T> {
    fn serialize(&self, value: &T, _context: &SerDesContext) -> Result<String, SerDesError> {
        serde_json::to_string(value).map_err(|e| SerDesError::Serialization(e.to_string()))
    }

    fn deserialize(&self, data: &str, _context: &SerDesContext) -> Result<T, SerDesError> {
        serde_json::from_str(data).map_err(|e| SerDesError::Deserialization(e.to_string()))
    }
}
```

### Replay-Safe Helpers

Optional helpers for generating deterministic non-deterministic values.

```rust
pub struct ReplaySafeHelpers;

impl ReplaySafeHelpers {
    /// Generate a deterministic UUID seeded by operation ID
    pub fn uuid_from_operation(operation_id: &str, seed: u64) -> Uuid {
        // Use blake2b hash of operation_id + seed to generate deterministic UUID
    }

    /// Get replay-safe timestamp from execution state
    pub fn timestamp_from_execution(state: &ExecutionState) -> u64 {
        // Return StartTimestamp from EXECUTION operation
    }
}
```

## Data Models

### Configuration Types

```rust
#[derive(Clone, Default)]
pub struct StepConfig {
    pub retry_strategy: Option<Box<dyn RetryStrategy>>,
    pub step_semantics: StepSemantics,
    pub serdes: Option<Arc<dyn SerDes<serde_json::Value>>>,
}

#[derive(Clone, Copy, Default)]
pub enum StepSemantics {
    AtMostOncePerRetry,
    #[default]
    AtLeastOncePerRetry,
}

#[derive(Clone, Default)]
pub struct CallbackConfig {
    pub timeout_seconds: Option<u64>,
    pub heartbeat_timeout_seconds: Option<u64>,
    pub serdes: Option<Arc<dyn SerDes<serde_json::Value>>>,
}

#[derive(Clone, Default)]
pub struct InvokeConfig<P, R> {
    pub serdes_payload: Option<Arc<dyn SerDes<P>>>,
    pub serdes_result: Option<Arc<dyn SerDes<R>>>,
    pub tenant_id: Option<String>,
}

#[derive(Clone, Default)]
pub struct MapConfig {
    pub max_concurrency: Option<usize>,
    pub item_batcher: Option<ItemBatcher>,
    pub completion_config: CompletionConfig,
    pub serdes: Option<Arc<dyn SerDes<serde_json::Value>>>,
}

#[derive(Clone, Default)]
pub struct ParallelConfig {
    pub max_concurrency: Option<usize>,
    pub completion_config: CompletionConfig,
    pub serdes: Option<Arc<dyn SerDes<serde_json::Value>>>,
}

#[derive(Clone, Default)]
pub struct ContextConfig {
    pub replay_children: bool,
    pub serdes: Option<Arc<dyn SerDes<serde_json::Value>>>,
}

#[derive(Clone, Default)]
pub struct CompletionConfig {
    pub min_successful: Option<usize>,
    pub tolerated_failure_count: Option<usize>,
    pub tolerated_failure_percentage: Option<f64>,
}

impl CompletionConfig {
    pub fn first_successful() -> Self { Self { min_successful: Some(1), ..Default::default() } }
    pub fn all_completed() -> Self { Self::default() }
    pub fn all_successful() -> Self { Self { tolerated_failure_count: Some(0), tolerated_failure_percentage: Some(0.0), ..Default::default() } }
}

#[derive(Clone)]
pub struct ItemBatcher {
    pub max_items_per_batch: usize,
    pub max_bytes_per_batch: usize,
}
```

### Result Types

```rust
pub struct BatchResult<T> {
    pub items: Vec<BatchItem<T>>,
    pub completion_reason: CompletionReason,
}

pub struct BatchItem<T> {
    pub index: usize,
    pub status: BatchItemStatus,
    pub result: Option<T>,
    pub error: Option<ErrorObject>,
}

#[derive(Clone, Copy)]
pub enum BatchItemStatus {
    Started,
    Succeeded,
    Failed,
    Cancelled,
    Pending,
}

#[derive(Clone, Copy)]
pub enum CompletionReason {
    AllCompleted,
    MinSuccessfulReached,
    FailureToleranceExceeded,
    Suspended,
}

impl<T> BatchResult<T> {
    pub fn succeeded(&self) -> Vec<&BatchItem<T>>;
    pub fn failed(&self) -> Vec<&BatchItem<T>>;
    pub fn get_results(&self) -> Result<Vec<&T>, DurableError>;
    pub fn success_count(&self) -> usize;
    pub fn failure_count(&self) -> usize;
    pub fn total_count(&self) -> usize;
    pub fn has_failures(&self) -> bool;
    pub fn throw_if_errors(&self) -> Result<(), DurableError>;
}
```

### Lambda Integration Types

```rust
#[derive(Deserialize)]
pub struct DurableExecutionInvocationInput {
    #[serde(rename = "DurableExecutionArn")]
    pub durable_execution_arn: String,
    #[serde(rename = "CheckpointToken")]
    pub checkpoint_token: String,
    #[serde(rename = "InitialExecutionState")]
    pub initial_execution_state: InitialExecutionState,
}

#[derive(Deserialize)]
pub struct InitialExecutionState {
    #[serde(rename = "Operations")]
    pub operations: Vec<Operation>,
    #[serde(rename = "NextMarker")]
    pub next_marker: Option<String>,
}

#[derive(Serialize)]
pub struct DurableExecutionInvocationOutput {
    #[serde(rename = "Status")]
    pub status: InvocationStatus,
    #[serde(rename = "Result", skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(rename = "Error", skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
}

#[derive(Serialize, Clone, Copy)]
pub enum InvocationStatus {
    SUCCEEDED,
    FAILED,
    PENDING,
}
```

### Operation Types

```rust
#[derive(Clone, Deserialize, Serialize)]
pub struct Operation {
    #[serde(rename = "Id")]
    pub id: String,
    #[serde(rename = "Type")]
    pub operation_type: OperationType,
    #[serde(rename = "Status")]
    pub status: OperationStatus,
    #[serde(rename = "Payload", skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
    #[serde(rename = "Error", skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
    #[serde(rename = "ParentId", skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(rename = "Name", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "SubType", skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<String>,
    #[serde(rename = "StartTimestamp", skip_serializing_if = "Option::is_none")]
    pub start_timestamp: Option<f64>,
    #[serde(rename = "EndTimestamp", skip_serializing_if = "Option::is_none")]
    pub end_timestamp: Option<f64>,
    // Type-specific details
    #[serde(rename = "ExecutionDetails", skip_serializing_if = "Option::is_none")]
    pub execution_details: Option<ExecutionDetails>,
    #[serde(rename = "StepDetails", skip_serializing_if = "Option::is_none")]
    pub step_details: Option<StepDetails>,
    #[serde(rename = "WaitDetails", skip_serializing_if = "Option::is_none")]
    pub wait_details: Option<WaitDetails>,
    #[serde(rename = "CallbackDetails", skip_serializing_if = "Option::is_none")]
    pub callback_details: Option<CallbackDetails>,
    #[serde(rename = "ChainedInvokeDetails", skip_serializing_if = "Option::is_none")]
    pub chained_invoke_details: Option<ChainedInvokeDetails>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct ExecutionDetails {
    #[serde(rename = "InputPayload")]
    pub input_payload: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct StepDetails {
    #[serde(rename = "Attempt")]
    pub attempt: Option<u32>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct WaitDetails {
    #[serde(rename = "WaitSeconds")]
    pub wait_seconds: Option<u64>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct CallbackDetails {
    #[serde(rename = "CallbackId")]
    pub callback_id: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct ChainedInvokeDetails {
    #[serde(rename = "FunctionName")]
    pub function_name: Option<String>,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub enum OperationType {
    #[serde(rename = "EXECUTION")]
    Execution,
    #[serde(rename = "STEP")]
    Step,
    #[serde(rename = "WAIT")]
    Wait,
    #[serde(rename = "CALLBACK")]
    Callback,
    #[serde(rename = "CHAINED_INVOKE")]
    ChainedInvoke,
    #[serde(rename = "CONTEXT")]
    Context,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub enum OperationStatus {
    #[serde(rename = "STARTED")]
    Started,
    #[serde(rename = "PENDING")]
    Pending,
    #[serde(rename = "READY")]
    Ready,
    #[serde(rename = "SUCCEEDED")]
    Succeeded,
    #[serde(rename = "FAILED")]
    Failed,
    #[serde(rename = "CANCELLED")]
    Cancelled,
    #[serde(rename = "TIMED_OUT")]
    TimedOut,
    #[serde(rename = "STOPPED")]
    Stopped,
}

pub struct OperationUpdate {
    pub id: String,
    pub action: OperationAction,
    pub operation_type: OperationType,
    pub payload: Option<String>,
    pub error: Option<ErrorObject>,
    pub parent_id: Option<String>,
    pub name: Option<String>,
    pub sub_type: Option<String>,
    // Type-specific options
    pub step_options: Option<StepOptions>,
    pub wait_options: Option<WaitOptions>,
    pub callback_options: Option<CallbackOptions>,
    pub chained_invoke_options: Option<ChainedInvokeOptions>,
    pub context_options: Option<ContextOptions>,
}

#[derive(Clone, Copy)]
pub enum OperationAction {
    Start,
    Succeed,
    Fail,
    Retry,
    Cancel,
}

#[derive(Clone, Serialize)]
pub struct StepOptions {
    #[serde(rename = "NextAttemptDelaySeconds", skip_serializing_if = "Option::is_none")]
    pub next_attempt_delay_seconds: Option<u64>,
}

#[derive(Clone, Serialize)]
pub struct WaitOptions {
    #[serde(rename = "WaitSeconds")]
    pub wait_seconds: u64,
}

#[derive(Clone, Serialize)]
pub struct CallbackOptions {
    #[serde(rename = "TimeoutSeconds", skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
    #[serde(rename = "HeartbeatTimeoutSeconds", skip_serializing_if = "Option::is_none")]
    pub heartbeat_timeout_seconds: Option<u64>,
}

#[derive(Clone, Serialize)]
pub struct ChainedInvokeOptions {
    #[serde(rename = "FunctionName")]
    pub function_name: String,
    #[serde(rename = "Payload", skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
    #[serde(rename = "TenantId", skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct ContextOptions {
    #[serde(rename = "ReplayChildren", skip_serializing_if = "Option::is_none")]
    pub replay_children: Option<bool>,
}
```

## Correctness Properties

*A property is a characteristic or behavior that should hold true across all valid executions of a system—essentially, a formal statement about what the system should do. Properties serve as the bridge between human-readable specifications and machine-verifiable correctness guarantees.*

### Property 1: Replay Round-Trip Consistency

*For any* sequence of operations that complete successfully and are checkpointed, when the function is replayed, each operation SHALL return its checkpointed result without re-executing the operation's closure.

**Validates: Requirements 3.2, 3.3, 3.4**

### Property 2: SerDes Round-Trip

*For any* value T that implements Serialize + DeserializeOwned, serializing then deserializing with JsonSerDes SHALL produce a value equal to the original.

**Validates: Requirements 11.2**

### Property 3: Operation ID Determinism

*For any* DurableContext with a given parent_id and step counter state, calling create_operation_id() SHALL produce the same ID when called in the same sequence position across multiple executions.

**Validates: Requirements 1.10**

### Property 4: Checkpoint Batching Efficiency

*For any* sequence of N checkpoint requests submitted within the batch time window, the Checkpointing_System SHALL send at most ceil(N * avg_size / max_batch_size) API calls.

**Validates: Requirements 2.3**

### Property 5: Concurrent ID Generation Uniqueness

*For any* number of concurrent tasks generating operation IDs from the same DurableContext, all generated IDs SHALL be unique.

**Validates: Requirements 17.3**

### Property 6: Map/Parallel Completion Criteria

*For any* map or parallel operation with CompletionConfig specifying min_successful=M, the operation SHALL complete successfully as soon as M tasks succeed, regardless of remaining task status.

**Validates: Requirements 8.6, 8.7, 9.3**

### Property 7: Step Semantics Checkpoint Ordering

*For any* step with AT_MOST_ONCE_PER_RETRY semantics, the checkpoint SHALL be created before the closure executes. *For any* step with AT_LEAST_ONCE_PER_RETRY semantics, the checkpoint SHALL be created after the closure executes.

**Validates: Requirements 4.1, 4.2**

### Property 8: Duration Validation

*For any* Duration value, constructing from seconds/minutes/hours/days/weeks/months/years SHALL produce the correct total seconds, and wait operations SHALL reject durations less than 1 second.

**Validates: Requirements 5.4, 12.7**

### Property 9: Orphaned Child Prevention

*For any* child operation whose parent has completed (marked done), attempting to checkpoint SHALL fail with OrphanedChildError.

**Validates: Requirements 2.8**

### Property 10: Non-Deterministic Execution Detection

*For any* replay where the operation type at a given operation_id differs from the checkpointed operation type, the system SHALL raise NonDeterministicExecutionError.

**Validates: Requirements 3.5**

### Property 11: Lambda Output Status Mapping

*For any* handler execution, the Lambda output status SHALL correctly reflect the execution outcome: SUCCEEDED for successful completion, FAILED for errors, and PENDING for suspended operations.

**Validates: Requirements 15.5, 15.6, 15.7**

### Property 12: READY Status Resume Without Re-checkpoint

*For any* operation in READY status during replay, the system SHALL resume execution without re-checkpointing the START action.

**Validates: Requirements 3.7**

### Property 13: Promise Combinator Correctness

*For any* set of futures passed to promise combinators:
- `all` SHALL return all results only when all futures succeed, or fail on first error
- `all_settled` SHALL return results for all futures regardless of success/failure
- `race` SHALL return the result of the first future to settle
- `any` SHALL return the result of the first future to succeed

**Validates: Requirements 20.1, 20.2, 20.3, 20.4**

## Error Handling

### Error Hierarchy

```rust
#[derive(Debug, thiserror::Error)]
pub enum DurableError {
    #[error("Execution error: {message}")]
    Execution { message: String, termination_reason: TerminationReason },

    #[error("Invocation error: {message}")]
    Invocation { message: String, termination_reason: TerminationReason },

    #[error("Checkpoint error: {message}")]
    Checkpoint { message: String, is_retriable: bool, aws_error: Option<AwsError> },

    #[error("Callback error: {message}")]
    Callback { message: String, callback_id: Option<String>, status: Option<OperationStatus> },

    #[error("Non-deterministic execution: {message}")]
    NonDeterministic { message: String, operation_id: Option<String> },

    #[error("Validation error: {message}")]
    Validation { message: String },

    #[error("Serialization error: {message}")]
    SerDes { message: String },

    #[error("Suspend execution")]
    Suspend { scheduled_timestamp: Option<f64> },

    #[error("Orphaned child: {message}")]
    OrphanedChild { message: String, operation_id: String },

    #[error("User code error: {message}")]
    UserCode { message: String, error_type: String, stack_trace: Option<String> },

    #[error("Size limit exceeded: {message}")]
    SizeLimit { message: String },

    #[error("Throttling: {message}")]
    Throttling { message: String },

    #[error("Resource not found: {message}")]
    ResourceNotFound { message: String },
}

#[derive(Debug, Clone, Copy)]
pub enum TerminationReason {
    UnhandledError,
    InvocationError,
    ExecutionError,
    CheckpointFailed,
    NonDeterministicExecution,
    StepInterrupted,
    CallbackError,
    SerializationError,
    SizeLimitExceeded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorObject {
    #[serde(rename = "ErrorType", skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
    #[serde(rename = "ErrorMessage", skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(rename = "StackTrace", skip_serializing_if = "Option::is_none")]
    pub stack_trace: Option<Vec<String>>,
    #[serde(rename = "ErrorData", skip_serializing_if = "Option::is_none")]
    pub error_data: Option<String>,
}
```

### Error Handling Strategy

1. **ExecutionError**: Return FAILED status, no Lambda retry
2. **InvocationError**: Propagate to Lambda for retry
3. **CheckpointError**: Check is_retriable - if true, propagate for retry; if false, return FAILED
4. **Suspend**: Return PENDING status, Lambda will re-invoke when ready
5. **UserCode errors**: Wrap in ErrorObject and return FAILED
6. **SizeLimit errors**: Return FAILED with clear message, do not propagate for retry
7. **InvalidParameterValueException for checkpoint tokens**: Allow propagation for retry
8. **Throttling**: Retry with exponential backoff

## Testing Strategy

### Unit Tests

Unit tests verify specific examples and edge cases:

- Configuration struct construction and defaults
- Error type creation and conversion
- Duration constructors and validation
- SerDesContext creation
- OperationIdentifier formatting
- ErrorObject serialization

### Property-Based Tests

Property-based tests verify universal properties using the `proptest` crate:

1. **Replay Round-Trip**: Generate random operation sequences, checkpoint them, verify replay returns same results
2. **SerDes Round-Trip**: Generate random JSON-serializable values, verify serialize/deserialize identity
3. **Operation ID Determinism**: Generate random parent_ids and counter states, verify deterministic ID generation
4. **Concurrent ID Uniqueness**: Spawn multiple tasks generating IDs, verify all unique
5. **Completion Criteria**: Generate random task outcomes and completion configs, verify correct completion behavior
6. **Duration Validation**: Generate random duration values, verify validation rules
7. **Step Semantics**: Verify checkpoint ordering for AT_MOST_ONCE vs AT_LEAST_ONCE
8. **Lambda Output**: Verify status mapping for various execution outcomes
9. **READY Status**: Verify no re-checkpoint for READY operations
10. **Promise Combinators**: Verify combinator behavior with various future outcomes

### Integration Tests

Integration tests verify component interactions:

- DurableContext with mock ExecutionState
- Checkpointing system with mock Lambda client
- Map/Parallel operations with mock child contexts
- Full handler execution with mock Lambda service
- EXECUTION operation input extraction

### Test Configuration

- Property tests: minimum 100 iterations per property
- Use `proptest` crate for property-based testing
- Use `tokio::test` for async tests
- Use `mockall` for mocking traits
