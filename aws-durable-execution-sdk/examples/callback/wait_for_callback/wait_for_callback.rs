//! wait_for_callback Example - Simplified Callback Pattern
//!
//! This example demonstrates the `wait_for_callback` method, which combines
//! callback creation with notification in a single, replay-safe operation.
//!
//! # Why use wait_for_callback?
//!
//! The traditional callback pattern requires three separate steps:
//! 1. Create a callback (checkpointed)
//! 2. Notify external system with callback ID (NOT automatically checkpointed!)
//! 3. Wait for the callback result
//!
//! The problem with step 2 is that if Lambda restarts after notification but
//! before the next checkpoint, the notification might be sent again during replay.
//!
//! `wait_for_callback` solves this by wrapping the notification in a child context,
//! ensuring the notification step is also checkpointed and won't be re-executed
//! during replay.
//!
//! # Comparison
//!
//! ## Traditional Pattern (manual callback)
//! ```rust,ignore
//! // Step 1: Create callback
//! let callback = ctx.create_callback::<Response>(None).await?;
//!
//! // Step 2: Notify external system - NOT REPLAY SAFE!
//! // If Lambda restarts here, this notification will be sent again!
//! send_notification(&callback.callback_id).await?;
//!
//! // Step 3: Wait for result
//! let result = callback.result().await?;
//! ```
//!
//! ## Recommended Pattern (wait_for_callback)
//! ```rust,ignore
//! // All-in-one: create callback, notify, and wait - REPLAY SAFE!
//! let result: Response = ctx.wait_for_callback(
//!     |callback_id| async move {
//!         // This notification is checkpointed and won't re-execute on replay
//!         send_notification(&callback_id).await
//!     },
//!     None,
//! ).await?;
//! ```
//!
//! # Running this example
//!
//! ```bash
//! cargo build --example wait_for_callback
//! ```

use aws_durable_execution_sdk::{
    durable_execution, CallbackConfig, Duration, DurableError,
};
use serde::{Deserialize, Serialize};

// =============================================================================
// Types
// =============================================================================

/// Input event for the payment workflow.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PaymentRequest {
    pub payment_id: String,
    pub amount: f64,
    pub currency: String,
    pub customer_email: String,
}

/// Response from the payment processor callback.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PaymentConfirmation {
    pub transaction_id: String,
    pub status: String,
    pub confirmed_amount: f64,
    pub timestamp: String,
}

/// Final result of the payment workflow.
#[derive(Debug, Clone, Serialize)]
pub struct PaymentResult {
    pub payment_id: String,
    pub status: String,
    pub transaction_id: Option<String>,
    pub message: String,
}

// =============================================================================
// Example 1: Basic wait_for_callback Usage
// =============================================================================

/// Basic payment workflow using wait_for_callback.
///
/// This example shows the simplest usage of wait_for_callback where we:
/// 1. Create a callback and notify the payment processor in one replay-safe call
/// 2. Wait for the payment confirmation
/// 3. Process the result
#[durable_execution]
pub async fn basic_payment_workflow(
    event: PaymentRequest,
    ctx: DurableContext,
) -> Result<PaymentResult, DurableError> {
    // Validate the payment request
    let validated = ctx.step_named("validate", |_| {
        if event.amount <= 0.0 {
            return Err("Amount must be positive".into());
        }
        Ok(event.clone())
    }, None).await?;

    // Clone values BEFORE the closure to avoid borrowing issues
    let payment_id_for_closure = validated.payment_id.clone();
    let amount = validated.amount;
    let currency_for_closure = validated.currency.clone();
    
    // Use wait_for_callback to:
    // 1. Create a callback
    // 2. Notify the payment processor (replay-safe!)
    // 3. Wait for the confirmation
    let confirmation: PaymentConfirmation = ctx.wait_for_callback(
        move |callback_id| {
            // Values are moved into this closure
            let payment_id = payment_id_for_closure;
            let currency = currency_for_closure;
            
            async move {
                // This closure is executed inside a checkpointed child context.
                // If Lambda restarts after this runs, it WON'T run again during replay.
                
                // In a real application, you would call your payment processor API:
                // payment_processor::initiate_payment(PaymentInitiation {
                //     callback_url: format!("https://lambda.../callbacks/{}", callback_id),
                //     payment_id,
                //     amount,
                //     currency,
                // }).await?;
                
                println!(
                    "Payment processor notified: payment_id={}, callback_id={}, amount={} {}",
                    payment_id, callback_id, amount, currency
                );
                
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            }
        },
        Some(CallbackConfig {
            timeout: Duration::from_hours(1),
            heartbeat_timeout: Duration::from_minutes(10),
            ..Default::default()
        }),
    ).await?;

    // Process the confirmation
    Ok(PaymentResult {
        payment_id: validated.payment_id,
        status: confirmation.status,
        transaction_id: Some(confirmation.transaction_id),
        message: format!("Payment confirmed: {} {}", confirmation.confirmed_amount, validated.currency),
    })
}

// =============================================================================
// Example 2: wait_for_callback with Error Handling
// =============================================================================

/// Payment workflow with comprehensive error handling.
///
/// This example shows how to handle errors from the submitter function
/// and from the callback itself.
#[durable_execution]
pub async fn payment_with_error_handling(
    event: PaymentRequest,
    ctx: DurableContext,
) -> Result<PaymentResult, DurableError> {
    // Clone values before the closure
    let payment_id_for_closure = event.payment_id.clone();
    let amount = event.amount;
    
    // Attempt payment with wait_for_callback
    let result: Result<PaymentConfirmation, DurableError> = ctx.wait_for_callback(
        move |callback_id| {
            let payment_id = payment_id_for_closure;
            
            async move {
                // Simulate potential notification failure
                if amount > 10000.0 {
                    // For large amounts, require additional verification
                    return Err("Large payment requires manual verification".into());
                }
                
                // Notify payment processor
                println!("Notifying payment processor: {} with callback {}", payment_id, callback_id);
                
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            }
        },
        Some(CallbackConfig {
            timeout: Duration::from_minutes(30),
            ..Default::default()
        }),
    ).await;

    // Handle the result
    match result {
        Ok(confirmation) => {
            Ok(PaymentResult {
                payment_id: event.payment_id,
                status: "completed".to_string(),
                transaction_id: Some(confirmation.transaction_id),
                message: "Payment successful".to_string(),
            })
        }
        Err(DurableError::UserCode { message, error_type, .. }) if error_type == "SubmitterError" => {
            // The submitter (notification) failed
            // This error is checkpointed, so it won't retry the notification
            Ok(PaymentResult {
                payment_id: event.payment_id,
                status: "failed".to_string(),
                transaction_id: None,
                message: format!("Notification failed: {}", message),
            })
        }
        Err(DurableError::Callback { message, .. }) => {
            // The callback timed out or was rejected
            Ok(PaymentResult {
                payment_id: event.payment_id,
                status: "timeout".to_string(),
                transaction_id: None,
                message: format!("Payment callback failed: {}", message),
            })
        }
        Err(e) => Err(e),
    }
}

// =============================================================================
// Example 3: Multiple Sequential Callbacks
// =============================================================================

/// Order workflow with multiple callback stages.
///
/// This example shows how to use multiple wait_for_callback calls
/// for a multi-stage approval process.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrderRequest {
    pub order_id: String,
    pub items: Vec<String>,
    pub total: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApprovalResponse {
    pub approved: bool,
    pub approver: String,
    pub comments: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ShippingConfirmation {
    pub tracking_number: String,
    pub carrier: String,
    pub estimated_delivery: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderResult {
    pub order_id: String,
    pub status: String,
    pub tracking_number: Option<String>,
}

#[durable_execution]
pub async fn multi_stage_order_workflow(
    event: OrderRequest,
    ctx: DurableContext,
) -> Result<OrderResult, DurableError> {
    let order_id = event.order_id.clone();

    // Stage 1: Manager approval (for orders > $1000)
    if event.total > 1000.0 {
        let oid_for_closure = order_id.clone();
        let total = event.total;
        
        let approval: ApprovalResponse = ctx.wait_for_callback(
            move |callback_id| {
                let oid = oid_for_closure;
                async move {
                    // Send approval request to manager
                    println!(
                        "Requesting manager approval for order {} (${:.2}). Callback: {}",
                        oid, total, callback_id
                    );
                    // In production: send_slack_message, send_email, etc.
                    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
                }
            },
            Some(CallbackConfig {
                timeout: Duration::from_hours(24),
                ..Default::default()
            }),
        ).await?;

        if !approval.approved {
            return Ok(OrderResult {
                order_id,
                status: format!("rejected by {}", approval.approver),
                tracking_number: None,
            });
        }
    }

    // Stage 2: Warehouse fulfillment
    let oid_for_closure = order_id.clone();
    let items_for_closure = event.items.clone();
    
    let shipping: ShippingConfirmation = ctx.wait_for_callback(
        move |callback_id| {
            let oid = oid_for_closure;
            let items = items_for_closure;
            async move {
                // Send fulfillment request to warehouse
                println!(
                    "Requesting fulfillment for order {} ({} items). Callback: {}",
                    oid, items.len(), callback_id
                );
                // In production: call warehouse API
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            }
        },
        Some(CallbackConfig {
            timeout: Duration::from_hours(48),
            ..Default::default()
        }),
    ).await?;

    Ok(OrderResult {
        order_id,
        status: "shipped".to_string(),
        tracking_number: Some(shipping.tracking_number),
    })
}

// =============================================================================
// Example 4: Comparing Manual vs wait_for_callback
// =============================================================================

/// This example shows the WRONG way (manual callback) vs the RIGHT way (wait_for_callback).
/// 
/// The manual approach has a replay safety issue that wait_for_callback solves.
#[durable_execution]
pub async fn comparison_example(
    event: PaymentRequest,
    ctx: DurableContext,
) -> Result<PaymentResult, DurableError> {
    // =========================================================================
    // WRONG: Manual callback pattern (NOT replay-safe notification)
    // =========================================================================
    // 
    // let callback = ctx.create_callback::<PaymentConfirmation>(None).await?;
    // 
    // // DANGER: If Lambda restarts after this line but before the next checkpoint,
    // // this notification will be sent AGAIN during replay!
    // send_notification_to_payment_processor(&callback.callback_id).await?;
    // 
    // let confirmation = callback.result().await?;

    // =========================================================================
    // RIGHT: wait_for_callback pattern (replay-safe notification)
    // =========================================================================
    let payment_id_for_closure = event.payment_id.clone();
    
    let confirmation: PaymentConfirmation = ctx.wait_for_callback(
        move |callback_id| {
            let payment_id = payment_id_for_closure;
            async move {
                // This is executed inside a checkpointed child context.
                // The SDK ensures this won't re-execute during replay.
                println!("Notifying processor: payment={}, callback={}", payment_id, callback_id);
                
                // In production:
                // send_notification_to_payment_processor(&callback_id).await?;
                
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            }
        },
        None,
    ).await?;

    Ok(PaymentResult {
        payment_id: event.payment_id,
        status: confirmation.status,
        transaction_id: Some(confirmation.transaction_id),
        message: "Payment processed".to_string(),
    })
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(basic_payment_workflow)).await
}
