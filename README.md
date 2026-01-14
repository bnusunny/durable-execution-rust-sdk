# AWS Durable Execution SDK for Rust

Build reliable, long-running workflows on AWS Lambda with automatic checkpointing and replay.

## Overview

Build resilient workflows on AWS Lambda with automatic state persistence and failure recovery. Your functions can run for up to one year, pausing and resuming seamlessly. Every operation is checkpointed — if Lambda restarts, execution continues from the last completed step. Replayed operations return cached results instantly.

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
aws-durable-execution-sdk = "0.1"
tokio = { version = "1.0", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
```

## Quick Start

```rust
use aws_durable_execution_sdk::{durable_execution, DurableContext, DurableError, Duration};
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
    let is_valid: bool = ctx.step(|_| Ok(true), None).await?;

    // Step 2: Process payment (automatically checkpointed)
    let payment_id: String = ctx.step(|_| Ok("pay_123".to_string()), None).await?;

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
use aws_durable_execution_sdk::{StepConfig, StepSemantics};

let config = StepConfig {
    step_semantics: StepSemantics::AtMostOncePerRetry,
    ..Default::default()
};
let result = ctx.step(|_| Ok(42), Some(config)).await?;
```

## Parallel Processing

Process collections with configurable concurrency:

```rust
use aws_durable_execution_sdk::MapConfig;

let results = ctx.map(
    vec![1, 2, 3, 4, 5],
    |child_ctx, item, _index| async move {
        child_ctx.step(|_| Ok(item * 2), None).await
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
use aws_durable_execution_sdk::{MapConfig, ItemBatcher};

let results = ctx.map(
    large_item_list,
    |child_ctx, item, _index| async move {
        child_ctx.step(|_| Ok(process(item)), None).await
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
        |ctx| Box::pin(async move { ctx.step(|_| Ok("a"), None).await }),
        |ctx| Box::pin(async move { ctx.step(|_| Ok("b"), None).await }),
    ],
    None,
).await?;
```

## Promise Combinators

Coordinate multiple durable operations with promise-style combinators:

```rust
// Wait for ALL operations to complete successfully
let results = ctx.all(vec![
    ctx.step(|_| Ok(1), None),
    ctx.step(|_| Ok(2), None),
]).await?;

// Wait for ALL operations to settle (success or failure)
let batch_result = ctx.all_settled(vec![
    ctx.step(|_| Ok("success"), None),
    ctx.step(|_| Err("failure".into()), None),
]).await;

// Return the FIRST operation to settle
let first = ctx.race(vec![
    ctx.step(|_| Ok("fast"), None),
    ctx.step(|_| Ok("slow"), None),
]).await?;

// Return the FIRST operation to succeed
let first_success = ctx.any(vec![
    ctx.step(|_| Err("fail".into()), None),
    ctx.step(|_| Ok("success"), None),
]).await?;
```

## Callbacks

Wait for external systems to signal your workflow:

```rust
use aws_durable_execution_sdk::CallbackConfig;

// Option 1: wait_for_callback - combines callback creation with notification (recommended)
let approval: ApprovalResponse = ctx.wait_for_callback(
    |callback_id| async move {
        // Notify external system with the callback ID
        // This is executed within a durable step for replay safety
        notify_approver(&callback_id).await
    },
    Some(CallbackConfig {
        timeout: Duration::from_hours(24),
        ..Default::default()
    }),
).await?;

// Option 2: Manual callback creation for more control
let callback = ctx.create_callback::<ApprovalResponse>(Some(CallbackConfig {
    timeout: Duration::from_hours(24),
    ..Default::default()
})).await?;

notify_approver(&callback.callback_id).await?;
let approval = callback.result().await?;
```

## Replay-Safe Helpers

Generate deterministic values that are safe for replay:

```rust
use aws_durable_execution_sdk::replay_safe::{
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
use aws_durable_execution_sdk::Duration;

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
use aws_durable_execution_sdk::DurableError;

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

All log messages automatically include `durable_execution_arn`, `operation_id`, `parent_id`, and `is_replay` for correlation. See [TRACING.md](aws-durable-execution-sdk/docs/TRACING.md) for detailed tracing configuration and best practices.

## Determinism Requirements

Durable workflows must be deterministic — same input must produce the same sequence of operations. Common pitfalls:

| Non-Deterministic | Solution |
|-------------------|----------|
| `HashMap` iteration | Use `BTreeMap` or sort keys |
| Random numbers | Generate inside steps |
| Current time | Use `timestamp_from_execution` |
| UUIDs | Use `uuid_string_from_operation` |
| Environment variables | Capture in step at workflow start |

See [DETERMINISM.md](aws-durable-execution-sdk/docs/DETERMINISM.md) for detailed guidance.

## Execution Limits

| Limit | Value |
|-------|-------|
| Maximum execution duration | 1 year |
| Maximum wait duration | 1 year |
| Minimum wait duration | 1 second |
| Maximum response payload | 6 MB |
| Maximum checkpoint payload | 256 KB |
| Maximum history size | 25,000 operations |

See [LIMITS.md](aws-durable-execution-sdk/docs/LIMITS.md) for complete details.

## Examples

See the `aws-durable-execution-sdk/examples/` directory:

- `simple_workflow.rs` — Basic order processing with steps and waits
- `parallel_processing.rs` — Concurrent item processing with map
- `callback_workflow.rs` — External system integration with callbacks

Run examples locally:

```bash
cargo build --example simple_workflow
```

## Architecture

- **DurableContext** — Main interface for all durable operations
- **ExecutionState** — Manages checkpoints, replay logic, and operation state
- **CheckpointBatcher** — Batches checkpoint requests for efficiency
- **OperationIdGenerator** — Deterministic ID generation using blake2b

## Documentation

- [API Documentation](aws-durable-execution-sdk/src/lib.rs) — Comprehensive rustdoc with examples
- [Determinism Guide](aws-durable-execution-sdk/docs/DETERMINISM.md) — Writing replay-safe workflows
- [Limits Reference](aws-durable-execution-sdk/docs/LIMITS.md) — Execution constraints
- [Tracing Guide](aws-durable-execution-sdk/docs/TRACING.md) — Logging configuration and best practices

## License

Apache-2.0
