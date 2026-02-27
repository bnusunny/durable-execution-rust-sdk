//! Context Validation: Parent in Child Example
//!
//! Demonstrates that using a parent `DurableContext` inside `run_in_child_context`
//! produces an error. The parent context should not be used for durable operations
//! within a child context's closure — only the child context parameter should be used.

use aws_durable_execution_sdk::{durable_execution, DurableError};

/// Attempt to use the parent context inside a child context closure.
///
/// This handler clones the parent context and then tries to call a step
/// on the parent context inside `run_in_child_context`. The SDK detects
/// this misuse and returns an error, which the handler catches and reports.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let parent_ctx = ctx.clone();

    let result = ctx
        .run_in_child_context(
            |_child_ctx| {
                let parent = parent_ctx;
                Box::pin(async move {
                    // Misuse: calling step on the PARENT context inside a child context
                    let val: String = parent
                        .step(|_| Ok("from_parent".to_string()), None)
                        .await?;
                    Ok(val)
                })
            },
            None,
        )
        .await;

    match result {
        Ok(val) => Ok(format!("unexpected_success: {}", val)),
        Err(e) => Ok(format!("caught_error: {}", e)),
    }
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
