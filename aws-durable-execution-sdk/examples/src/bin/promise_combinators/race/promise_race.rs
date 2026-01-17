//! Promise Race Example
//!
//! Return the first operation to complete (success or failure) using the `race!` macro
//! or `ctx.map()` with `CompletionConfig::first_successful()`.
//!
//! # When to Use Each Approach
//!
//! - **`race!` macro**: Best for combining different step operations where you want
//!   the first result regardless of success/failure (e.g., timeout patterns)
//! - **`ctx.map()` with first_successful**: Best for processing collections where
//!   you want the first success

use aws_durable_execution_sdk::{
    durable_execution, race, CompletionConfig, DurableContext, DurableError, MapConfig,
};

/// Return the first operation to complete using the `race!` macro.
///
/// This example shows the `race!` macro for competitive operations where
/// you want the first result to settle, regardless of success or failure.
/// This is useful for timeout patterns.
#[durable_execution]
pub async fn handler_with_macro(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    // Race between multiple operations - first to settle wins
    // Different closures = different types, so use the race! macro
    // Note: Use async move blocks with cloned contexts to satisfy lifetime requirements
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();

    let result = race!(
        ctx,
        async move {
            // Fast operation
            ctx1.step(|_| Ok("fast result".to_string()), None).await
        },
        async move {
            // Slower operation (may not complete if fast wins)
            ctx2.step(|_| Ok("slow result".to_string()), None).await
        },
    )
    .await?;

    Ok(result)
}

/// Example: Timeout pattern using race!
///
/// Race between an operation and a timeout - whichever settles first wins.
#[durable_execution]
pub async fn handler_with_timeout(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    // Race between operation and timeout
    // Note: Use async move blocks with cloned contexts to satisfy lifetime requirements
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();

    let result = race!(
        ctx,
        async move {
            // Simulate a slow operation
            ctx1.step(|_| Ok("operation completed".to_string()), None)
                .await
        },
        async move {
            // Timeout - returns error to signal timeout
            ctx2.step(|_| Err::<String, _>("operation timed out".into()), None)
                .await
        },
    )
    .await;

    // Handle the result - could be success or timeout error
    match result {
        Ok(data) => Ok(data),
        Err(e) => {
            // Could log the timeout and return a default or propagate
            Err(e)
        }
    }
}

/// Return the first operation to complete using `ctx.map()`.
///
/// Uses `ctx.map()` with first_successful completion config
/// to return as soon as any operation succeeds.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    // Define competing operations (using String for serialization)
    let operations: Vec<String> =
        vec!["fast".to_string(), "medium".to_string(), "slow".to_string()];

    // Configure to return on first success
    let config = MapConfig {
        completion_config: CompletionConfig::first_successful(),
        max_concurrency: Some(3),
        ..Default::default()
    };

    // Create competing operations
    let batch_result = ctx
        .map(
            operations,
            |child_ctx: DurableContext, name: String, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(|_| Ok(format!("{} result", name)), None)
                        .await
                })
            },
            Some(config),
        )
        .await?;

    // Get the first successful result
    let results = batch_result.get_results()?;
    Ok(results.first().cloned().cloned().unwrap_or_default())
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    // Use the macro-based handler by default
    lambda_runtime::run(lambda_runtime::service_fn(handler_with_macro)).await
}
