//! Custom Serialization Example
//!
//! Using custom serialization for step results.

use aws_durable_execution_sdk::{
    durable_execution, DurableError,
    serdes::{custom_serdes, SerDesError},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensitiveData {
    pub user_id: String,
    pub data: String,
}

/// Create a custom serializer that masks sensitive data.
pub fn create_masked_serdes() -> impl aws_durable_execution_sdk::serdes::SerDes<SensitiveData> {
    custom_serdes::<SensitiveData, _, _>(
        |value, _ctx| {
            // Custom serialization - could encrypt or mask data
            serde_json::to_string(value)
                .map_err(|e| SerDesError::serialization(e.to_string()))
        },
        |data, _ctx| {
            // Custom deserialization
            serde_json::from_str(data)
                .map_err(|e| SerDesError::deserialization(e.to_string()))
        },
    )
}

/// Handler demonstrating custom serialization.
#[durable_execution]
pub async fn handler(_event: serde_json::Value, ctx: DurableContext) -> Result<SensitiveData, DurableError> {
    // Use default JSON serialization
    let data: SensitiveData = ctx
        .step_named("process_data", |_| {
            Ok(SensitiveData {
                user_id: "user-123".to_string(),
                data: "sensitive information".to_string(),
            })
        }, None)
        .await?;

    Ok(data)
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
