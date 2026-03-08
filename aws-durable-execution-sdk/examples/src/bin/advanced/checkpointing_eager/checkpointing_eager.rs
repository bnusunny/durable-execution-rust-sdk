//! Checkpointing Eager Mode Example
//!
//! Demonstrates using `CheckpointingMode::Eager` which checkpoints after every
//! operation for maximum durability.

use durable_execution_sdk::{durable_execution, DurableError};

/// Execute steps demonstrating eager checkpointing mode.
///
/// This example demonstrates:
/// - `CheckpointingMode::Eager` flushes checkpoints after every operation
/// - Maximum durability at the cost of higher latency
/// - Best for critical workflows where every operation must be durable
///
/// Note: CheckpointingMode is configured at the execution level, not per-step.
/// This example shows the handler pattern used with eager checkpointing.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let step1: String = ctx
        .step_named(
            "critical_step_1",
            |_step_ctx| Ok("checkpoint_after_this".to_string()),
            None,
        )
        .await?;

    let step2: String = ctx
        .step_named(
            "critical_step_2",
            |_step_ctx| Ok("also_checkpointed".to_string()),
            None,
        )
        .await?;

    Ok(format!("eager: {} + {}", step1, step2))
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
