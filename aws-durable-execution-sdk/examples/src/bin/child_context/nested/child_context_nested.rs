//! Nested Child Context Example
//!
//! Creating nested child contexts for complex workflow hierarchies.

use aws_durable_execution_sdk::{durable_execution, DurableError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NestedResult {
    pub level: u32,
    pub value: String,
}

/// Create nested child contexts for hierarchical workflows.
///
/// Child contexts can contain other child contexts, allowing
/// for complex workflow organization.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<Vec<NestedResult>, DurableError> {
    let mut results = Vec::new();

    // Level 1 child context
    let level1_result = ctx
        .run_in_child_context_named(
            "level_1",
            |child_ctx| {
                Box::pin(async move {
                    let result: NestedResult = child_ctx
                        .step(
                            |_| {
                                Ok(NestedResult {
                                    level: 1,
                                    value: "level 1 complete".to_string(),
                                })
                            },
                            None,
                        )
                        .await?;

                    // Level 2 nested child context
                    let level2_result = child_ctx
                        .run_in_child_context_named(
                            "level_2",
                            |nested_ctx| {
                                Box::pin(async move {
                                    nested_ctx
                                        .step(
                                            |_| {
                                                Ok(NestedResult {
                                                    level: 2,
                                                    value: "level 2 complete".to_string(),
                                                })
                                            },
                                            None,
                                        )
                                        .await
                                })
                            },
                            None,
                        )
                        .await?;

                    Ok((result, level2_result))
                })
            },
            None,
        )
        .await?;

    results.push(level1_result.0);
    results.push(level1_result.1);

    Ok(results)
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
