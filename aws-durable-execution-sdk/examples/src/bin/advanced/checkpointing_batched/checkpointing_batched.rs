//! Checkpointing Batched Mode Example
//!
//! Demonstrates using `CheckpointingMode::Batched` (the default) which groups
//! multiple operations into batches before checkpointing for balanced performance.

use aws_durable_execution_sdk::{durable_execution, DurableError};

/// Execute steps demonstrating batched checkpointing mode.
///
/// This example demonstrates:
/// - `CheckpointingMode::Batched` groups operations before checkpointing
/// - Balanced durability and performance (default mode)
/// - Best for most general-purpose workflows
///
/// Note: CheckpointingMode is configured at the execution level, not per-step.
/// Batched is the default mode.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let step1: String = ctx
        .step_named(
            "batched_step_1",
            |_step_ctx| Ok("batched_1".to_string()),
            None,
        )
        .await?;

    let step2: String = ctx
        .step_named(
            "batched_step_2",
            |_step_ctx| Ok("batched_2".to_string()),
            None,
        )
        .await?;

    let step3: String = ctx
        .step_named(
            "batched_step_3",
            |_step_ctx| Ok("batched_3".to_string()),
            None,
        )
        .await?;

    Ok(format!("batched: {} + {} + {}", step1, step2, step3))
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
