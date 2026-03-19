//! Checkpointing Optimistic Mode Example
//!
//! Demonstrates using `CheckpointingMode::Optimistic` which executes multiple
//! operations before checkpointing for best performance.

use durable_execution_sdk::{durable_execution, DurableError};

/// Execute steps demonstrating optimistic checkpointing mode.
///
/// This example demonstrates:
/// - `CheckpointingMode::Optimistic` delays checkpointing for best performance
/// - More work may be replayed on failure
/// - Best for workflows with cheap, idempotent operations
///
/// Note: CheckpointingMode is configured at the execution level, not per-step.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let step1: String = ctx
        .step_named(
            "optimistic_step_1",
            |_step_ctx| async move { Ok("fast_1".to_string()) },
            None,
        )
        .await?;

    let step2: String = ctx
        .step_named(
            "optimistic_step_2",
            |_step_ctx| async move { Ok("fast_2".to_string()) },
            None,
        )
        .await?;

    let step3: String = ctx
        .step_named(
            "optimistic_step_3",
            |_step_ctx| async move { Ok("fast_3".to_string()) },
            None,
        )
        .await?;

    Ok(format!("optimistic: {} + {} + {}", step1, step2, step3))
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
