//! Promise All Example
//!
//! Wait for all operations to complete using `ctx.all()` or the `all!` macro.
//!
//! # When to Use Each Approach
//!
//! - **`all!` macro**: Best for combining different step operations (heterogeneous futures)
//! - **`ctx.all()` method**: Best for homogeneous futures from iterators/loops
//! - **`ctx.map()`**: Best for processing collections with automatic iteration

use aws_durable_execution_sdk::{all, durable_execution, DurableError, DurableContext, MapConfig};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedResult {
    pub results: Vec<i32>,
    pub sum: i32,
}

/// Wait for all operations using the `all!` macro - heterogeneous futures.
///
/// This example shows when `all!` macro works well: when combining different
/// step operations with different closures. Each closure has a unique type,
/// but the macro handles type erasure automatically.
#[durable_execution]
pub async fn handler_with_macro(_event: serde_json::Value, ctx: DurableContext) -> Result<AggregatedResult, DurableError> {
    // Different closures = different types - use the all! macro!
    // The macro automatically boxes each future to erase types
    // Note: Use async move blocks with cloned contexts to satisfy lifetime requirements
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();
    let ctx3 = ctx.clone();
    
    let results = all!(ctx,
        async move { ctx1.step(|_| Ok(10), None).await },
        async move { ctx2.step(|_| Ok(20), None).await },
        async move { ctx3.step(|_| Ok(30), None).await },
    ).await?;

    let sum: i32 = results.iter().sum();
    Ok(AggregatedResult { results, sum })
}

/// Wait for all operations using ctx.all() - homogeneous futures.
///
/// This example shows when `ctx.all()` works well: when all futures
/// are created from the same closure/function, they have the same type.
#[durable_execution]
pub async fn handler(_event: serde_json::Value, ctx: DurableContext) -> Result<AggregatedResult, DurableError> {
    let values = vec![10, 20, 30];
    
    // Create futures from the same closure - they all have the same type!
    // This is the key: use a single closure that captures different data
    let futures: Vec<_> = values
        .into_iter()
        .map(|v| {
            let ctx = ctx.clone();
            async move {
                ctx.step(move |_| Ok(v), None).await
            }
        })
        .collect();

    // Now ctx.all() works because all futures have the same type
    let results = ctx.all(futures).await?;
    let sum: i32 = results.iter().sum();

    Ok(AggregatedResult { results, sum })
}

/// Alternative using ctx.map() - more ergonomic for most cases.
///
/// `ctx.map()` is often simpler because it handles the iteration
/// and type erasure for you.
#[durable_execution]
pub async fn handler_with_map(_event: serde_json::Value, ctx: DurableContext) -> Result<AggregatedResult, DurableError> {
    let values = vec![10, 20, 30];

    let batch_result = ctx.map(
        values,
        |child_ctx: DurableContext, value: i32, _index: usize| {
            Box::pin(async move {
                child_ctx.step(|_| Ok(value), None).await
            })
        },
        Some(MapConfig {
            max_concurrency: Some(3),
            ..Default::default()
        }),
    ).await?;

    let results = batch_result.get_results()?.into_iter().cloned().collect::<Vec<_>>();
    let sum: i32 = results.iter().sum();

    Ok(AggregatedResult { results, sum })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    // Use the macro-based handler by default
    lambda_runtime::run(lambda_runtime::service_fn(handler_with_macro)).await
}
