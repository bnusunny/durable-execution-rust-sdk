//! Wait-for-Callback Multiple Invocations Example
//!
//! This example demonstrates making multiple sequential `wait_for_callback` calls
//! within a single handler execution. Each callback creates an independent external
//! wait point, and the handler chains them one after another.
//!
//! # Key Concepts
//!
//! - Multiple `wait_for_callback` calls in a single handler
//! - Each callback gets its own unique callback ID
//! - Callbacks are awaited sequentially — the second waits until the first completes
//!
//! # Running this example
//!
//! ```bash
//! cargo build --example wfc_multiple
//! ```

use durable_execution_sdk::{durable_execution, DurableError};
use serde::{Deserialize, Serialize};

/// Response received from an external system via callback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackResponse {
    pub status: String,
    pub message: String,
}

/// Combined result from multiple sequential callbacks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultipleCallbackResult {
    pub first: CallbackResponse,
    pub second: CallbackResponse,
}

/// Demonstrates multiple sequential wait_for_callback calls in a single handler.
///
/// The handler calls `wait_for_callback` twice. Each call suspends execution
/// until the corresponding external system responds. The second callback is
/// only initiated after the first one completes.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<MultipleCallbackResult, DurableError> {
    // First wait_for_callback
    let first: CallbackResponse = ctx
        .wait_for_callback(
            move |callback_id| async move {
                println!("First callback ID: {}", callback_id);
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            },
            None,
        )
        .await?;

    // Second wait_for_callback — only reached after the first completes
    let second: CallbackResponse = ctx
        .wait_for_callback(
            move |callback_id| async move {
                println!("Second callback ID: {}", callback_id);
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            },
            None,
        )
        .await?;

    Ok(MultipleCallbackResult { first, second })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
