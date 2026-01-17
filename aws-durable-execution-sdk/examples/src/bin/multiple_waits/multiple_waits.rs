//! Multiple Waits Example
//!
//! Using multiple wait operations in a workflow.

use aws_durable_execution_sdk::{durable_execution, DurableError, Duration};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowProgress {
    pub stage: String,
    pub completed_waits: u32,
}

/// Execute a workflow with multiple wait stages.
///
/// This demonstrates how waits can be used to create
/// staged workflows with delays between stages.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<WorkflowProgress, DurableError> {
    // Stage 1: Initial processing
    let _stage1: String = ctx
        .step_named(
            "stage_1_process",
            |_| Ok("stage 1 complete".to_string()),
            None,
        )
        .await?;

    // Wait before stage 2
    ctx.wait(Duration::from_seconds(5), Some("wait_before_stage_2"))
        .await?;

    // Stage 2: Secondary processing
    let _stage2: String = ctx
        .step_named(
            "stage_2_process",
            |_| Ok("stage 2 complete".to_string()),
            None,
        )
        .await?;

    // Wait before stage 3
    ctx.wait(Duration::from_seconds(10), Some("wait_before_stage_3"))
        .await?;

    // Stage 3: Final processing
    let _stage3: String = ctx
        .step_named(
            "stage_3_process",
            |_| Ok("stage 3 complete".to_string()),
            None,
        )
        .await?;

    // Final wait before completion
    ctx.wait(Duration::from_seconds(2), Some("final_cooldown"))
        .await?;

    Ok(WorkflowProgress {
        stage: "completed".to_string(),
        completed_waits: 3,
    })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
