//! Custom Logger Example
//!
//! Using custom logging with the durable execution SDK.

use aws_durable_execution_sdk::{
    durable_execution, DurableError,
    custom_logger, simple_custom_logger,
    TracingLogger, ReplayAwareLogger, ReplayLoggingConfig,
};
use std::sync::Arc;

/// Demonstrate custom logger creation.
///
/// The SDK provides factory functions for creating custom loggers
/// since the Logger trait is sealed.
pub fn create_custom_loggers() {
    // Simple custom logger with single function for all levels
    let _simple_logger = simple_custom_logger(|level, msg, _info| {
        println!("[{}] {}", level, msg);
    });

    // Full custom logger with separate functions per level
    let _full_logger = custom_logger(
        |msg, _info| println!("[DEBUG] {}", msg),
        |msg, _info| println!("[INFO] {}", msg),
        |msg, _info| println!("[WARN] {}", msg),
        |msg, _info| println!("[ERROR] {}", msg),
    );

    // Replay-aware logger that suppresses logs during replay
    let _replay_aware = ReplayAwareLogger::new(
        Arc::new(TracingLogger),
        ReplayLoggingConfig::SuppressAll,
    );

    // Replay-aware logger that only shows errors during replay
    let _errors_only = ReplayAwareLogger::new(
        Arc::new(TracingLogger),
        ReplayLoggingConfig::ErrorsOnly,
    );
}

/// Handler demonstrating logging.
#[durable_execution]
pub async fn handler(_event: serde_json::Value, ctx: DurableContext) -> Result<String, DurableError> {
    // Use context logging methods
    ctx.log_info("Starting workflow");
    
    let result: String = ctx
        .step_named("process", |_| {
            Ok("processed".to_string())
        }, None)
        .await?;

    ctx.log_info("Workflow completed");

    Ok(result)
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
