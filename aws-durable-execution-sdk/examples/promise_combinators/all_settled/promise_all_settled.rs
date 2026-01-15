//! Promise All Settled Example
//!
//! Wait for all operations to settle (success or failure) using the `all_settled!` macro
//! or `ctx.map()`.
//!
//! # When to Use Each Approach
//!
//! - **`all_settled!` macro**: Best for combining different step operations where you
//!   want to collect all outcomes regardless of success/failure
//! - **`ctx.map()`**: Best for processing collections with automatic iteration

use aws_durable_execution_sdk::{all_settled, durable_execution, DurableError, DurableContext, MapConfig, CompletionConfig};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettledResult {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
}

/// Wait for all operations to settle using the `all_settled!` macro.
///
/// This example shows the `all_settled!` macro for collecting all outcomes
/// from multiple different operations, regardless of success or failure.
#[durable_execution]
pub async fn handler_with_macro(_event: serde_json::Value, ctx: DurableContext) -> Result<SettledResult, DurableError> {
    // Different closures = different types, so use the all_settled! macro
    // Note: Use async move blocks with cloned contexts to satisfy lifetime requirements
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();
    let ctx3 = ctx.clone();
    
    let batch = all_settled!(ctx,
        async move { ctx1.step(|_| Ok("op1 success".to_string()), None).await },
        async move { ctx2.step(|_| Err::<String, _>("op2 failed".into()), None).await },
        async move { ctx3.step(|_| Ok("op3 success".to_string()), None).await },
    ).await?;

    // Process the results
    println!("Total operations: {}", batch.items.len());
    println!("Succeeded: {}", batch.success_count());
    println!("Failed: {}", batch.failure_count());

    // Access successful results
    for item in batch.succeeded() {
        if let Some(result) = item.get_result() {
            println!("Success at index {}: {}", item.index, result);
        }
    }

    // Access failures for logging/retry
    for item in batch.failed() {
        if let Some(error) = item.get_error() {
            eprintln!("Failed at index {}: {:?}", item.index, error);
        }
    }

    Ok(SettledResult {
        total: batch.items.len(),
        succeeded: batch.success_count(),
        failed: batch.failure_count(),
    })
}

/// Wait for all operations to settle using `ctx.map()`.
///
/// Uses `ctx.map()` which collects both successes and failures
/// in a BatchResult.
#[durable_execution]
pub async fn handler(_event: serde_json::Value, ctx: DurableContext) -> Result<SettledResult, DurableError> {
    // Define operations - some will succeed, some will fail (using owned types)
    let operations: Vec<(String, bool)> = vec![
        ("op1".to_string(), true),   // will succeed
        ("op2".to_string(), false),  // will fail
        ("op3".to_string(), true),   // will succeed
    ];

    // Use map with all_completed to wait for everything
    let config = MapConfig {
        completion_config: CompletionConfig::all_completed(),
        ..Default::default()
    };

    let batch_result = ctx.map(
        operations,
        |child_ctx: DurableContext, (name, should_succeed): (String, bool), _index: usize| {
            Box::pin(async move {
                if should_succeed {
                    child_ctx.step(|_| Ok(format!("{} success", name)), None).await
                } else {
                    child_ctx.step(|_| Err::<String, _>(format!("{} failed", name).into()), None).await
                }
            })
        },
        Some(config),
    ).await?;

    Ok(SettledResult {
        total: batch_result.items.len(),
        succeeded: batch_result.succeeded().len(),
        failed: batch_result.failed().len(),
    })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    // Use the macro-based handler by default
    lambda_runtime::run(lambda_runtime::service_fn(handler_with_macro)).await
}
