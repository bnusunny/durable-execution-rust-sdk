//! First Successful Parallel Example
//!
//! Complete when the first branch succeeds using `CompletionConfig`.

use aws_durable_execution_sdk::{
    durable_execution, CompletionConfig, DurableContext, DurableError, MapConfig,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataResult {
    pub source: String,
    pub data: String,
}

/// Try multiple data sources and use the first successful result.
///
/// This pattern is useful for redundant operations where you
/// want the fastest successful response.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<DataResult, DurableError> {
    let sources: Vec<String> = vec![
        "primary".to_string(),
        "secondary".to_string(),
        "cache".to_string(),
    ];

    let results = ctx
        .map(
            sources,
            |child_ctx: DurableContext, source: String, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step_named(
                            &format!("{}_db", source),
                            |_| {
                                Ok(DataResult {
                                    source: source.clone(),
                                    data: format!("data_from_{}", source),
                                })
                            },
                            None,
                        )
                        .await
                })
            },
            Some(MapConfig {
                completion_config: CompletionConfig::first_successful(),
                ..Default::default()
            }),
        )
        .await?;

    // Get the first successful result
    let first_result = results
        .succeeded()
        .first()
        .and_then(|item| item.result.clone())
        .ok_or_else(|| DurableError::execution("All data sources failed"))?;

    Ok(first_result)
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
