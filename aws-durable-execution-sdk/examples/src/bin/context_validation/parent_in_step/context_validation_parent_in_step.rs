//! Context Validation: Parent in Step Example
//!
//! Demonstrates that using a parent `DurableContext` to call durable operations
//! inside a child context's closure produces an error. The step closure itself
//! is synchronous (`FnOnce(StepContext) -> Result<T, Error>`), so async durable
//! operations cannot be called inside it. Instead, this example shows that using
//! the parent context inside a child context closure (rather than the child context)
//! produces incorrect behavior or an error.

use aws_durable_execution_sdk::{durable_execution, DurableError};

/// Attempt to use the parent context inside a child context to call a step.
///
/// This handler demonstrates context misuse by cloning the parent context
/// and using it inside `run_in_child_context` instead of the provided child context.
/// The SDK detects this and returns an error.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    // First, do a normal step on the parent context
    let _setup: String = ctx
        .step_named("setup", |_| Ok("setup_done".to_string()), None)
        .await?;

    let parent_ctx = ctx.clone();

    // Now try to use the parent context inside a child context
    let result = ctx
        .run_in_child_context_named(
            "misuse_child",
            |_child_ctx| {
                let parent = parent_ctx;
                Box::pin(async move {
                    // Misuse: calling step on the PARENT context inside a child context
                    let val: String = parent
                        .step_named("parent_step_in_child", |_| Ok("from_parent".to_string()), None)
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
