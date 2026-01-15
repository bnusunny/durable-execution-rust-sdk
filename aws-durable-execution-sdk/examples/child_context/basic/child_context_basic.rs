//! Basic Child Context Example
//!
//! Using `ctx.run_in_child_context()` to create isolated nested workflows.

use aws_durable_execution_sdk::{durable_execution, DurableError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubWorkflowResult {
    pub step1: String,
    pub step2: String,
}

/// Run operations in an isolated child context.
///
/// Child contexts have their own operation ID namespace and
/// can be used to organize complex workflows into logical units.
#[durable_execution]
pub async fn handler(_event: serde_json::Value, ctx: DurableContext) -> Result<SubWorkflowResult, DurableError> {
    // Run a sub-workflow in a child context
    let result = ctx
        .run_in_child_context_named("sub_workflow", |child_ctx| {
            Box::pin(async move {
                // Operations in child context are isolated
                let step1: String = child_ctx
                    .step_named("child_step_1", |_| Ok("child step 1 done".to_string()), None)
                    .await?;

                let step2: String = child_ctx
                    .step_named("child_step_2", |_| Ok("child step 2 done".to_string()), None)
                    .await?;

                Ok(SubWorkflowResult { step1, step2 })
            })
        }, None)
        .await?;

    Ok(result)
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
