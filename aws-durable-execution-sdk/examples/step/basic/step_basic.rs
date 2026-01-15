//! Basic Step Example
//!
//! Basic usage of `ctx.step()` to checkpoint a simple operation.
//! Steps are the fundamental unit of work in durable executions.

use aws_durable_execution_sdk::{durable_execution, DurableError};

/// Execute a single step and return its result.
///
/// The step is automatically checkpointed. If the Lambda restarts,
/// the step result will be returned from the checkpoint without
/// re-executing the step logic.
#[durable_execution]
pub async fn handler(_event: serde_json::Value, ctx: DurableContext) -> Result<String, DurableError> {
    let result = ctx
        .step(|_step_ctx| Ok("step completed".to_string()), None)
        .await?;

    Ok(result)
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
