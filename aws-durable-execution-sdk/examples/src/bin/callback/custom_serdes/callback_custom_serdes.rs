//! Callback Custom SerDes Example
//!
//! Demonstrates using `create_callback_named` with a custom `SerDesAny`
//! implementation passed via `CallbackConfig { serdes: Some(...) }`.
//! This is similar to the `wfc_custom_serdes` example but uses the manual
//! `create_callback` pattern instead of `wait_for_callback`.
//!
//! # Key Concepts
//!
//! - Implementing the `SerDesAny` trait for custom serialization
//! - Passing a custom `SerDesAny` via `CallbackConfig::serdes`
//! - Using `create_callback_named` with a typed callback payload
//!
//! # Running this example
//!
//! ```bash
//! cargo build --example callback_custom_serdes
//! ```

use durable_execution_sdk::{durable_execution, CallbackConfig, DurableError, SerDesAny};
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

/// Demonstrates create_callback_named with a custom `SerDesAny` implementation.
///
/// The handler creates a `CallbackConfig` with a custom serializer/deserializer
/// and passes it to `create_callback_named`. The callback ID is printed,
/// simulating notification of an external system, then the handler waits
/// for the callback result.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<CustomPayload, DurableError> {
    let config = CallbackConfig {
        serdes: Some(Arc::new(CustomPayloadSerDes)),
        ..Default::default()
    };

    let callback = ctx
        .create_callback_named::<CustomPayload>("custom_serdes_callback", Some(config))
        .await?;

    println!("Callback ID (custom serdes): {}", callback.callback_id);

    let result = callback.result().await?;

    Ok(result)
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
