//! Concurrent Callback Wait Example
//!
//! Demonstrates a `ctx.create_callback()` and a `ctx.step()` running concurrently
//! via `tokio::join!`. This shows how callback operations can be composed with
//! other durable operations in parallel.

use durable_execution_sdk::{durable_execution, CallbackConfig, DurableError, Duration};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackWaitResult {
    pub step_result: String,
    pub callback_status: String,
}

/// Execute a callback creation and a step concurrently using `tokio::join!`.
///
/// One branch creates a callback and waits for its result, while the other
/// branch executes a step. Since the callback suspends waiting for an external
/// response, the execution will be in Running status.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<CallbackWaitResult, DurableError> {
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();

    let config = CallbackConfig {
        timeout: Duration::from_minutes(5),
        ..Default::default()
    };

    let (step_result, callback_result) = tokio::join!(
        ctx1.step(|_| async move { Ok("step_done".to_string()) }, None),
        async {
            let cb = ctx2
                .create_callback_named::<String>("concurrent_cb", Some(config))
                .await?;
            println!("Callback ID: {}", cb.callback_id);
            cb.result().await
        },
    );

    Ok(CallbackWaitResult {
        step_result: step_result?,
        callback_status: callback_result?,
    })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
