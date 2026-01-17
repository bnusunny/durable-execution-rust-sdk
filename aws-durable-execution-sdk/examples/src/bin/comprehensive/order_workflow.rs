//! Comprehensive Order Workflow Example
//!
//! A complete order processing workflow demonstrating multiple
//! durable operations working together.

use aws_durable_execution_sdk::{
    durable_execution, CallbackConfig, CompletionConfig, DurableContext, DurableError, Duration,
    MapConfig,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrderEvent {
    pub order_id: String,
    pub customer_id: String,
    pub items: Vec<OrderItem>,
    pub requires_approval: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrderItem {
    pub sku: String,
    pub quantity: u32,
    pub price_cents: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderResult {
    pub order_id: String,
    pub status: String,
    pub total_cents: u64,
    pub items_processed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResponse {
    pub approved: bool,
    pub approver: String,
}

/// Comprehensive order processing workflow.
///
/// This example demonstrates:
/// - Steps for validation and processing
/// - Child contexts for service checks
/// - Map operations for processing items
/// - Callbacks for approval workflows
/// - Wait operations for delays
#[durable_execution]
pub async fn handler(event: OrderEvent, ctx: DurableContext) -> Result<OrderResult, DurableError> {
    let order_id = event.order_id.clone();

    // Step 1: Validate order
    let is_valid: bool = ctx
        .step_named("validate_order", |_| Ok(!event.items.is_empty()), None)
        .await?;

    if !is_valid {
        return Err(DurableError::execution("Order validation failed: no items"));
    }

    // Step 2: Check services using child contexts
    let _inventory_check: String = ctx
        .run_in_child_context_named(
            "check_inventory",
            |child_ctx| {
                Box::pin(async move {
                    child_ctx
                        .step(|_| Ok("inventory_ok".to_string()), None)
                        .await
                })
            },
            None,
        )
        .await?;

    let _customer_check: String = ctx
        .run_in_child_context_named(
            "check_customer",
            |child_ctx| {
                Box::pin(async move {
                    child_ctx
                        .step(|_| Ok("customer_ok".to_string()), None)
                        .await
                })
            },
            None,
        )
        .await?;

    // Step 3: Process items with map
    let processed_items = ctx
        .map(
            event.items.clone(),
            |child_ctx: DurableContext, item: OrderItem, index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step_named(
                            &format!("process_item_{}", index),
                            |_| Ok(item.price_cents * item.quantity as u64),
                            None,
                        )
                        .await
                })
            },
            Some(MapConfig {
                max_concurrency: Some(5),
                completion_config: CompletionConfig::all_successful(),
                ..Default::default()
            }),
        )
        .await?;

    let item_totals = processed_items.get_results()?;
    let total_cents: u64 = item_totals.iter().copied().sum();

    // Step 4: Approval workflow (if required)
    if event.requires_approval {
        let callback = ctx
            .create_callback_named::<ApprovalResponse>(
                "order_approval",
                Some(CallbackConfig {
                    timeout: Duration::from_hours(24),
                    ..Default::default()
                }),
            )
            .await?;

        // In production, send callback_id to approval system
        println!("Approval callback: {}", callback.callback_id);

        let approval = callback.result().await?;

        if !approval.approved {
            return Ok(OrderResult {
                order_id,
                status: "rejected".to_string(),
                total_cents,
                items_processed: event.items.len(),
            });
        }
    }

    // Step 5: Wait for processing delay
    ctx.wait(Duration::from_seconds(2), Some("processing_delay"))
        .await?;

    // Step 6: Finalize order
    let _finalized: bool = ctx.step_named("finalize_order", |_| Ok(true), None).await?;

    Ok(OrderResult {
        order_id,
        status: "completed".to_string(),
        total_cents,
        items_processed: event.items.len(),
    })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
