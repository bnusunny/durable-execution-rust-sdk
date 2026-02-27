//! Concurrent Callback Submitter Example
//!
//! Demonstrates multiple callback operations with independent submitters
//! running concurrently via `tokio::join!`. Each callback has its own
//! submitter function that notifies a different external system.

use aws_durable_execution_sdk::{durable_execution, CallbackConfig, DurableError, Duration};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiCallbackResult {
    pub callback_a: String,
    pub callback_b: String,
}

/// Execute two wait_for_callback operations concurrently using `tokio::join!`.
///
/// Each callback has its own independent submitter. Both callbacks are created
/// and awaited concurrently. Since callbacks suspend waiting for external
/// responses, the execution will be in Running status.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<MultiCallbackResult, DurableError> {
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();

    let config = CallbackConfig {
        timeout: Duration::from_minutes(10),
        ..Default::default()
    };

    let (result_a, result_b) = tokio::join!(
        ctx1.wait_for_callback(
            move |callback_id| async move {
                println!("Submitter A: callback_id={}", callback_id);
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            },
            Some(config.clone()),
        ),
        ctx2.wait_for_callback(
            move |callback_id| async move {
                println!("Submitter B: callback_id={}", callback_id);
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            },
            Some(config),
        ),
    );

    Ok(MultiCallbackResult {
        callback_a: result_a?,
        callback_b: result_b?,
    })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
