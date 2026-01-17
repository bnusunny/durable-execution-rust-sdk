//! Tests for comprehensive examples using LocalDurableTestRunner.

use aws_durable_execution_sdk::{
    CallbackConfig, CompletionConfig, DurableContext, DurableError, Duration, MapConfig,
    OperationType,
};
use aws_durable_execution_sdk_examples::test_helper::assert_event_signatures_unordered;
use aws_durable_execution_sdk_testing::{
    ExecutionStatus, LocalDurableTestRunner, TestEnvironmentConfig,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrderResult {
    pub order_id: String,
    pub status: String,
    pub total_cents: u64,
    pub items_processed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApprovalResponse {
    pub approved: bool,
    pub approver: String,
}

/// Handler from comprehensive/order_workflow example (without macro for testing)
async fn order_workflow_handler(
    event: OrderEvent,
    ctx: DurableContext,
) -> Result<OrderResult, DurableError> {
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

#[tokio::test]
async fn test_order_workflow_no_approval() {
    LocalDurableTestRunner::<OrderEvent, OrderResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(order_workflow_handler);
    let input = OrderEvent {
        order_id: "order-123".to_string(),
        customer_id: "customer-456".to_string(),
        items: vec![
            OrderItem {
                sku: "SKU-001".to_string(),
                quantity: 2,
                price_cents: 1000,
            },
            OrderItem {
                sku: "SKU-002".to_string(),
                quantity: 1,
                price_cents: 2500,
            },
        ],
        requires_approval: false,
    };
    let result = runner.run(input).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    let order_result = result.get_result().unwrap();
    assert_eq!(order_result.order_id, "order-123");
    assert_eq!(order_result.status, "completed");
    assert_eq!(order_result.total_cents, 4500); // (2*1000) + (1*2500)
    assert_eq!(order_result.items_processed, 2);

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    let step_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Step)
        .collect();
    assert!(!step_ops.is_empty(), "Should have step operations");

    let context_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Context)
        .collect();
    assert!(!context_ops.is_empty(), "Should have context operations");

    // Use unordered comparison due to map concurrency
    assert_event_signatures_unordered(
        operations,
        "tests/history/order_workflow_no_approval.history.json",
    );

    LocalDurableTestRunner::<OrderEvent, OrderResult>::teardown_test_environment()
        .await
        .unwrap();
}

#[tokio::test]
async fn test_order_workflow_with_approval() {
    LocalDurableTestRunner::<OrderEvent, OrderResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(order_workflow_handler);
    let input = OrderEvent {
        order_id: "order-789".to_string(),
        customer_id: "customer-012".to_string(),
        items: vec![OrderItem {
            sku: "SKU-003".to_string(),
            quantity: 1,
            price_cents: 5000,
        }],
        requires_approval: true,
    };
    let result = runner.run(input).await.unwrap();

    // With approval required, the execution should be running (waiting for callback)
    assert_eq!(result.get_status(), ExecutionStatus::Running);

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    let callback_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Callback)
        .collect();
    assert!(
        !callback_ops.is_empty(),
        "Should have callback operation for approval"
    );

    let approval_callback = callback_ops.first().unwrap();
    assert_eq!(approval_callback.name, Some("order_approval".to_string()));

    // Use unordered comparison due to map concurrency
    assert_event_signatures_unordered(
        operations,
        "tests/history/order_workflow_with_approval.history.json",
    );

    LocalDurableTestRunner::<OrderEvent, OrderResult>::teardown_test_environment()
        .await
        .unwrap();
}
