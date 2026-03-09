# AWS Durable Execution SDK for Rust

Build reliable, long-running workflows on AWS Lambda with automatic checkpointing and replay.

## Overview

The AWS Durable Execution SDK enables Rust developers to build workflows that survive Lambda restarts, timeouts, and failures. Every operation is automatically checkpointed — if Lambda restarts, execution resumes from the last completed step. Replayed operations return cached results instantly. Workflows can run for up to one year, pausing and resuming seamlessly.

## Installation

```toml
[dependencies]
durable-execution-sdk = "0.1"
tokio = { version = "1.0", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
```

## Quick start

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

## Core operations

| Operation | Description |
|-----------|-------------|
| `step()` / `step_named()` | Execute and checkpoint a unit of work |
| `wait()` | Pause execution for a duration (suspends Lambda) |
| `wait_for_condition()` | Poll a condition with configurable retry |
| `create_callback()` / `wait_for_callback()` | Wait for external systems to signal completion |
| `invoke()` | Call other durable Lambda functions |
| `map()` | Process collections in parallel with concurrency limits |
| `parallel()` | Execute multiple independent operations concurrently |
| `run_in_child_context()` | Create isolated nested workflows |
| `all()` / `any()` / `race()` / `all_settled()` | Promise-style combinators |

## Step semantics

Two execution modes for different use cases:

- `AtLeastOncePerRetry` (default) — Checkpoint after execution. May re-execute on interruption, but result is always persisted.
- `AtMostOncePerRetry` — Checkpoint before execution. Guarantees at-most-once execution for non-idempotent operations.

```rust
use durable_execution_sdk::{StepConfig, StepSemantics};

let config = StepConfig {
    step_semantics: StepSemantics::AtMostOncePerRetry,
    ..Default::default()
};
let result = ctx.step(|_| Ok(42), Some(config)).await?;
```

## Parallel processing

```rust
use durable_execution_sdk::MapConfig;

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

## Callbacks

```rust
use durable_execution_sdk::CallbackConfig;

// Recommended: wait_for_callback combines creation with replay-safe notification
let approval: ApprovalResponse = ctx.wait_for_callback(
    |callback_id| async move {
        notify_approver(&callback_id).await?;
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    },
    Some(CallbackConfig {
        timeout: Duration::from_hours(24),
        ..Default::default()
    }),
).await?;
```

## Retry strategies

```rust
use durable_execution_sdk::{StepConfig, ExponentialBackoff, FixedDelay, LinearBackoff};

let config = StepConfig {
    retry_strategy: Some(Box::new(ExponentialBackoff::new(3, 1000, 30000))),
    ..Default::default()
};
```

## Error handling

```rust
use durable_execution_sdk::DurableError;

// Execution errors (workflow fails, no Lambda retry)
DurableError::execution("Invalid order");

// Invocation errors (triggers Lambda retry)
DurableError::invocation("Temporary failure");
```

## Logging

```rust
ctx.log_info("Processing order started");
ctx.log_info_with("Processing order", &[
    ("order_id", "ORD-12345"),
    ("amount", "99.99"),
]);
```

All log messages automatically include `durable_execution_arn`, `operation_id`, `parent_id`, and `is_replay` for correlation.

## Replay-safe helpers

```rust
use durable_execution_sdk::replay_safe::{uuid_string_from_operation, timestamp_from_execution};

// Deterministic UUID (same inputs = same UUID across replays)
let uuid = uuid_string_from_operation(&operation_id, 0);

// Replay-safe timestamp (execution start time, not wall clock)
let ts = timestamp_from_execution(ctx.state());
```

## Duration support

```rust
use durable_execution_sdk::Duration;

let d = Duration::from_seconds(30);
let d = Duration::from_minutes(5);
let d = Duration::from_hours(2);
let d = Duration::from_days(7);
let d = Duration::from_weeks(2);   // 14 days
let d = Duration::from_months(3);  // 90 days
let d = Duration::from_years(1);   // 365 days
```

## Execution limits

| Limit | Value |
|-------|-------|
| Maximum execution duration | 1 year |
| Maximum wait duration | 1 year |
| Minimum wait duration | 1 second |
| Maximum response payload | 6 MB |
| Maximum checkpoint payload | 256 KB |
| Maximum history size | 25,000 operations |

## Crate structure

| Crate | Description |
|-------|-------------|
| `durable-execution-sdk` | Core SDK with context, handlers, and client |
| `durable-execution-sdk-macros` | `#[durable_execution]` proc macro |
| `durable-execution-sdk-testing` | Local and cloud test runners |

## Documentation

- [Determinism Guide](docs/DETERMINISM.md) — Writing replay-safe workflows
- [Limits Reference](docs/LIMITS.md) — Execution constraints
- [Tracing Guide](docs/TRACING.md) — Logging configuration and best practices

## License

Apache-2.0
