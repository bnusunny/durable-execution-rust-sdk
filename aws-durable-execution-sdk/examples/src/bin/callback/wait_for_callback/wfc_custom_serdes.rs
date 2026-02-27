//! Wait-for-Callback with Custom SerDesAny Example
//!
//! This example demonstrates using `wait_for_callback` with a custom
//! `SerDesAny` implementation passed via `CallbackConfig { serdes: ... }`.
//! The custom serializer/deserializer handles a `CustomPayload` type,
//! showing how to implement type-erased serialization for callback payloads.
//!
//! # Key Concepts
//!
//! - Implementing the `SerDesAny` trait for custom serialization
//! - Passing a custom `SerDesAny` via `CallbackConfig::serdes`
//! - Using `wait_for_callback` with a typed callback payload
//!
//! # Running this example
//!
//! ```bash
//! cargo build --example wfc_custom_serdes
//! ```

use aws_durable_execution_sdk::{durable_execution, CallbackConfig, DurableError, SerDesAny};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Custom payload type for the callback response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomPayload {
    pub data: String,
}

/// A custom `SerDesAny` implementation for `CustomPayload`.
///
/// This demonstrates how to implement type-erased serialization by
/// downcasting `dyn Any` to the concrete `CustomPayload` type.
struct CustomPayloadSerDes;

impl SerDesAny for CustomPayloadSerDes {
    fn serialize_any(&self, value: &dyn std::any::Any) -> Result<String, DurableError> {
        let payload = value
            .downcast_ref::<CustomPayload>()
            .ok_or_else(|| DurableError::execution("Expected CustomPayload type"))?;
        serde_json::to_string(payload)
            .map_err(|e| DurableError::execution(format!("Serialization error: {}", e)))
    }

    fn deserialize_any(&self, data: &str) -> Result<Box<dyn std::any::Any + Send>, DurableError> {
        let payload: CustomPayload = serde_json::from_str(data)
            .map_err(|e| DurableError::execution(format!("Deserialization error: {}", e)))?;
        Ok(Box::new(payload))
    }
}

/// Demonstrates wait_for_callback with a custom `SerDesAny` implementation.
///
/// The handler creates a `CallbackConfig` with a custom serializer/deserializer
/// and passes it to `wait_for_callback`. The callback submitter prints the
/// callback ID, simulating notification of an external system.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<CustomPayload, DurableError> {
    let config = CallbackConfig {
        serdes: Some(Arc::new(CustomPayloadSerDes)),
        ..Default::default()
    };

    let response: CustomPayload = ctx
        .wait_for_callback(
            move |callback_id| async move {
                println!("Callback ID (custom serdes): {}", callback_id);
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            },
            Some(config),
        )
        .await?;

    Ok(response)
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
