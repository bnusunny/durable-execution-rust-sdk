# Design Document: SDK Ergonomics Improvements

## Overview

This design document describes improvements to the AWS Durable Execution SDK for Rust to enhance developer ergonomics. The improvements focus on three areas:

1. **Enhanced wait_for_callback** - Improving the convenience method to properly checkpoint submitter execution
2. **ItemBatcher integration** - Full integration of item batching in map operations
3. **Logging improvements** - Adding tracing spans, simplified logging API, and extra field passthrough

These changes bring the Rust SDK closer to parity with the Python SDK while maintaining idiomatic Rust patterns.

## Architecture

The improvements integrate into the existing SDK architecture:

```
┌─────────────────────────────────────────────────────────────────┐
│                        DurableContext                           │
├─────────────────────────────────────────────────────────────────┤
│  wait_for_callback() ──────► Callback Handler + Step Handler    │
│  map() ────────────────────► Map Handler + ItemBatcher          │
│  log_info/debug/warn/error() ► TracingLogger with spans         │
└─────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────┐
│                      Tracing Integration                        │
├─────────────────────────────────────────────────────────────────┤
│  Operation Spans ──────────► tracing::span! with fields         │
│  LogInfo.extra ────────────► Dynamic field injection            │
└─────────────────────────────────────────────────────────────────┘
```

## Components and Interfaces

### 1. Enhanced wait_for_callback

The current `wait_for_callback` implementation calls the submitter outside of a durable step, which means the submitter execution is not checkpointed. This can lead to duplicate submissions during replay.

#### Current Implementation (Problem)

```rust
pub async fn wait_for_callback<T, F, Fut>(
    &self,
    submitter: F,
    config: Option<CallbackConfig>,
) -> DurableResult<T> {
    let callback = self.create_callback(config).await?;
    let callback_id = callback.callback_id.clone();
    
    // Problem: This step only stores the callback_id, doesn't execute submitter
    self.step_named("submit_callback", move |_| Ok(callback_id_for_step), None).await?;
    
    // Problem: Submitter runs outside step - not checkpointed!
    submitter(callback_id).await?;
    
    callback.result().await
}
```

#### New Implementation

```rust
pub async fn wait_for_callback<T, F, Fut>(
    &self,
    submitter: F,
    config: Option<CallbackConfig>,
) -> DurableResult<T>
where
    T: Serialize + DeserializeOwned + Send + Sync,
    F: FnOnce(String) -> Fut + Send,
    Fut: Future<Output = Result<(), Box<dyn Error + Send + Sync>>> + Send,
{
    // Create the callback first
    let callback: Callback<T> = self.create_callback(config).await?;
    let callback_id = callback.callback_id.clone();
    
    // Execute submitter within a step for checkpointing
    // The step returns () on success, indicating submission completed
    self.step_named(
        "submit_callback",
        move |_step_ctx| {
            // We need to run the async submitter synchronously within the step
            // This is handled by the step handler's async support
            Ok(())
        },
        None,
    ).await?;
    
    // Run the actual submitter - but only if we're not replaying
    // Check if the step was replayed
    let checkpoint_result = self.state.get_checkpoint_result(&self.last_operation_id()).await;
    if !checkpoint_result.is_succeeded() {
        submitter(callback_id.clone()).await.map_err(|e| DurableError::UserCode {
            message: e.to_string(),
            error_type: "SubmitterError".to_string(),
            stack_trace: None,
        })?;
    }
    
    // Wait for the callback result
    callback.result().await
}
```

However, this approach has a problem: the step closure cannot be async. A better approach is to use a child context:

#### Recommended Implementation

```rust
pub async fn wait_for_callback<T, F, Fut>(
    &self,
    submitter: F,
    config: Option<CallbackConfig>,
) -> DurableResult<T>
where
    T: Serialize + DeserializeOwned + Send + Sync,
    F: FnOnce(String) -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), Box<dyn Error + Send + Sync>>> + Send + 'static,
{
    // Create the callback first
    let callback: Callback<T> = self.create_callback(config).await?;
    let callback_id = callback.callback_id.clone();
    
    // Execute submitter within a child context for proper checkpointing
    // The child context ensures the submitter execution is tracked
    self.run_in_child_context(
        move |child_ctx| {
            let callback_id = callback_id.clone();
            async move {
                // Use a step to checkpoint that we're about to submit
                child_ctx.step_named(
                    "execute_submitter",
                    move |_| Ok(()),
                    None,
                ).await?;
                
                // The submitter runs after the step checkpoint
                // If we replay, the step succeeds immediately and we skip the submitter
                Ok::<(), DurableError>(())
            }
        },
        None,
    ).await?;
    
    // Actually run the submitter if not replaying
    // This is determined by checking if the child context was replayed
    let submit_op_id = format!("{}-submit", self.last_operation_id());
    let checkpoint_result = self.state.get_checkpoint_result(&submit_op_id).await;
    
    if !checkpoint_result.is_existent() || !checkpoint_result.is_succeeded() {
        submitter(callback.callback_id.clone()).await.map_err(|e| DurableError::UserCode {
            message: e.to_string(),
            error_type: "SubmitterError".to_string(),
            stack_trace: None,
        })?;
    }
    
    // Wait for the callback result
    callback.result().await
}
```

### 2. ItemBatcher Integration

The `ItemBatcher` struct exists but needs fuller integration with the map handler.

#### ItemBatcher Configuration

```rust
/// Configuration for batching items in map operations.
#[derive(Debug, Clone)]
pub struct ItemBatcher {
    /// Maximum number of items per batch.
    pub max_items_per_batch: usize,
    /// Maximum total bytes per batch (estimated via serialization).
    pub max_bytes_per_batch: usize,
}

impl Default for ItemBatcher {
    fn default() -> Self {
        Self {
            max_items_per_batch: 100,
            max_bytes_per_batch: 256 * 1024, // 256KB
        }
    }
}

impl ItemBatcher {
    /// Creates a new ItemBatcher with the specified limits.
    pub fn new(max_items_per_batch: usize, max_bytes_per_batch: usize) -> Self {
        Self { max_items_per_batch, max_bytes_per_batch }
    }
    
    /// Batches items according to configuration.
    /// Returns a vector of (start_index, batch) tuples.
    pub fn batch<T: Serialize>(&self, items: &[T]) -> Vec<(usize, Vec<T>)>
    where
        T: Clone,
    {
        let mut batches = Vec::new();
        let mut current_batch = Vec::new();
        let mut current_bytes = 0usize;
        let mut batch_start_index = 0;
        
        for (i, item) in items.iter().enumerate() {
            let item_bytes = serde_json::to_string(item)
                .map(|s| s.len())
                .unwrap_or(0);
            
            // Check if adding this item would exceed limits
            let would_exceed_items = current_batch.len() >= self.max_items_per_batch;
            let would_exceed_bytes = current_bytes + item_bytes > self.max_bytes_per_batch 
                && !current_batch.is_empty();
            
            if would_exceed_items || would_exceed_bytes {
                batches.push((batch_start_index, std::mem::take(&mut current_batch)));
                current_bytes = 0;
                batch_start_index = i;
            }
            
            current_batch.push(item.clone());
            current_bytes += item_bytes;
        }
        
        if !current_batch.is_empty() {
            batches.push((batch_start_index, current_batch));
        }
        
        batches
    }
}
```

#### Map Handler Integration

The map handler already has basic batching support. The enhancement adds byte-based batching:

```rust
/// Batches items according to the ItemBatcher configuration.
fn batch_items<T: Serialize + Clone>(items: &[T], batcher: &ItemBatcher) -> Vec<(usize, Vec<T>)> {
    batcher.batch(items)
}
```

### 3. Tracing Spans for Operations

Add tracing spans to each operation handler for better observability.

#### Span Creation Helper

```rust
/// Creates a tracing span for a durable operation.
fn create_operation_span(
    operation_type: &str,
    op_id: &OperationIdentifier,
    durable_execution_arn: &str,
) -> tracing::Span {
    tracing::info_span!(
        "durable_operation",
        operation_type = %operation_type,
        operation_id = %op_id.operation_id,
        parent_id = ?op_id.parent_id,
        name = ?op_id.name,
        durable_execution_arn = %durable_execution_arn,
    )
}
```

#### Integration in Step Handler

```rust
pub async fn step_handler<T, F>(
    func: F,
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    config: &StepConfig,
    logger: &Arc<dyn Logger>,
) -> Result<T, DurableError>
where
    T: Serialize + DeserializeOwned,
    F: StepFn<T>,
{
    let span = create_operation_span("step", op_id, state.durable_execution_arn());
    let _guard = span.enter();
    
    // ... existing implementation ...
    
    // On completion, record status
    span.record("status", &tracing::field::display(status));
}
```

### 4. Simplified Logging API

Add convenience methods to DurableContext for logging with automatic context.

```rust
impl DurableContext {
    /// Logs a message at INFO level with automatic context.
    pub fn log_info(&self, message: &str) {
        self.log_with_level(LogLevel::Info, message, &[]);
    }
    
    /// Logs a message at INFO level with extra fields.
    pub fn log_info_with<'a>(&self, message: &str, fields: &[(&'a str, &'a str)]) {
        self.log_with_level(LogLevel::Info, message, fields);
    }
    
    /// Logs a message at DEBUG level with automatic context.
    pub fn log_debug(&self, message: &str) {
        self.log_with_level(LogLevel::Debug, message, &[]);
    }
    
    /// Logs a message at DEBUG level with extra fields.
    pub fn log_debug_with<'a>(&self, message: &str, fields: &[(&'a str, &'a str)]) {
        self.log_with_level(LogLevel::Debug, message, fields);
    }
    
    /// Logs a message at WARN level with automatic context.
    pub fn log_warn(&self, message: &str) {
        self.log_with_level(LogLevel::Warn, message, &[]);
    }
    
    /// Logs a message at WARN level with extra fields.
    pub fn log_warn_with<'a>(&self, message: &str, fields: &[(&'a str, &'a str)]) {
        self.log_with_level(LogLevel::Warn, message, fields);
    }
    
    /// Logs a message at ERROR level with automatic context.
    pub fn log_error(&self, message: &str) {
        self.log_with_level(LogLevel::Error, message, &[]);
    }
    
    /// Logs a message at ERROR level with extra fields.
    pub fn log_error_with<'a>(&self, message: &str, fields: &[(&'a str, &'a str)]) {
        self.log_with_level(LogLevel::Error, message, fields);
    }
    
    fn log_with_level(&self, level: LogLevel, message: &str, extra: &[(&str, &str)]) {
        let mut log_info = LogInfo::new(self.durable_execution_arn());
        
        if let Some(ref parent_id) = self.parent_id {
            log_info = log_info.with_parent_id(parent_id);
        }
        
        for (key, value) in extra {
            log_info = log_info.with_extra(*key, *value);
        }
        
        match level {
            LogLevel::Debug => self.logger.debug(message, &log_info),
            LogLevel::Info => self.logger.info(message, &log_info),
            LogLevel::Warn => self.logger.warn(message, &log_info),
            LogLevel::Error => self.logger.error(message, &log_info),
        }
    }
}

enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}
```

### 5. Extra Fields in Tracing Output

Update TracingLogger to include extra fields from LogInfo.

```rust
impl Logger for TracingLogger {
    fn info(&self, message: &str, info: &LogInfo) {
        // Build dynamic fields from extra
        let extra_fields: Vec<_> = info.extra.iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        
        tracing::info!(
            durable_execution_arn = ?info.durable_execution_arn,
            operation_id = ?info.operation_id,
            parent_id = ?info.parent_id,
            is_replay = info.is_replay,
            extra = ?extra_fields,
            "{}",
            message
        );
    }
    
    // Similar for debug, warn, error...
}
```

A more sophisticated approach uses `tracing`'s `valuable` feature or dynamic fields:

```rust
impl Logger for TracingLogger {
    fn info(&self, message: &str, info: &LogInfo) {
        // Use tracing's span to add dynamic fields
        let span = tracing::info_span!(
            "log",
            durable_execution_arn = ?info.durable_execution_arn,
            operation_id = ?info.operation_id,
            parent_id = ?info.parent_id,
            is_replay = info.is_replay,
        );
        
        let _guard = span.enter();
        
        // Log extra fields as a separate event or include in message
        if info.extra.is_empty() {
            tracing::info!("{}", message);
        } else {
            let extra_str: String = info.extra.iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(", ");
            tracing::info!("{} [{}]", message, extra_str);
        }
    }
}
```

## Data Models

### LogInfo Enhancement

The existing `LogInfo` struct already supports extra fields. No changes needed to the data model.

```rust
#[derive(Debug, Clone, Default)]
pub struct LogInfo {
    pub durable_execution_arn: Option<String>,
    pub operation_id: Option<String>,
    pub parent_id: Option<String>,
    pub is_replay: bool,
    pub extra: Vec<(String, String)>,  // Already exists
}
```

## Correctness Properties

*A property is a characteristic or behavior that should hold true across all valid executions of a system—essentially, a formal statement about what the system should do. Properties serve as the bridge between human-readable specifications and machine-verifiable correctness guarantees.*

### Property 1: wait_for_callback Checkpoints Submitter Execution

*For any* valid callback configuration and submitter function, when wait_for_callback is called, the SDK SHALL create a checkpoint that tracks whether the submitter has been executed, ensuring the submitter is not re-executed during replay.

**Validates: Requirements 1.1, 1.2**

### Property 2: wait_for_callback Error Propagation

*For any* submitter function that returns an error, wait_for_callback SHALL propagate that error with the error message preserved and error type set to "SubmitterError".

**Validates: Requirements 1.3**

### Property 3: wait_for_callback Configuration Passthrough

*For any* CallbackConfig with timeout and heartbeat_timeout values, wait_for_callback SHALL pass these values to the underlying callback operation.

**Validates: Requirements 1.4**

### Property 4: wait_for_callback Result Return

*For any* callback that receives a success signal with a result value, wait_for_callback SHALL return that result value deserialized to the expected type.

**Validates: Requirements 1.5**

### Property 5: ItemBatcher Configuration Respected

*For any* ItemBatcher configuration with max_items_per_batch and max_bytes_per_batch, the batch method SHALL produce batches where each batch has at most max_items_per_batch items AND at most max_bytes_per_batch bytes (estimated).

**Validates: Requirements 2.1, 2.2**

### Property 6: ItemBatcher Ordering Preservation

*For any* list of items, after batching with ItemBatcher, concatenating all batches in order SHALL produce a list equal to the original input list.

**Validates: Requirements 2.3, 2.4, 2.6, 2.7**

### Property 7: Tracing Span Fields

*For any* durable operation (step, wait, callback, invoke, map, parallel), the created tracing span SHALL include operation_id, operation_type, and durable_execution_arn as structured fields, and SHALL include parent_id when the operation has a parent.

**Validates: Requirements 3.2, 3.3, 3.4, 3.5**

### Property 8: Tracing Span Lifecycle

*For any* durable operation, a tracing span SHALL be created when the operation starts and SHALL be closed when the operation completes (success or failure).

**Validates: Requirements 3.1, 3.6**

### Property 9: Logging Methods Automatic Context

*For any* call to log_info, log_debug, log_warn, or log_error on DurableContext, the resulting log message SHALL include durable_execution_arn and parent_id (when available) without the caller needing to specify them.

**Validates: Requirements 4.5**

### Property 10: Logging Methods Extra Fields

*For any* call to log_info_with, log_debug_with, log_warn_with, or log_error_with with extra fields, those fields SHALL appear in the resulting log output.

**Validates: Requirements 4.6**

### Property 11: TracingLogger Extra Field Passthrough

*For any* LogInfo with extra fields, when passed to TracingLogger, all extra fields SHALL appear in the tracing output as key-value pairs.

**Validates: Requirements 5.1, 5.2**

## Error Handling

### wait_for_callback Errors

| Error Condition | Error Type | Behavior |
|----------------|------------|----------|
| Submitter returns error | DurableError::UserCode | Propagate with "SubmitterError" type |
| Callback times out | DurableError::Callback | Return timeout error |
| Callback receives failure | DurableError::Callback | Return callback error |
| Serialization fails | DurableError::SerDes | Return serialization error |

### ItemBatcher Errors

ItemBatcher operations are infallible - they always produce valid batches. Empty input produces empty output.

### Logging Errors

Logging operations should never fail or throw. If tracing fails, the error is silently ignored to avoid disrupting workflow execution.

## Testing Strategy

### Unit Tests

1. **wait_for_callback tests**
   - Test successful callback flow
   - Test submitter error propagation
   - Test replay behavior (submitter not re-executed)
   - Test configuration passthrough

2. **ItemBatcher tests**
   - Test default configuration values
   - Test item count batching
   - Test byte size batching
   - Test empty input
   - Test single item
   - Test ordering preservation

3. **Logging tests**
   - Test each log level method
   - Test extra field inclusion
   - Test automatic context inclusion

### Property-Based Tests

Property-based tests will use the `proptest` crate with minimum 100 iterations per test.

1. **ItemBatcher batching property** - Generate random item lists and verify batching constraints
2. **ItemBatcher ordering property** - Generate random items and verify order preservation
3. **Logging context property** - Generate random contexts and verify field inclusion

### Integration Tests

1. **wait_for_callback integration** - Test with mock callback service
2. **Map with batching integration** - Test map operation with ItemBatcher configured
3. **Tracing span integration** - Test span creation and closure with tracing subscriber

