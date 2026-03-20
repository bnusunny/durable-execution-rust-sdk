# AWS Durable Execution SDK for Rust

[![CI](https://github.com/aws/durable-execution-sdk-rust/actions/workflows/pr.yml/badge.svg)](https://github.com/aws/durable-execution-sdk-rust/actions/workflows/pr.yml)
[![Crates.io](https://img.shields.io/crates/v/durable-execution-sdk.svg)](https://crates.io/crates/durable-execution-sdk)
[![Documentation](https://docs.rs/durable-execution-sdk/badge.svg)](https://docs.rs/durable-execution-sdk)
[![License](https://img.shields.io/crates/l/durable-execution-sdk.svg)](LICENSE)

Build reliable, long-running workflows on AWS Lambda with automatic checkpointing and replay.

## Overview

Build resilient workflows on AWS Lambda with automatic state persistence and failure recovery. Your functions can run for up to one year, pausing and resuming seamlessly. Every operation is checkpointed — if Lambda restarts, execution continues from the last completed step. Replayed operations return cached results instantly.

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
durable-execution-sdk = "0.1"
tokio = { version = "1.0", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
```

For testing support, add the testing crate:

```toml
[dev-dependencies]
durable-execution-sdk-testing = "0.1"
```

## Quick Start

```rust
use durable_execution_sdk::{durable_execution, DurableContext, DurableError, Duration};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct OrderEvent {
    order_id: String,
    amount: f64,
}

#[derive(Serialize)]
struct OrderResult {
    status: String,
    order_id: String,
}

#[durable_execution]
async fn process_order(event: OrderEvent, ctx: DurableContext) -> Result<OrderResult, DurableError> {
    // Step 1: Validate (automatically checkpointed)
    let is_valid: bool = ctx.step(|_| async move { Ok(true) }, None).await?;

    // Step 2: Process payment (automatically checkpointed)
    let payment_id: String = ctx.step(|_| async move { Ok("pay_123".to_string()) }, None).await?;

    // Step 3: Wait for confirmation (suspends Lambda, resumes later)
    ctx.wait(Duration::from_seconds(5), Some("payment_confirmation")).await?;

    Ok(OrderResult {
        status: "completed".to_string(),
        order_id: event.order_id,
    })
}
```

## Core Operations

| Operation | Description |
|-----------|-------------|
| `step()` / `step_named()` | Execute and checkpoint a unit of work |
| `wait()` | Pause execution for a duration (suspends Lambda) |
| `wait_for_condition()` | Poll a condition with configurable retry until it passes |
| `create_callback()` | Wait for external systems to signal completion |
| `wait_for_callback()` | Create callback and notify external system in one replay-safe call |
| `invoke()` | Call other durable Lambda functions |
| `map()` | Process collections in parallel with concurrency limits |
| `parallel()` | Execute multiple independent operations concurrently |
| `run_in_child_context()` | Create isolated nested workflows |

## Step Semantics

Two execution modes for different use cases:

- **AtLeastOncePerRetry** (default) — Checkpoint after execution. May re-execute on interruption, but result is always persisted.
- **AtMostOncePerRetry** — Checkpoint before execution. Guarantees at-most-once execution for non-idempotent operations.

```rust
use durable_execution_sdk::{StepConfig, StepSemantics};

let config = StepConfig {
    step_semantics: StepSemantics::AtMostOncePerRetry,
    ..Default::default()
};
let result = ctx.step(|_| async move { Ok(42) }, Some(config)).await?;
```

## Parallel Processing

Process collections with configurable concurrency:

```rust
use durable_execution_sdk::MapConfig;

let results = ctx.map(
    vec![1, 2, 3, 4, 5],
    |child_ctx, item, _index| async move {
        child_ctx.step(|_| async move { Ok(item * 2) }, None).await
    },
    Some(MapConfig {
        max_concurrency: Some(3),
        ..Default::default()
    }),
).await?;
```

### Item Batching

Reduce checkpoint overhead for large collections using `ItemBatcher`:

```rust
use durable_execution_sdk::{MapConfig, ItemBatcher};

let results = ctx.map(
    large_item_list,
    |child_ctx, item, _index| async move {
        child_ctx.step(|_| async move { Ok(process(item)) }, None).await
    },
    Some(MapConfig {
        item_batcher: Some(ItemBatcher::new(100, 256 * 1024)), // 100 items or 256KB per batch
        ..Default::default()
    }),
).await?;
```

Execute independent branches concurrently:

```rust
let results = ctx.parallel(
    vec![
        |ctx| Box::pin(async move { ctx.step(|_| async move { Ok("a") }, None).await }),
        |ctx| Box::pin(async move { ctx.step(|_| async move { Ok("b") }, None).await }),
    ],
    None,
).await?;
```

## Promise Combinators

Coordinate multiple durable operations with promise-style combinators:

```rust
// Wait for ALL operations to complete successfully
let results = ctx.all(vec![
    ctx.step(|_| async move { Ok(1) }, None),
    ctx.step(|_| async move { Ok(2) }, None),
]).await?;

// Wait for ALL operations to settle (success or failure)
let batch_result = ctx.all_settled(vec![
    ctx.step(|_| async move { Ok("success") }, None),
    ctx.step(|_| async move { Err("failure".into()) }, None),
]).await;

// Return the FIRST operation to settle
let first = ctx.race(vec![
    ctx.step(|_| async move { Ok("fast") }, None),
    ctx.step(|_| async move { Ok("slow") }, None),
]).await?;

// Return the FIRST operation to succeed
let first_success = ctx.any(vec![
    ctx.step(|_| async move { Err("fail".into()) }, None),
    ctx.step(|_| async move { Ok("success") }, None),
]).await?;
```

## Callbacks

Wait for external systems to signal your workflow:

```rust
use durable_execution_sdk::CallbackConfig;

// Option 1: wait_for_callback - combines callback creation with notification (recommended)
// The submitter function runs inside a checkpointed child context, ensuring
// the notification won't be re-sent during replay.
let approval: ApprovalResponse = ctx.wait_for_callback(
    |callback_id| async move {
        // This notification is checkpointed and won't re-execute on replay
        notify_approver(&callback_id).await?;
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    },
    Some(CallbackConfig {
        timeout: Duration::from_hours(24),
        ..Default::default()
    }),
).await?;

// Option 2: Manual callback creation for more control
// Note: Be careful - notifications outside of steps are NOT replay-safe!
let callback = ctx.create_callback::<ApprovalResponse>(Some(CallbackConfig {
    timeout: Duration::from_hours(24),
    ..Default::default()
})).await?;

// Wrap notification in a step for replay safety
ctx.step_named("notify", |_| async move {
    // Notification logic here
    Ok(())
}, None).await?;

let approval = callback.result().await?;
```

### Why use wait_for_callback?

The traditional callback pattern has a subtle replay issue:

```rust
// PROBLEMATIC: If Lambda restarts after notify_approver() but before the next
// checkpoint, the notification will be sent AGAIN during replay!
let callback = ctx.create_callback::<Response>(None).await?;
notify_approver(&callback.callback_id).await?;  // NOT checkpointed!
let result = callback.result().await?;
```

`wait_for_callback` solves this by running your notification inside a checkpointed child context.

## Retry Strategies

Configure automatic retries for steps with built-in strategies:

```rust
use durable_execution_sdk::{StepConfig, ExponentialBackoff, FixedDelay, LinearBackoff, NoRetry};

// Exponential backoff: 1s, 2s, 4s, 8s, ...
let config = StepConfig {
    retry_strategy: Some(Box::new(ExponentialBackoff::new(3, 1000, 30000))),
    ..Default::default()
};

// Fixed delay: 2s, 2s, 2s, ...
let config = StepConfig {
    retry_strategy: Some(Box::new(FixedDelay::new(5, 2000))),
    ..Default::default()
};

// Linear backoff: 1s, 2s, 3s, 4s, ...
let config = StepConfig {
    retry_strategy: Some(Box::new(LinearBackoff::new(3, 1000))),
    ..Default::default()
};
```

Filter which errors are retryable:

```rust
use durable_execution_sdk::RetryableErrorFilter;

let filter = RetryableErrorFilter::new(vec!["TimeoutError".to_string()]);
assert!(filter.is_retryable_with_type("request timed out", "TimeoutError"));
```

## Replay-Safe Helpers

Generate deterministic values that are safe for replay:

```rust
use durable_execution_sdk::replay_safe::{
    uuid_string_from_operation,
    timestamp_from_execution,
};

// Deterministic UUID from operation ID (same inputs = same UUID)
let operation_id = ctx.next_operation_id();
let uuid = uuid_string_from_operation(&operation_id, 0);

// Replay-safe timestamp (execution start time, not current time)
if let Some(timestamp_ms) = timestamp_from_execution(ctx.state()) {
    println!("Execution started at: {} ms", timestamp_ms);
}
```

## Extended Duration Support

```rust
use durable_execution_sdk::Duration;

let seconds = Duration::from_seconds(30);
let minutes = Duration::from_minutes(5);
let hours = Duration::from_hours(2);
let days = Duration::from_days(7);
let weeks = Duration::from_weeks(2);      // 14 days
let months = Duration::from_months(3);    // 90 days
let years = Duration::from_years(1);      // 365 days
```

## Error Handling

The SDK provides typed errors for different failure scenarios:

```rust
use durable_execution_sdk::DurableError;

// Execution errors (workflow fails, no Lambda retry)
DurableError::execution("Invalid order");

// Invocation errors (triggers Lambda retry)
DurableError::invocation("Temporary failure");

// Check if error is retriable
if error.is_retriable() {
    // Handle retriable error
}
```

## Logging

The SDK provides a simplified logging API with automatic context:

```rust
// Basic logging - context is automatically included
ctx.log_info("Processing order started");
ctx.log_debug("Validating input parameters");
ctx.log_warn("Retry attempt 2 of 5");
ctx.log_error("Payment processing failed");

// Logging with extra fields for filtering
ctx.log_info_with("Processing order", &[
    ("order_id", "ORD-12345"),
    ("amount", "99.99"),
]);
```

All log messages automatically include `durable_execution_arn`, `operation_id`, `parent_id`, and `is_replay` for correlation. See [TRACING.md](durable-execution-sdk/docs/TRACING.md) for detailed tracing configuration and best practices.

## Determinism Requirements

Durable workflows must be deterministic — same input must produce the same sequence of operations. Common pitfalls:

| Non-Deterministic | Solution |
|-------------------|----------|
| `HashMap` iteration | Use `BTreeMap` or sort keys |
| Random numbers | Generate inside steps |
| Current time | Use `timestamp_from_execution` |
| UUIDs | Use `uuid_string_from_operation` |
| Environment variables | Capture in step at workflow start |

See [DETERMINISM.md](durable-execution-sdk/docs/DETERMINISM.md) for detailed guidance.

## Execution Limits

| Limit | Value |
|-------|-------|
| Maximum execution duration | 1 year |
| Maximum wait duration | 1 year |
| Minimum wait duration | 1 second |
| Maximum response payload | 6 MB |
| Maximum checkpoint payload | 256 KB |
| Maximum history size | 25,000 operations |

See [LIMITS.md](durable-execution-sdk/docs/LIMITS.md) for complete details.

## Testing

The `durable-execution-sdk-testing` crate provides two test runners for verifying durable workflows at different levels.

### Local Testing

`LocalDurableTestRunner` executes workflows in-process with a simulated checkpoint server. It supports time skipping, operation inspection, and callback interaction — no AWS credentials needed.

```rust
use durable_execution_sdk_testing::{
    LocalDurableTestRunner, TestEnvironmentConfig, ExecutionStatus,
};

#[tokio::test]
async fn test_order_workflow() {
    LocalDurableTestRunner::setup_test_environment(TestEnvironmentConfig {
        skip_time: true,
        checkpoint_delay: None,
    }).await.unwrap();

    let mut runner = LocalDurableTestRunner::new(process_order);
    let result = runner.run("input").await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    // Inspect individual operations
    if let Some(op) = runner.get_operation("validate").await {
        assert_eq!(op.get_status(), OperationStatus::Succeeded);
    }

    LocalDurableTestRunner::teardown_test_environment().await.unwrap();
}
```

### Cloud Testing

`CloudDurableTestRunner` invokes deployed Lambda functions and polls `GetDurableExecutionHistory` for real-time operation tracking. It supports operation handles for waiting on specific operations, callback interaction during execution, and configurable polling with timeout.

```rust
use durable_execution_sdk_testing::{
    CloudDurableTestRunner, CloudTestRunnerConfig, ExecutionStatus,
};
use std::time::Duration;

#[tokio::test]
async fn test_deployed_workflow() {
    let mut runner = CloudDurableTestRunner::<String>::new("my-function-name")
        .await
        .unwrap()
        .with_config(CloudTestRunnerConfig::new()
            .with_poll_interval(Duration::from_millis(500))
            .with_timeout(Duration::from_secs(60)));

    // Get a handle to wait for a specific operation during execution
    let callback_handle = runner.get_operation_handle("my-callback");

    let result = runner.run("input").await.unwrap();
    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    // All operations are available after run completes
    let ops = runner.get_all_operations();
    println!("Total operations: {}", ops.len());
}
```

### Callback Testing

Both runners support callback interaction through operation handles:

```rust
use durable_execution_sdk_testing::WaitingOperationStatus;

// Get a handle before calling run()
let handle = runner.get_operation_handle("approval-callback");

// In a separate task, wait for the callback to be ready, then respond
let handle_clone = handle.clone();
tokio::spawn(async move {
    handle_clone.wait_for_data(WaitingOperationStatus::Submitted).await.unwrap();
    handle_clone.send_callback_success(r#""approved""#).await.unwrap();
});

let result = runner.run(payload).await.unwrap();
```

### Operation Inspection

Both runners provide consistent APIs for inspecting operations after execution:

```rust
// Look up operations by name, index, or ID
let op = runner.get_operation("process_payment");
let first_op = runner.get_operation_by_index(0);
let specific_op = runner.get_operation_by_id("op-abc-123");

// Get typed operation details
let step: StepDetails<String> = op.get_step_details()?;
let callback: CallbackDetails<Response> = op.get_callback_details()?;

// Enumerate child operations
let children = op.get_child_operations();
```

## Examples

See the `durable-execution-sdk/examples/` directory for runnable examples organized by feature:

| Category | Examples |
|----------|----------|
| `hello_world/` | Minimal workflow with a single step |
| `step/` | Step semantics, retries, error handling, named steps |
| `wait/` | Wait durations, named waits, multiple waits |
| `wait_for_condition/` | Polling conditions with configurable retry |
| `callback/` | Simple callbacks, heartbeats, failures, custom serialization |
| `invoke/` | Calling other durable functions |
| `map/` | Parallel collection processing, concurrency, empty arrays |
| `parallel/` | Concurrent branches, failure tolerance, min-successful |
| `promise_combinators/` | all, any, race, all_settled with waits and timeouts |
| `child_context/` | Nested workflows, large data, checkpoint limits |
| `concurrency/` | Concurrent operations and wait patterns |
| `serde/` | Custom serialization |
| `error_handling/` | Error types and recovery patterns |
| `comprehensive/` | End-to-end order processing workflow |

Each example has a corresponding history replay test in `examples/tests/`.

## Architecture

- **DurableContext** — Main interface for all durable operations
- **ExecutionState** — Manages checkpoints, replay logic, and operation state
- **CheckpointBatcher** — Batches checkpoint requests for efficiency
- **OperationIdGenerator** — Deterministic ID generation using blake2b

### Crate Structure

| Crate | Description |
|-------|-------------|
| `durable-execution-sdk` | Core SDK with context, handlers, and client |
| `durable-execution-sdk-macros` | `#[durable_execution]` proc macro |
| `durable-execution-sdk-testing` | Local and cloud test runners, operation inspection, mocks |

## Documentation

- [API Documentation](durable-execution-sdk/sdk/src/lib.rs) — Comprehensive rustdoc with examples
- [Determinism Guide](durable-execution-sdk/docs/DETERMINISM.md) — Writing replay-safe workflows
- [Limits Reference](durable-execution-sdk/docs/LIMITS.md) — Execution constraints
- [Tracing Guide](durable-execution-sdk/docs/TRACING.md) — Logging configuration and best practices
- [Language SDK Specification](durable-execution-sdk/docs/language_sdk_specification.md) — Cross-language SDK design spec

## License

Apache-2.0
