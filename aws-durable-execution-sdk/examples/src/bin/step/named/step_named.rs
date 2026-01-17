//! Named Step Example
//!
//! Using `ctx.step_named()` to create steps with descriptive names.
//! Named steps are easier to identify in logs and debugging.

use aws_durable_execution_sdk::{durable_execution, DurableError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingResult {
    pub step_name: String,
    pub value: i32,
}

/// Execute named steps for better observability.
///
/// Named steps appear in logs and traces with their given names,
/// making it easier to understand workflow execution.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<Vec<ProcessingResult>, DurableError> {
    // Step with explicit name
    let first: ProcessingResult = ctx
        .step_named(
            "fetch_data",
            |_step_ctx| {
                Ok(ProcessingResult {
                    step_name: "fetch_data".to_string(),
                    value: 42,
                })
            },
            None,
        )
        .await?;

    // Another named step
    let second: ProcessingResult = ctx
        .step_named(
            "process_data",
            |_step_ctx| {
                Ok(ProcessingResult {
                    step_name: "process_data".to_string(),
                    value: 100,
                })
            },
            None,
        )
        .await?;

    // Third named step
    let third: ProcessingResult = ctx
        .step_named(
            "finalize",
            |_step_ctx| {
                Ok(ProcessingResult {
                    step_name: "finalize".to_string(),
                    value: 200,
                })
            },
            None,
        )
        .await?;

    Ok(vec![first, second, third])
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
