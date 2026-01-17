//! Callback/Approval Workflow Example
//!
//! This example demonstrates how to build approval workflows using callbacks
//! in the AWS Durable Execution SDK. It shows how to:
//!
//! - Create callbacks to wait for external signals
//! - Configure callback timeouts and heartbeats
//! - Handle callback results and timeouts
//!
//! # Running this example
//!
//! This example is designed to run as an AWS Lambda function. To deploy:
//!
//! 1. Build with `cargo lambda build --release --example callback_workflow`
//! 2. Deploy to AWS Lambda with durable execution enabled
//! 3. Invoke with a JSON payload matching the `ApprovalRequest` structure
//!
//! # Workflow Overview
//!
//! 1. **Create Approval Request**: Initialize the approval workflow
//! 2. **Create Callback**: Generate a unique callback ID
//! 3. **Notify Approvers**: Send callback ID to external approval system
//! 4. **Wait for Approval**: Suspend until callback is received
//! 5. **Process Approval**: Handle the approval decision
//!
//! # External Integration
//!
//! The callback ID should be shared with an external system (e.g., via SNS, SQS,
//! or a REST API). The external system then calls the Lambda durable execution
//! callback API with the callback ID and result payload.

use aws_durable_execution_sdk::{durable_execution, CallbackConfig, DurableError, Duration};
use serde::{Deserialize, Serialize};

/// Input event for the approval workflow.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApprovalRequest {
    /// Unique request identifier
    pub request_id: String,
    /// Type of approval (e.g., "expense", "access", "deployment")
    pub approval_type: String,
    /// User requesting approval
    pub requester: String,
    /// Description of what needs approval
    pub description: String,
    /// Amount (for expense approvals)
    pub amount: Option<f64>,
    /// List of approvers
    pub approvers: Vec<String>,
    /// Timeout in hours (default: 24)
    pub timeout_hours: Option<u64>,
}

/// Response from an approver.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResponse {
    /// Whether the request was approved
    pub approved: bool,
    /// Who approved/rejected
    pub approver: String,
    /// Optional comments
    pub comments: Option<String>,
    /// Timestamp of the decision
    pub decision_timestamp: String,
}

/// Result of the approval workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResult {
    /// Request identifier
    pub request_id: String,
    /// Final status
    pub status: String,
    /// Approval decision
    pub approved: bool,
    /// Who made the decision
    pub decided_by: String,
    /// Comments from approver
    pub comments: Option<String>,
}

/// Notification sent to approvers.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApprovalNotification {
    request_id: String,
    callback_id: String,
    approval_type: String,
    requester: String,
    description: String,
    amount: Option<f64>,
    approve_url: String,
    reject_url: String,
}

/// Main approval workflow using callbacks.
///
/// This workflow demonstrates the callback pattern for human-in-the-loop
/// approval processes. The Lambda function suspends while waiting for
/// external approval, freeing up compute resources.
#[durable_execution]
pub async fn approval_workflow(
    event: ApprovalRequest,
    ctx: DurableContext,
) -> Result<ApprovalResult, DurableError> {
    let request_id = event.request_id.clone();
    let timeout_hours = event.timeout_hours.unwrap_or(24);

    // =========================================================================
    // Step 1: Validate and prepare the approval request
    // =========================================================================
    let validated_request = ctx
        .step_named(
            "validate_request",
            |_| {
                // Validate the approval request
                if event.approvers.is_empty() {
                    return Err("At least one approver is required".into());
                }

                if event.description.is_empty() {
                    return Err("Description is required".into());
                }

                // For expense approvals, validate amount
                if event.approval_type == "expense"
                    && (event.amount.is_none() || event.amount.unwrap() <= 0.0)
                {
                    return Err("Valid amount is required for expense approvals".into());
                }

                Ok(event.clone())
            },
            None,
        )
        .await?;

    // =========================================================================
    // Step 2: Create a callback and wait for approval
    // =========================================================================
    // Create a callback with a configurable timeout.
    // The callback ID will be shared with the external approval system.
    let callback = ctx
        .create_callback_named::<ApprovalResponse>(
            "approval_callback",
            Some(CallbackConfig {
                timeout: Duration::from_hours(timeout_hours),
                // Heartbeat timeout - if no heartbeat received within this time,
                // the callback is considered abandoned
                heartbeat_timeout: Duration::from_hours(1),
                ..Default::default()
            }),
        )
        .await?;

    // =========================================================================
    // Step 3: Notify approvers
    // =========================================================================
    // Send the callback ID to the external approval system.
    // In a real application, this would send emails, Slack messages, etc.
    ctx.step_named(
        "notify_approvers",
        |_| {
            let notification = ApprovalNotification {
                request_id: validated_request.request_id.clone(),
                callback_id: callback.callback_id.clone(),
                approval_type: validated_request.approval_type.clone(),
                requester: validated_request.requester.clone(),
                description: validated_request.description.clone(),
                amount: validated_request.amount,
                // In production, these would be actual URLs to your approval UI
                approve_url: format!(
                    "https://approvals.example.com/approve?callback_id={}",
                    callback.callback_id
                ),
                reject_url: format!(
                    "https://approvals.example.com/reject?callback_id={}",
                    callback.callback_id
                ),
            };

            // In a real application, you would:
            // - Send email to approvers
            // - Post to Slack channel
            // - Create a ticket in your ticketing system
            // - etc.

            println!("Notification sent: {:?}", notification);

            // Store notification for audit purposes
            Ok(notification)
        },
        None,
    )
    .await?;

    // =========================================================================
    // Step 4: Wait for the callback result
    // =========================================================================
    // This suspends the Lambda execution until the callback is received.
    // The external system calls the Lambda durable execution callback API
    // with the callback_id and an ApprovalResponse payload.
    //
    // Example external API call:
    // POST /durable-executions/{arn}/callbacks/{callback_id}/success
    // Body: { "approved": true, "approver": "manager@example.com", ... }
    let approval = callback.result().await?;

    // =========================================================================
    // Step 5: Process the approval decision
    // =========================================================================
    let final_result = ctx
        .step_named(
            "process_decision",
            |_| {
                if approval.approved {
                    // Handle approval
                    // In a real application, this might:
                    // - Update database records
                    // - Trigger downstream workflows
                    // - Send confirmation emails
                    println!("Request {} approved by {}", request_id, approval.approver);
                } else {
                    // Handle rejection
                    println!("Request {} rejected by {}", request_id, approval.approver);
                }

                Ok(ApprovalResult {
                    request_id: request_id.clone(),
                    status: if approval.approved {
                        "approved".to_string()
                    } else {
                        "rejected".to_string()
                    },
                    approved: approval.approved,
                    decided_by: approval.approver.clone(),
                    comments: approval.comments.clone(),
                })
            },
            None,
        )
        .await?;

    Ok(final_result)
}

/// Multi-level approval workflow.
///
/// This example shows how to implement a workflow that requires
/// multiple levels of approval (e.g., manager then director).
#[durable_execution]
pub async fn multi_level_approval(
    event: ApprovalRequest,
    ctx: DurableContext,
) -> Result<ApprovalResult, DurableError> {
    let request_id = event.request_id.clone();

    // =========================================================================
    // Level 1: Manager Approval
    // =========================================================================
    let manager_callback = ctx
        .create_callback_named::<ApprovalResponse>(
            "manager_approval",
            Some(CallbackConfig {
                timeout: Duration::from_hours(24),
                ..Default::default()
            }),
        )
        .await?;

    // Notify manager
    ctx.step_named(
        "notify_manager",
        |_| {
            println!(
                "Manager approval requested. Callback ID: {}",
                manager_callback.callback_id
            );
            Ok(())
        },
        None,
    )
    .await?;

    // Wait for manager approval
    let manager_response = manager_callback.result().await?;

    if !manager_response.approved {
        return Ok(ApprovalResult {
            request_id,
            status: "rejected_by_manager".to_string(),
            approved: false,
            decided_by: manager_response.approver,
            comments: manager_response.comments,
        });
    }

    // =========================================================================
    // Level 2: Director Approval (only if amount > $10,000)
    // =========================================================================
    if event.amount.unwrap_or(0.0) > 10000.0 {
        let director_callback = ctx
            .create_callback_named::<ApprovalResponse>(
                "director_approval",
                Some(CallbackConfig {
                    timeout: Duration::from_hours(48),
                    ..Default::default()
                }),
            )
            .await?;

        // Notify director
        ctx.step_named(
            "notify_director",
            |_| {
                println!(
                    "Director approval requested. Callback ID: {}",
                    director_callback.callback_id
                );
                Ok(())
            },
            None,
        )
        .await?;

        // Wait for director approval
        let director_response = director_callback.result().await?;

        if !director_response.approved {
            return Ok(ApprovalResult {
                request_id,
                status: "rejected_by_director".to_string(),
                approved: false,
                decided_by: director_response.approver,
                comments: director_response.comments,
            });
        }
    }

    // =========================================================================
    // All approvals received
    // =========================================================================
    Ok(ApprovalResult {
        request_id,
        status: "fully_approved".to_string(),
        approved: true,
        decided_by: "all_levels".to_string(),
        comments: Some("Approved by all required levels".to_string()),
    })
}

/// Main function for the Lambda runtime.
///
/// This is the entry point when running as a Lambda function.
/// The `#[durable_execution]` macro generates the actual handler.
#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    // Run the Lambda handler
    lambda_runtime::run(lambda_runtime::service_fn(approval_workflow)).await
}
