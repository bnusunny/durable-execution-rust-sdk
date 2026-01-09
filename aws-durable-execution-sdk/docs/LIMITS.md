# Execution Limits and Constraints

This document describes the execution limits and constraints for durable execution workflows. Understanding these limits is essential for designing workflows that operate within service constraints.

## Overview

Durable executions have various limits to ensure system stability and fair resource usage. These limits apply to execution duration, payload sizes, checkpoint frequency, and other aspects of workflow execution.

## Execution Duration Limits

### Maximum Execution Duration

**Limit**: 1 year (365 days)

A single durable execution can run for up to one year from its start time. This includes all time spent waiting, processing, and suspended.

```rust
use aws_durable_execution_sdk::Duration;

// ✅ Valid: Wait for 30 days
ctx.wait(Duration::from_days(30), None).await?;

// ✅ Valid: Wait for 6 months
ctx.wait(Duration::from_months(6), None).await?;

// ⚠️ Approaching limit: Wait for 11 months
ctx.wait(Duration::from_months(11), None).await?;

// ❌ May exceed limit if execution has been running for a while
ctx.wait(Duration::from_years(1), None).await?;
```

### Minimum Wait Duration

**Limit**: 1 second minimum

Wait operations must specify a duration of at least 1 second.

```rust
use aws_durable_execution_sdk::Duration;

// ✅ Valid: Wait for 1 second
ctx.wait(Duration::from_seconds(1), None).await?;

// ✅ Valid: Wait for 5 seconds
ctx.wait(Duration::from_seconds(5), None).await?;

// ❌ Invalid: Duration less than 1 second will be rejected
// ctx.wait(Duration::from_seconds(0), None).await?; // ValidationError
```

### Maximum Wait Duration

**Limit**: 1 year (365 days)

A single wait operation cannot exceed 1 year.

```rust
use aws_durable_execution_sdk::Duration;

// ✅ Valid: Wait for 1 year
ctx.wait(Duration::from_years(1), None).await?;

// ❌ Invalid: Exceeds maximum wait duration
// ctx.wait(Duration::from_days(400), None).await?; // ValidationError
```

## Payload Size Limits

### Maximum Response Payload

**Limit**: 6 MB (6,291,456 bytes)

The final result returned from a durable execution cannot exceed 6 MB when serialized to JSON. This is a Lambda response size limit.

```rust
use serde::Serialize;

#[derive(Serialize)]
struct LargeResult {
    data: Vec<u8>,
}

// ⚠️ Be careful with large results
async fn workflow_with_large_result(ctx: DurableContext) -> Result<LargeResult, DurableError> {
    let data = vec![0u8; 5_000_000]; // 5 MB - OK
    Ok(LargeResult { data })
}

// ❌ This will fail if the result exceeds 6 MB
async fn workflow_too_large(ctx: DurableContext) -> Result<LargeResult, DurableError> {
    let data = vec![0u8; 7_000_000]; // 7 MB - Too large!
    Ok(LargeResult { data })
}
```

**Handling Large Results**:

If your workflow produces large results, consider:

1. **Store in S3**: Write large data to S3 and return the S3 key
2. **Checkpoint the result**: The SDK can checkpoint large results via the EXECUTION operation, returning an empty result field
3. **Paginate**: Return results in smaller chunks

```rust
// ✅ Better approach: Store large data externally
async fn workflow_large_data(ctx: DurableContext) -> Result<String, DurableError> {
    let large_data = generate_large_data();
    
    // Store in S3 and return the key
    let s3_key: String = ctx.step(|_| {
        let key = format!("results/{}", uuid::Uuid::new_v4());
        s3_client.put_object(&key, &large_data)?;
        Ok(key)
    }, None).await?;
    
    Ok(s3_key) // Return small reference instead of large data
}
```

### Maximum Checkpoint Payload

**Limit**: 256 KB per operation

Each individual checkpoint (step result, callback result, etc.) is limited to 256 KB when serialized.

```rust
// ⚠️ Step results should be reasonably sized
let result: Vec<u8> = ctx.step(|_| {
    Ok(vec![0u8; 200_000]) // 200 KB - OK
}, None).await?;

// ❌ This may fail if the result exceeds 256 KB
let large_result: Vec<u8> = ctx.step(|_| {
    Ok(vec![0u8; 300_000]) // 300 KB - Too large for checkpoint!
}, None).await?;
```

### Maximum Input Payload

**Limit**: 256 KB

The input payload to a durable execution is limited to 256 KB.

## Checkpoint and History Limits

### Maximum History Size

**Limit**: 25,000 operations per execution

A single execution can have at most 25,000 operations in its history. This includes all steps, waits, callbacks, invokes, and context operations.

```rust
// ⚠️ Be mindful of operation count in loops
async fn many_operations(ctx: DurableContext, items: Vec<Item>) -> Result<(), DurableError> {
    // If items.len() > 25,000, this will eventually fail
    for item in items {
        ctx.step(|_| process_item(&item), None).await?;
    }
    Ok(())
}

// ✅ Better: Use map for batch processing
async fn batch_operations(ctx: DurableContext, items: Vec<Item>) -> Result<(), DurableError> {
    // Map creates fewer top-level operations
    ctx.map(items, |child_ctx, item, _| {
        child_ctx.step(|_| process_item(&item), None)
    }, None).await?;
    Ok(())
}
```

### Maximum Checkpoint Batch Size

**Limit**: 750 KB per batch

The SDK batches multiple checkpoint operations together for efficiency. Each batch is limited to 750 KB total.

## Callback Limits

### Maximum Callback Timeout

**Limit**: 1 year (365 days)

A callback can wait for up to 1 year before timing out.

```rust
use aws_durable_execution_sdk::CallbackConfig;

// ✅ Valid: 24-hour callback timeout
let callback = ctx.create_callback::<Response>(Some(CallbackConfig {
    timeout: Duration::from_hours(24),
    ..Default::default()
})).await?;

// ✅ Valid: 30-day callback timeout
let callback = ctx.create_callback::<Response>(Some(CallbackConfig {
    timeout: Duration::from_days(30),
    ..Default::default()
})).await?;
```

### Heartbeat Timeout

**Limit**: Must be less than callback timeout

If configured, the heartbeat timeout must be less than the overall callback timeout.

## Concurrency Limits

### Map/Parallel Concurrency

**Recommendation**: Set appropriate `max_concurrency` based on downstream service limits

While there's no hard limit on concurrency, you should configure `max_concurrency` to avoid overwhelming downstream services.

```rust
use aws_durable_execution_sdk::MapConfig;

// ✅ Good: Limit concurrency to avoid overwhelming downstream services
ctx.map(items, process_item, Some(MapConfig {
    max_concurrency: Some(10),
    ..Default::default()
})).await?;

// ⚠️ Caution: Unlimited concurrency may cause issues
ctx.map(items, process_item, None).await?;
```

## Invoke Limits

### Chained Invoke Depth

**Recommendation**: Keep invoke chains shallow

While there's no hard limit on invoke depth, deep chains can be harder to debug and may hit Lambda concurrency limits.

## Error Handling for Limits

When limits are exceeded, the SDK returns specific error types:

```rust
use aws_durable_execution_sdk::DurableError;

match result {
    Err(DurableError::SizeLimit { message }) => {
        // Payload size exceeded
        eprintln!("Size limit exceeded: {}", message);
    }
    Err(DurableError::Validation { message }) => {
        // Invalid configuration (e.g., duration too short/long)
        eprintln!("Validation error: {}", message);
    }
    Err(DurableError::Throttling { message }) => {
        // Rate limit exceeded
        eprintln!("Throttled: {}", message);
    }
    _ => {}
}
```

## Summary Table

| Limit | Value | Notes |
|-------|-------|-------|
| Maximum execution duration | 1 year | Total time from start to completion |
| Minimum wait duration | 1 second | Per wait operation |
| Maximum wait duration | 1 year | Per wait operation |
| Maximum response payload | 6 MB | Final result returned to caller |
| Maximum checkpoint payload | 256 KB | Per operation result |
| Maximum input payload | 256 KB | Initial execution input |
| Maximum history size | 25,000 ops | Total operations per execution |
| Maximum checkpoint batch | 750 KB | Per checkpoint API call |
| Maximum callback timeout | 1 year | Per callback |

## Best Practices

1. **Monitor operation count**: Track how many operations your workflows create and refactor if approaching limits.

2. **Keep payloads small**: Store large data externally (S3, DynamoDB) and pass references in checkpoints.

3. **Set reasonable timeouts**: Don't use maximum timeouts unless necessary. Shorter timeouts help detect issues faster.

4. **Use batching**: For processing many items, use `map` with appropriate batching instead of individual steps.

5. **Handle limit errors gracefully**: Implement proper error handling for size limit and validation errors.
