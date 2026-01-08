//! Simple Step-Based Workflow Example
//!
//! This example demonstrates a basic order processing workflow using the
//! AWS Durable Execution SDK. It shows how to:
//!
//! - Use the `#[durable_execution]` macro to create a durable Lambda handler
//! - Execute checkpointed steps with `ctx.step()`
//! - Use wait operations to pause execution with `ctx.wait()`
//! - Handle errors appropriately
//!
//! # Running this example
//!
//! This example is designed to run as an AWS Lambda function. To deploy:
//!
//! 1. Build with `cargo lambda build --release --example simple_workflow`
//! 2. Deploy to AWS Lambda with durable execution enabled
//! 3. Invoke with a JSON payload matching the `OrderEvent` structure
//!
//! # Workflow Steps
//!
//! 1. **Validate Order**: Checks if the order is valid
//! 2. **Process Payment**: Simulates payment processing
//! 3. **Wait for Confirmation**: Pauses for payment confirmation
//! 4. **Fulfill Order**: Completes the order fulfillment
//!
//! Each step is automatically checkpointed. If the Lambda function is
//! interrupted at any point, it will resume from the last completed step.

use aws_durable_execution_sdk::{
    durable_execution, Duration, DurableError, TerminationReason,
};
use serde::{Deserialize, Serialize};

/// Input event for the order processing workflow.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrderEvent {
    /// Unique identifier for the order
    pub order_id: String,
    /// Customer identifier
    pub customer_id: String,
    /// Order amount in cents
    pub amount_cents: u64,
    /// Items in the order (can be a single item or a list)
    #[serde(deserialize_with = "deserialize_items")]
    pub items: Vec<OrderItem>,
}

/// Custom deserializer that accepts either a single OrderItem or a Vec<OrderItem>
fn deserialize_items<'de, D>(deserializer: D) -> Result<Vec<OrderItem>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, SeqAccess, Visitor};
    
    struct ItemsVisitor;
    
    impl<'de> Visitor<'de> for ItemsVisitor {
        type Value = Vec<OrderItem>;
        
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a single OrderItem or a sequence of OrderItems")
        }
        
        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut items = Vec::new();
            while let Some(item) = seq.next_element()? {
                items.push(item);
            }
            Ok(items)
        }
        
        fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
        where
            M: de::MapAccess<'de>,
        {
            // Single item as an object
            let item = OrderItem::deserialize(de::value::MapAccessDeserializer::new(map))?;
            Ok(vec![item])
        }
    }
    
    deserializer.deserialize_any(ItemsVisitor)
}

/// An item in the order.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrderItem {
    /// Product SKU
    pub sku: String,
    /// Quantity ordered
    pub quantity: u32,
    /// Price per unit in cents
    pub unit_price_cents: u64,
}

/// Result of order validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ValidationResult {
    is_valid: bool,
    order_id: String,
    validation_errors: Vec<String>,
}

/// Result of payment processing.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PaymentResult {
    transaction_id: String,
    status: String,
    amount_charged_cents: u64,
}

/// Result of order fulfillment.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FulfillmentResult {
    tracking_number: String,
    estimated_delivery: String,
    warehouse_id: String,
}

/// Final result of the order processing workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderResult {
    /// Order status
    pub status: String,
    /// Order ID
    pub order_id: String,
    /// Payment transaction ID
    pub transaction_id: String,
    /// Shipping tracking number
    pub tracking_number: String,
    /// Estimated delivery date
    pub estimated_delivery: String,
}

/// Main order processing workflow.
///
/// This function is decorated with `#[durable_execution]` which transforms it
/// into a Lambda handler that integrates with AWS Lambda's durable execution service.
#[durable_execution]
pub async fn process_order(
    event: OrderEvent,
    ctx: DurableContext,
) -> Result<OrderResult, DurableError> {
    // Capture order details for use in steps
    let order_id = event.order_id.clone();
    let amount = event.amount_cents;

    // =========================================================================
    // Step 1: Validate the order
    // =========================================================================
    // This step is automatically checkpointed. If the Lambda restarts after
    // this step completes, the validation result will be returned from the
    // checkpoint without re-executing the validation logic.
    let validation: ValidationResult = ctx
        .step_named("validate_order", |_step_ctx| {
            // In a real application, this would validate:
            // - Customer exists and is in good standing
            // - Items are in stock
            // - Prices are correct
            // - Shipping address is valid
            
            let mut errors = Vec::new();
            
            if event.items.is_empty() {
                errors.push("Order must contain at least one item".to_string());
            }
            
            if event.amount_cents == 0 {
                errors.push("Order amount must be greater than zero".to_string());
            }
            
            Ok(ValidationResult {
                is_valid: errors.is_empty(),
                order_id: event.order_id.clone(),
                validation_errors: errors,
            })
        }, None)
        .await?;

    // Check validation result
    if !validation.is_valid {
        return Err(DurableError::Execution {
            message: format!(
                "Order validation failed: {}",
                validation.validation_errors.join(", ")
            ),
            termination_reason: TerminationReason::ExecutionError,
        });
    }

    // =========================================================================
    // Step 2: Process payment
    // =========================================================================
    // Payment processing is a critical step. The checkpoint ensures that
    // even if Lambda restarts, we won't charge the customer twice.
    let payment: PaymentResult = ctx
        .step_named("process_payment", |_step_ctx| {
            // In a real application, this would:
            // - Call a payment gateway API
            // - Handle payment failures and retries
            // - Store payment confirmation
            
            // Simulated payment processing
            Ok(PaymentResult {
                transaction_id: format!("txn_{}", uuid_v4_mock()),
                status: "completed".to_string(),
                amount_charged_cents: amount,
            })
        }, None)
        .await?;

    // =========================================================================
    // Step 3: Wait for payment confirmation
    // =========================================================================
    // This wait operation suspends the Lambda execution. The function will
    // be re-invoked after 5 seconds, and execution will resume from here.
    // This is useful for:
    // - Waiting for external payment confirmation
    // - Rate limiting API calls
    // - Implementing delays in workflows
    ctx.wait(Duration::from_seconds(5), Some("payment_confirmation"))
        .await?;

    // =========================================================================
    // Step 4: Fulfill the order
    // =========================================================================
    // After payment is confirmed, we proceed with fulfillment.
    let fulfillment: FulfillmentResult = ctx
        .step_named("fulfill_order", |_step_ctx| {
            // In a real application, this would:
            // - Reserve inventory
            // - Create shipping label
            // - Notify warehouse
            // - Send confirmation email
            
            Ok(FulfillmentResult {
                tracking_number: format!("TRK{}", uuid_v4_mock()),
                estimated_delivery: "2024-01-15".to_string(),
                warehouse_id: "WH-001".to_string(),
            })
        }, None)
        .await?;

    // =========================================================================
    // Return final result
    // =========================================================================
    Ok(OrderResult {
        status: "completed".to_string(),
        order_id,
        transaction_id: payment.transaction_id,
        tracking_number: fulfillment.tracking_number,
        estimated_delivery: fulfillment.estimated_delivery,
    })
}

/// Mock UUID generator for example purposes.
/// In production, use the `uuid` crate.
fn uuid_v4_mock() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:032x}", timestamp)
}

// Note: In a real Lambda deployment, you would use lambda_runtime::run()
// The #[durable_execution] macro generates the necessary handler code.


/// Main function for the Lambda runtime.
///
/// This is the entry point when running as a Lambda function.
/// The `#[durable_execution]` macro generates the actual handler.
#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize tracing for structured logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_target(false)
        .init();

    // Run the Lambda handler
    lambda_runtime::run(lambda_runtime::service_fn(process_order)).await
}
