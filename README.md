# AWS Durable Execution SDK for Rust

Build reliable, long-running workflows on AWS Lambda with automatic checkpointing and replay.

## Overview

This SDK solves Lambda's fundamental limitations — execution time limits and ephemeral state — by automatically checkpointing every operation. If your function times out or restarts, it resumes exactly where it left off. Completed operations return cached results instantly without re-executing.

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

## Callbacks

Wait for external systems to signal your workflow:

```rust
use aws_durable_execution_sdk::CallbackConfig;

let callback = ctx.create_callback::<ApprovalResponse>(Some(CallbackConfig {
    timeout: Duration::from_hours(24),
    ..Default::default()
})).await?;

// Share callback.callback_id with external system
notify_approver(&callback.callback_id).await?;

// Suspends until callback is received
let approval = callback.result().await?;
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

## Examples

See the `examples/` directory:

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
- **OperationIdGenerator** — Deterministic ID generation using blake2b (ensures same operation gets same ID across replays)

## License

Apache-2.0
