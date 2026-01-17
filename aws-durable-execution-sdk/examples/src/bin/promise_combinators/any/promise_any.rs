//! Promise Any Example
//!
//! Return the first successful operation using the `any!` macro or `ctx.map()`
//! with `CompletionConfig::first_successful()`.
//!
//! # When to Use Each Approach
//!
//! - **`any!` macro**: Best for combining different step operations (heterogeneous futures)
//! - **`ctx.map()` with first_successful**: Best for processing collections

use aws_durable_execution_sdk::{
    any, durable_execution, CompletionConfig, DurableContext, DurableError, MapConfig,
};

/// Return the first operation to succeed using the `any!` macro.
///
/// This example shows the `any!` macro for fallback patterns where you want
/// the first successful result from multiple different operations.
#[durable_execution]
pub async fn handler_with_macro(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    // Try multiple sources - return first success
    // Different closures = different types, so use the any! macro
    // Note: Use async move blocks with cloned contexts to satisfy lifetime requirements
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();
    let ctx3 = ctx.clone();

    let result = any!(
        ctx,
        async move {
            // Simulate primary source failure
            ctx1.step(|_| Err::<String, _>("primary unavailable".into()), None)
                .await
        },
        async move {
            // Secondary source succeeds
            ctx2.step(|_| Ok("data from secondary".to_string()), None)
                .await
        },
        async move {
            // Fallback source (may not be called if secondary succeeds)
            ctx3.step(|_| Ok("data from fallback".to_string()), None)
                .await
        },
    )
    .await?;

    Ok(result)
}

/// Return the first operation to succeed using `ctx.map()`.
///
/// Uses `ctx.map()` with first_successful completion config
/// to return the first successful result, ignoring failures.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    // Define operations - some will fail, some will succeed (using owned types)
    let operations: Vec<(String, bool)> = vec![
        ("op1".to_string(), false), // will fail
        ("op2".to_string(), true),  // will succeed
        ("op3".to_string(), true),  // will succeed
    ];

    // Configure to return on first success
    let config = MapConfig {
        completion_config: CompletionConfig::first_successful(),
        max_concurrency: Some(3),
        ..Default::default()
    };

    // Create operations where some may fail
    let batch_result = ctx
        .map(
            operations,
            |child_ctx: DurableContext, (name, should_succeed): (String, bool), _index: usize| {
                Box::pin(async move {
                    if should_succeed {
                        child_ctx
                            .step(|_| Ok(format!("{} succeeded", name)), None)
                            .await
                    } else {
                        child_ctx
                            .step(
                                |_| Err::<String, _>(format!("{} failed", name).into()),
                                None,
                            )
                            .await
                    }
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
