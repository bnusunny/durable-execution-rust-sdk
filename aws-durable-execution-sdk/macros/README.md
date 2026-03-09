# durable-execution-sdk-macros

Procedural macros for the [AWS Durable Execution SDK](https://crates.io/crates/durable-execution-sdk).

## Overview

This crate provides the `#[durable_execution]` attribute macro that transforms an async function into a Lambda handler compatible with the AWS Durable Execution service. You typically don't need to depend on this crate directly — it's re-exported by `durable-execution-sdk` when the `macros` feature is enabled (on by default).

## Usage

```rust
use durable_execution_sdk::{durable_execution, DurableContext, DurableError};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct MyEvent {
    order_id: String,
}

#[derive(Serialize)]
struct MyResult {
    status: String,
}

#[durable_execution]
async fn my_handler(event: MyEvent, ctx: DurableContext) -> Result<MyResult, DurableError> {
    let result = ctx.step(|_| Ok("processed".to_string()), None).await?;
    Ok(MyResult { status: result })
}
```

## What the macro does

The `#[durable_execution]` attribute generates two functions from your handler:

1. An inner async function (`__<name>_inner`) containing your original logic
2. A Lambda handler wrapper that accepts `LambdaEvent<DurableExecutionInvocationInput>` and delegates to `run_durable_handler` with the inner function

All runtime concerns (event deserialization, state management, context creation, result/error/suspend handling) are encapsulated in `run_durable_handler`.

## Function signature requirements

The decorated function must:

- Be `async`
- Have exactly two parameters: `(event: EventType, ctx: DurableContext)`
- Return `Result<T, DurableError>` where `T: Serialize`
- `EventType` must implement `Deserialize`

## License

Apache-2.0
