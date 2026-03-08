//! Context Validation: Parent in Wait Condition Example
//!
//! Demonstrates that using a parent `DurableContext` inside a child context's
//! closure to call `wait_for_condition` produces an error. The parent context
//! should not be used for durable operations within a child context — only the
//! child context parameter should be used.

use durable_execution_sdk::{
    durable_execution, DurableError, Duration, WaitForConditionConfig, WaitForConditionContext,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PollState {
    pub attempt: u32,
}

/// Attempt to use the parent context's wait_for_condition inside a child context.
///
/// This handler clones the parent context and then tries to call
/// `wait_for_condition` on the parent context inside `run_in_child_context`.
/// The SDK detects this misuse and returns an error.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let parent_ctx = ctx.clone();

    let result = ctx
        .run_in_child_context_named(
            "misuse_wait_condition",
            |_child_ctx| {
                let parent = parent_ctx;
                Box::pin(async move {
                    // Misuse: calling wait_for_condition on the PARENT context inside a child context
                    let config = WaitForConditionConfig::from_interval(
                        PollState { attempt: 0 },
                        Duration::from_seconds(1),
                        Some(3),
                    );

                    let val: String = parent
                        .wait_for_condition(
                            |_state: &PollState, wait_ctx: &WaitForConditionContext| {
                                if wait_ctx.attempt >= 1 {
                                    Ok("condition_met".to_string())
                                } else {
                                    Err("not yet".into())
                                }
                            },
                            config,
                        )
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
