# Determinism in Durable Executions

This document explains the determinism requirements for durable execution workflows and provides guidance on writing correct, replay-safe code.

## Why Determinism Matters

Durable execution workflows can be interrupted and replayed at any point. When a workflow resumes after an interruption (e.g., Lambda timeout, failure, or scaling), the SDK replays the workflow from the beginning, returning checkpointed results for completed operations without re-executing them.

**The critical requirement**: Your workflow code must execute the same sequence of operations in the same order every time it runs. If the sequence changes between the original execution and a replay, the SDK cannot correctly match operations to their checkpointed results, leading to `NonDeterministicExecutionError`.

### How Replay Works

```
Original Execution:
┌─────────────────────────────────────────────────────────────┐
│ step("validate") → step("process") → wait(5s) → step("complete") │
│      ↓                   ↓              ↓            ↓       │
│  checkpoint          checkpoint     checkpoint   checkpoint  │
└─────────────────────────────────────────────────────────────┘

Replay After Interruption (e.g., after wait):
┌─────────────────────────────────────────────────────────────┐
│ step("validate") → step("process") → wait(5s) → step("complete") │
│      ↓                   ↓              ↓            ↓       │
│  return cached      return cached   return cached  EXECUTE   │
└─────────────────────────────────────────────────────────────┘
```

During replay, the SDK matches each operation by its deterministic operation ID. If your code takes a different path during replay, the operation IDs won't match, and the SDK will detect a non-deterministic execution.

## Common Sources of Non-Determinism in Rust

### 1. HashMap and HashSet Iteration Order

**Problem**: Rust's `HashMap` and `HashSet` use randomized hashing by default, meaning iteration order is not guaranteed to be consistent across runs.

```rust
use std::collections::HashMap;

// ❌ INCORRECT: HashMap iteration order is non-deterministic
async fn process_items_wrong(ctx: &DurableContext, items: HashMap<String, i32>) -> Result<(), DurableError> {
    for (key, value) in items.iter() {
        // The order of iteration may differ between runs!
        ctx.step(|_| Ok(value * 2), None).await?;
    }
    Ok(())
}

// ✅ CORRECT: Sort keys before iteration
async fn process_items_correct(ctx: &DurableContext, items: HashMap<String, i32>) -> Result<(), DurableError> {
    let mut keys: Vec<_> = items.keys().collect();
    keys.sort();
    
    for key in keys {
        let value = items.get(key).unwrap();
        ctx.step(|_| Ok(value * 2), None).await?;
    }
    Ok(())
}

// ✅ CORRECT: Use BTreeMap for deterministic iteration
use std::collections::BTreeMap;

async fn process_items_btree(ctx: &DurableContext, items: BTreeMap<String, i32>) -> Result<(), DurableError> {
    for (key, value) in items.iter() {
        // BTreeMap iteration is always sorted by key
        ctx.step(|_| Ok(value * 2), None).await?;
    }
    Ok(())
}
```

### 2. Random Number Generation

**Problem**: Random number generators produce different values on each run.

```rust
use rand::Rng;

// ❌ INCORRECT: Random values differ between runs
async fn random_workflow_wrong(ctx: &DurableContext) -> Result<i32, DurableError> {
    let mut rng = rand::thread_rng();
    let random_value = rng.gen_range(0..100);
    
    if random_value > 50 {
        // This branch may or may not execute on replay!
        ctx.step(|_| Ok("high"), None).await?;
    }
    Ok(random_value)
}

// ✅ CORRECT: Generate random values inside a step
async fn random_workflow_correct(ctx: &DurableContext) -> Result<i32, DurableError> {
    let random_value: i32 = ctx.step(|_| {
        let mut rng = rand::thread_rng();
        Ok(rng.gen_range(0..100))
    }, None).await?;
    
    // Now random_value is checkpointed and consistent across replays
    if random_value > 50 {
        ctx.step(|_| Ok("high"), None).await?;
    }
    Ok(random_value)
}
```

### 3. Current Time / Timestamps

**Problem**: `SystemTime::now()` returns different values on each run.

```rust
use std::time::SystemTime;

// ❌ INCORRECT: Time differs between runs
async fn time_workflow_wrong(ctx: &DurableContext) -> Result<(), DurableError> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    if now % 2 == 0 {
        // This branch depends on current time!
        ctx.step(|_| Ok("even second"), None).await?;
    }
    Ok(())
}

// ✅ CORRECT: Use replay-safe timestamp helper
use aws_durable_execution_sdk::replay_safe::timestamp_seconds_from_execution;

async fn time_workflow_correct(ctx: &DurableContext) -> Result<(), DurableError> {
    // This returns the execution start time, consistent across replays
    let start_time = timestamp_seconds_from_execution(ctx.state())
        .unwrap_or(0);
    
    if start_time % 2 == 0 {
        ctx.step(|_| Ok("even second"), None).await?;
    }
    Ok(())
}

// ✅ CORRECT: Capture time inside a step
async fn time_workflow_step(ctx: &DurableContext) -> Result<(), DurableError> {
    let captured_time: u64 = ctx.step(|_| {
        Ok(SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs())
    }, None).await?;
    
    // captured_time is now checkpointed
    if captured_time % 2 == 0 {
        ctx.step(|_| Ok("even second"), None).await?;
    }
    Ok(())
}
```

### 4. UUID Generation

**Problem**: UUIDs are designed to be unique, meaning they differ on each generation.

```rust
use uuid::Uuid;

// ❌ INCORRECT: UUID differs between runs
async fn uuid_workflow_wrong(ctx: &DurableContext) -> Result<String, DurableError> {
    let id = Uuid::new_v4().to_string();
    
    // Using this UUID in operation names or logic is non-deterministic
    ctx.step(|_| Ok(format!("processed-{}", id)), None).await
}

// ✅ CORRECT: Use replay-safe UUID helper
use aws_durable_execution_sdk::replay_safe::uuid_string_from_operation;

async fn uuid_workflow_correct(ctx: &DurableContext) -> Result<String, DurableError> {
    let operation_id = ctx.next_operation_id();
    let id = uuid_string_from_operation(&operation_id, 0);
    
    // This UUID is deterministic based on operation ID
    ctx.step(|_| Ok(format!("processed-{}", id)), None).await
}

// ✅ CORRECT: Generate UUID inside a step
async fn uuid_workflow_step(ctx: &DurableContext) -> Result<String, DurableError> {
    let id: String = ctx.step(|_| {
        Ok(Uuid::new_v4().to_string())
    }, None).await?;
    
    // id is now checkpointed and consistent
    ctx.step(|_| Ok(format!("processed-{}", id)), None).await
}
```

### 5. Environment Variables and Configuration

**Problem**: Environment variables or external configuration may change between runs.

```rust
// ❌ INCORRECT: Environment may change between runs
async fn env_workflow_wrong(ctx: &DurableContext) -> Result<(), DurableError> {
    let feature_enabled = std::env::var("FEATURE_FLAG")
        .map(|v| v == "true")
        .unwrap_or(false);
    
    if feature_enabled {
        // This branch depends on environment!
        ctx.step(|_| Ok("feature enabled"), None).await?;
    }
    Ok(())
}

// ✅ CORRECT: Capture configuration in a step at the start
async fn env_workflow_correct(ctx: &DurableContext) -> Result<(), DurableError> {
    let feature_enabled: bool = ctx.step(|_| {
        Ok(std::env::var("FEATURE_FLAG")
            .map(|v| v == "true")
            .unwrap_or(false))
    }, None).await?;
    
    // feature_enabled is now checkpointed
    if feature_enabled {
        ctx.step(|_| Ok("feature enabled"), None).await?;
    }
    Ok(())
}
```

### 6. External API Calls Outside Steps

**Problem**: External APIs may return different results on each call.

```rust
// ❌ INCORRECT: API response may differ between runs
async fn api_workflow_wrong(ctx: &DurableContext) -> Result<(), DurableError> {
    let response = reqwest::get("https://api.example.com/status")
        .await?
        .json::<ApiStatus>()
        .await?;
    
    if response.is_healthy {
        // This branch depends on external API state!
        ctx.step(|_| Ok("healthy"), None).await?;
    }
    Ok(())
}

// ✅ CORRECT: Make API calls inside steps
async fn api_workflow_correct(ctx: &DurableContext) -> Result<(), DurableError> {
    let response: ApiStatus = ctx.step(|_| {
        // Make the API call here - result will be checkpointed
        Ok(ApiStatus { is_healthy: true })
    }, None).await?;
    
    // response is now checkpointed
    if response.is_healthy {
        ctx.step(|_| Ok("healthy"), None).await?;
    }
    Ok(())
}
```

### 7. Thread-Local State

**Problem**: Thread-local state may not be preserved across Lambda invocations.

```rust
use std::cell::RefCell;

thread_local! {
    static COUNTER: RefCell<i32> = RefCell::new(0);
}

// ❌ INCORRECT: Thread-local state is not preserved
async fn thread_local_wrong(ctx: &DurableContext) -> Result<(), DurableError> {
    COUNTER.with(|c| *c.borrow_mut() += 1);
    let count = COUNTER.with(|c| *c.borrow());
    
    if count > 5 {
        // This depends on thread-local state!
        ctx.step(|_| Ok("high count"), None).await?;
    }
    Ok(())
}

// ✅ CORRECT: Use checkpointed state
async fn thread_local_correct(ctx: &DurableContext) -> Result<(), DurableError> {
    let count: i32 = ctx.step(|_| {
        // Compute or fetch the count deterministically
        Ok(6)
    }, None).await?;
    
    if count > 5 {
        ctx.step(|_| Ok("high count"), None).await?;
    }
    Ok(())
}
```

### 8. Floating-Point Arithmetic Edge Cases

**Problem**: While floating-point arithmetic is generally deterministic in Rust, certain operations (like `f64::sin()`, `f64::exp()`) may have platform-specific implementations that produce slightly different results.

```rust
// ⚠️ CAUTION: Floating-point edge cases
async fn float_workflow(ctx: &DurableContext, value: f64) -> Result<(), DurableError> {
    // Basic arithmetic is safe
    let sum = value + 1.0;
    
    // Transcendental functions may vary slightly across platforms
    // If you use these for branching, checkpoint the result first
    let sin_value: f64 = ctx.step(|_| Ok(value.sin()), None).await?;
    
    if sin_value > 0.5 {
        ctx.step(|_| Ok("positive"), None).await?;
    }
    Ok(())
}
```

## Rules for Writing Deterministic Workflows

### Rule 1: Same Input → Same Operation Sequence

Given the same input, your workflow must always execute the same sequence of durable operations (steps, waits, callbacks, etc.) in the same order.

### Rule 2: Capture Non-Deterministic Values in Steps

Any value that might differ between runs should be captured inside a step:
- Random numbers
- Current time
- UUIDs
- Environment variables
- External API responses

### Rule 3: Use Deterministic Data Structures

- Prefer `BTreeMap` over `HashMap` when iteration order matters
- Prefer `BTreeSet` over `HashSet` when iteration order matters
- Sort collections before iterating if you must use hash-based structures

### Rule 4: Avoid Side Effects Outside Steps

Side effects (logging, metrics, external calls) outside of steps are fine as long as they don't affect the control flow of your workflow.

```rust
// ✅ OK: Logging doesn't affect control flow
async fn logging_ok(ctx: &DurableContext) -> Result<(), DurableError> {
    tracing::info!("Starting workflow");  // Fine - doesn't affect flow
    
    let result = ctx.step(|_| Ok(42), None).await?;
    
    tracing::info!("Got result: {}", result);  // Fine
    Ok(())
}

// ❌ INCORRECT: Side effect affects control flow
async fn logging_wrong(ctx: &DurableContext) -> Result<(), DurableError> {
    let log_result = external_logger.log("Starting").await;
    
    if log_result.is_err() {
        // Control flow depends on external side effect!
        ctx.step(|_| Ok("logging failed"), None).await?;
    }
    Ok(())
}
```

### Rule 5: Use Replay-Safe Helpers

The SDK provides helpers for common non-deterministic operations:

- `uuid_from_operation`: Deterministic UUID generation
- `timestamp_from_execution`: Replay-safe timestamps

## Detecting Non-Determinism

The SDK automatically detects non-deterministic execution by comparing the operation type at each operation ID during replay. If a mismatch is detected, the SDK raises a `NonDeterministicExecutionError`.

```
Original: step("a") → step("b") → wait(5s)
Replay:   step("a") → wait(5s)  → ???
                      ^^^^^^^^
                      Mismatch! Expected step, got wait
                      → NonDeterministicExecutionError
```

## Testing for Determinism

To verify your workflow is deterministic:

1. **Run the workflow multiple times** with the same input and verify the operation sequence is identical.

2. **Simulate replay** by running the workflow, capturing checkpoints, then running again with those checkpoints pre-loaded.

3. **Review code for non-deterministic patterns** listed above.

## Summary

| Source of Non-Determinism | Solution |
|---------------------------|----------|
| HashMap/HashSet iteration | Use BTreeMap/BTreeSet or sort keys |
| Random numbers | Generate inside steps |
| Current time | Use `timestamp_from_execution` or capture in step |
| UUIDs | Use `uuid_from_operation` or generate in step |
| Environment variables | Capture in step at workflow start |
| External API calls | Make calls inside steps |
| Thread-local state | Use checkpointed state instead |
| Platform-specific floats | Checkpoint results of transcendental functions |
