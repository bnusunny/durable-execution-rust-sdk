# Tracing Best Practices for Durable Executions

This document explains how to configure and use tracing effectively with the AWS Durable Execution SDK for Rust. Proper tracing setup enables you to monitor, debug, and correlate logs across your durable workflows.

## Table of Contents

- [Configuring Tracing for Lambda](#configuring-tracing-for-lambda)
- [Log Correlation Across Operations](#log-correlation-across-operations)
- [Structured Fields for Filtering](#structured-fields-for-filtering)
- [Replay-Aware Logging](#replay-aware-logging)
- [Common Tracing Patterns](#common-tracing-patterns)

## Configuring Tracing for Lambda

### Basic Setup

The SDK uses the `tracing` crate for structured logging. The Lambda Rust Runtime provides a built-in default subscriber with sensible options for AWS CloudWatch.

### Using the Lambda Runtime's Default Subscriber (Recommended)

The simplest way to enable tracing is to use the Lambda runtime's built-in subscriber:

```rust
use lambda_runtime::{run, service_fn, tracing, Error};
use aws_durable_execution_sdk::{durable_execution, DurableContext, DurableError};

#[tokio::main]
async fn main() -> Result<(), Error> {
    // Initialize the default subscriber - this is all you need!
    tracing::init_default_subscriber();
    
    run(service_fn(handler)).await
}

#[durable_execution]
async fn handler(event: MyEvent, ctx: DurableContext) -> Result<MyResult, DurableError> {
    ctx.log_info("Processing event");
    // ... workflow logic ...
    Ok(MyResult { status: "completed".to_string() })
}
```

The default subscriber:
- Sends logging information to AWS CloudWatch
- Uses `RUST_LOG` environment variable to determine log level
- Integrates with Lambda's advanced logging controls (if configured)
- Defaults to INFO level

### Custom Tracing Configuration

If you need more control over the tracing configuration, you can set up a custom subscriber:

```toml
[dependencies]
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
```

```rust
use tracing_subscriber::{fmt, EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

fn init_custom_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::layer()
                .json()
                .with_ansi(false)  // Disable ANSI colors for CloudWatch
                .with_target(true)
                .with_current_span(true)
                .flatten_event(true)
        )
        .init();
}
```

### Environment Variables

Control log levels using the `RUST_LOG` environment variable in your Lambda configuration:

```bash
# Show all logs at info level (default)
RUST_LOG=info

# Show debug logs for your workflow, info for SDK
RUST_LOG=my_workflow=debug,aws_durable_execution_sdk=info

# Show only warnings and errors
RUST_LOG=warn

# Show trace level for detailed debugging (includes raw payload dump)
RUST_LOG=trace
```

## Log Correlation Across Operations

### Automatic Context Fields

The SDK automatically includes correlation fields in all log messages:

| Field | Description |
|-------|-------------|
| `durable_execution_arn` | Unique identifier for the entire workflow execution |
| `operation_id` | Unique identifier for the current operation |
| `parent_id` | Parent operation ID for nested operations |
| `is_replay` | Whether the operation is being replayed |

### Using the Simplified Logging API

The `DurableContext` provides convenience methods that automatically include context:

```rust
use aws_durable_execution_sdk::{DurableContext, DurableError};

async fn my_workflow(ctx: DurableContext) -> Result<(), DurableError> {
    // Basic logging - context is automatically included
    ctx.log_info("Starting order processing");
    ctx.log_debug("Validating input parameters");
    
    // Logging with extra fields
    ctx.log_info_with("Processing order", &[
        ("order_id", "ORD-12345"),
        ("customer_id", "CUST-789"),
    ]);
    
    // Warning and error logging
    ctx.log_warn("Retry attempt 2 of 5");
    ctx.log_error_with("Payment failed", &[
        ("error_code", "INSUFFICIENT_FUNDS"),
        ("amount", "150.00"),
    ]);
    
    Ok(())
}
```

### Tracing Spans for Operations

The SDK creates tracing spans for each durable operation. These spans include:

- `operation_type`: The type of operation (step, wait, callback, invoke, map, parallel)
- `operation_id`: The unique operation identifier
- `parent_id`: The parent operation ID (for nested operations)
- `name`: The operation name (if provided)
- `durable_execution_arn`: The execution ARN

Example log output in JSON format:

```json
{
  "timestamp": "2025-01-14T10:30:00.000Z",
  "level": "INFO",
  "message": "Processing order",
  "durable_execution_arn": "arn:aws:lambda:us-east-1:123456789012:function:my-func:durable:abc123",
  "operation_id": "a1b2c3d4e5f6",
  "parent_id": null,
  "is_replay": false,
  "extra": "order_id=ORD-12345, customer_id=CUST-789"
}
```

### Correlating Logs in CloudWatch

Use CloudWatch Logs Insights to query logs by execution:

```sql
-- Find all logs for a specific execution
fields @timestamp, @message, operation_id, is_replay
| filter durable_execution_arn = "arn:aws:lambda:us-east-1:123456789012:function:my-func:durable:abc123"
| sort @timestamp asc

-- Find all errors in a time range
fields @timestamp, @message, durable_execution_arn, operation_id
| filter level = "ERROR"
| sort @timestamp desc
| limit 100

-- Trace a specific operation and its children
fields @timestamp, @message, operation_id, parent_id
| filter durable_execution_arn = "arn:aws:lambda:us-east-1:123456789012:function:my-func:durable:abc123"
| filter operation_id = "a1b2c3d4e5f6" or parent_id = "a1b2c3d4e5f6"
| sort @timestamp asc
```

## Structured Fields for Filtering

### Adding Custom Fields

Use the `log_*_with` methods to add custom fields for filtering:

```rust
async fn process_payment(ctx: &DurableContext, payment: &Payment) -> Result<(), DurableError> {
    ctx.log_info_with("Processing payment", &[
        ("payment_id", &payment.id),
        ("amount", &payment.amount.to_string()),
        ("currency", &payment.currency),
        ("merchant_id", &payment.merchant_id),
    ]);
    
    // ... payment processing logic ...
    
    Ok(())
}
```

### Using LogInfo Directly

For more control, create `LogInfo` directly:

```rust
use aws_durable_execution_sdk::{LogInfo, Logger};

fn log_with_custom_context(ctx: &DurableContext, message: &str) {
    let log_info = ctx.create_log_info()
        .with_extra("custom_field", "custom_value")
        .with_extra("request_id", "req-123");
    
    ctx.logger().info(message, &log_info);
}
```

### Queryable Field Patterns

Design your extra fields for easy querying:

```rust
// Good: Consistent field names across your application
ctx.log_info_with("Order event", &[
    ("event_type", "ORDER_CREATED"),
    ("order_id", &order.id),
    ("status", "pending"),
]);

ctx.log_info_with("Order event", &[
    ("event_type", "ORDER_COMPLETED"),
    ("order_id", &order.id),
    ("status", "completed"),
]);

// Query in CloudWatch:
// filter extra like /event_type=ORDER_CREATED/
// filter extra like /order_id=ORD-123/
```

## Replay-Aware Logging

### Understanding Replay

When a durable execution resumes after an interruption, the SDK replays completed operations by returning their checkpointed results. During replay:

- Operations don't re-execute their logic
- The `is_replay` flag is set to `true` in log messages
- You may want to suppress or filter replay logs to reduce noise

### Configuring Replay-Aware Logging

Use `ReplayAwareLogger` to control logging behavior during replay:

```rust
use aws_durable_execution_sdk::{
    DurableContext, TracingLogger, ReplayAwareLogger, ReplayLoggingConfig
};
use std::sync::Arc;

// Option 1: Suppress all logs during replay (default)
let logger = ReplayAwareLogger::suppress_replay(Arc::new(TracingLogger));

// Option 2: Allow only errors during replay
let logger = ReplayAwareLogger::new(
    Arc::new(TracingLogger),
    ReplayLoggingConfig::ErrorsOnly,
);

// Option 3: Allow errors and warnings during replay
let logger = ReplayAwareLogger::new(
    Arc::new(TracingLogger),
    ReplayLoggingConfig::WarningsAndErrors,
);

// Option 4: Allow all logs during replay
let logger = ReplayAwareLogger::allow_all(Arc::new(TracingLogger));

// Apply to context
let ctx = DurableContext::new(state).with_logger(Arc::new(logger));
```

### Replay Logging Configuration Options

| Configuration | Behavior |
|--------------|----------|
| `SuppressAll` | No logs during replay (default) |
| `AllowAll` | All logs emitted during replay |
| `ErrorsOnly` | Only ERROR level logs during replay |
| `WarningsAndErrors` | WARN and ERROR logs during replay |

### Distinguishing Replay Logs

When using `AllowAll`, you can filter replay logs in CloudWatch:

```sql
-- Show only fresh execution logs (not replay)
fields @timestamp, @message
| filter is_replay = false

-- Show only replay logs
fields @timestamp, @message
| filter is_replay = true

-- Compare fresh vs replay for debugging
fields @timestamp, @message, is_replay, operation_id
| filter durable_execution_arn = "arn:aws:..."
| sort @timestamp asc
```

## Common Tracing Patterns

### Pattern 1: Workflow Progress Tracking

Track workflow progress through major milestones:

```rust
async fn order_workflow(ctx: DurableContext, order: Order) -> Result<OrderResult, DurableError> {
    ctx.log_info_with("Workflow started", &[
        ("order_id", &order.id),
        ("stage", "STARTED"),
    ]);
    
    // Validation step
    let is_valid = ctx.step_named("validate", |_| {
        // validation logic
        Ok(true)
    }, None).await?;
    
    ctx.log_info_with("Validation complete", &[
        ("order_id", &order.id),
        ("stage", "VALIDATED"),
        ("is_valid", &is_valid.to_string()),
    ]);
    
    // Payment step
    let payment_id = ctx.step_named("process_payment", |_| {
        // payment logic
        Ok("pay_123".to_string())
    }, None).await?;
    
    ctx.log_info_with("Payment processed", &[
        ("order_id", &order.id),
        ("stage", "PAYMENT_COMPLETE"),
        ("payment_id", &payment_id),
    ]);
    
    // Fulfillment
    ctx.step_named("fulfill", |_| {
        // fulfillment logic
        Ok(())
    }, None).await?;
    
    ctx.log_info_with("Workflow completed", &[
        ("order_id", &order.id),
        ("stage", "COMPLETED"),
    ]);
    
    Ok(OrderResult { order_id: order.id, status: "completed".to_string() })
}
```

### Pattern 2: Error Context Enrichment

Add context when errors occur:

```rust
async fn risky_operation(ctx: &DurableContext, input: &str) -> Result<String, DurableError> {
    match perform_operation(input).await {
        Ok(result) => {
            ctx.log_debug_with("Operation succeeded", &[
                ("input", input),
                ("result_size", &result.len().to_string()),
            ]);
            Ok(result)
        }
        Err(e) => {
            ctx.log_error_with("Operation failed", &[
                ("input", input),
                ("error", &e.to_string()),
                ("error_type", "OPERATION_FAILURE"),
            ]);
            Err(DurableError::execution(format!("Operation failed: {}", e)))
        }
    }
}
```

### Pattern 3: Parallel Operation Tracing

Track parallel operations with indices:

```rust
async fn parallel_processing(ctx: DurableContext, items: Vec<Item>) -> Result<Vec<Result>, DurableError> {
    ctx.log_info_with("Starting parallel processing", &[
        ("item_count", &items.len().to_string()),
    ]);
    
    let results = ctx.map(
        items,
        |child_ctx, item, index| async move {
            child_ctx.log_debug_with("Processing item", &[
                ("index", &index.to_string()),
                ("item_id", &item.id),
            ]);
            
            let result = child_ctx.step(|_| {
                // process item
                Ok(item.id.clone())
            }, None).await?;
            
            child_ctx.log_debug_with("Item processed", &[
                ("index", &index.to_string()),
                ("item_id", &item.id),
            ]);
            
            Ok(result)
        },
        None,
    ).await?;
    
    let success_count = results.iter().filter(|r| r.is_ok()).count();
    ctx.log_info_with("Parallel processing complete", &[
        ("total", &items.len().to_string()),
        ("succeeded", &success_count.to_string()),
        ("failed", &(items.len() - success_count).to_string()),
    ]);
    
    Ok(results)
}
```

### Pattern 4: Custom Logger for External Systems

Send logs to external systems alongside CloudWatch:

```rust
use aws_durable_execution_sdk::{custom_logger, LogInfo, Logger};
use std::sync::Arc;

fn create_dual_logger() -> Arc<dyn Logger> {
    Arc::new(custom_logger(
        |msg, info| {
            // Log to tracing (CloudWatch)
            tracing::debug!(
                durable_execution_arn = ?info.durable_execution_arn,
                operation_id = ?info.operation_id,
                "{}",
                msg
            );
            // Also send to external system
            send_to_external_logging(msg, info, "DEBUG");
        },
        |msg, info| {
            tracing::info!(
                durable_execution_arn = ?info.durable_execution_arn,
                operation_id = ?info.operation_id,
                "{}",
                msg
            );
            send_to_external_logging(msg, info, "INFO");
        },
        |msg, info| {
            tracing::warn!(
                durable_execution_arn = ?info.durable_execution_arn,
                operation_id = ?info.operation_id,
                "{}",
                msg
            );
            send_to_external_logging(msg, info, "WARN");
        },
        |msg, info| {
            tracing::error!(
                durable_execution_arn = ?info.durable_execution_arn,
                operation_id = ?info.operation_id,
                "{}",
                msg
            );
            send_to_external_logging(msg, info, "ERROR");
        },
    ))
}

fn send_to_external_logging(msg: &str, info: &LogInfo, level: &str) {
    // Send to Datadog, Splunk, etc.
}
```

### Pattern 5: Callback Tracing

Track external callback workflows:

```rust
use aws_durable_execution_sdk::{DurableContext, DurableError, CallbackConfig, Duration};

async fn approval_workflow(ctx: DurableContext, request: ApprovalRequest) -> Result<bool, DurableError> {
    ctx.log_info_with("Starting approval workflow", &[
        ("request_id", &request.id),
        ("approver", &request.approver_email),
    ]);
    
    // Create callback and notify approver
    let approval: ApprovalResponse = ctx.wait_for_callback(
        |callback_id| async move {
            ctx.log_info_with("Callback created, notifying approver", &[
                ("callback_id", &callback_id),
                ("approver", &request.approver_email),
            ]);
            
            // Send notification to approver with callback_id
            notify_approver(&request.approver_email, &callback_id).await?;
            
            ctx.log_debug_with("Approver notified", &[
                ("callback_id", &callback_id),
            ]);
            
            Ok(())
        },
        Some(CallbackConfig {
            timeout: Duration::from_hours(24),
            ..Default::default()
        }),
    ).await?;
    
    ctx.log_info_with("Approval received", &[
        ("request_id", &request.id),
        ("approved", &approval.approved.to_string()),
        ("approver", &approval.approver),
    ]);
    
    Ok(approval.approved)
}
```

## Summary

| Best Practice | Description |
|--------------|-------------|
| Use JSON format | Enables structured querying in CloudWatch |
| Include correlation fields | Use `durable_execution_arn` and `operation_id` for tracing |
| Use simplified logging API | `log_info`, `log_debug`, etc. for automatic context |
| Configure replay logging | Suppress or filter logs during replay to reduce noise |
| Add custom fields | Use `log_*_with` for queryable business context |
| Track workflow stages | Log at major milestones for progress visibility |
| Enrich error context | Include relevant context when logging errors |
