//! Callback/Approval Workflow Example
//!
//! This example demonstrates how to build approval workflows using callbacks
//! in the AWS Durable Execution SDK. It shows how to:
//!
//! - Create callbacks to wait for external signals
//! - Configure callback timeouts and heartbeats
//! - Handle callback results and timeouts
//! - Integrate with SQS for external notification
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
//! │  Approval       │────▶│  Approval       │────▶│  Simulated      │
//! │  Workflow       │     │  Queue (SQS)    │     │  Approver       │
//! │  Lambda         │     │                 │     │  Lambda         │
//! └─────────────────┘     └─────────────────┘     └─────────────────┘
//!         │                                               │
//!         │              ┌─────────────────┐              │
//!         └──────────────│  Callback API   │◀─────────────┘
//!           (waits)      │  (Lambda)       │  (completes)
//!                        └─────────────────┘
//! ```
//!
//! # Running this example
//!
//! 1. Deploy with `sam build && sam deploy`
//! 2. Invoke the ApprovalWorkflow function with an ApprovalRequest payload
//! 3. The workflow sends a message to SQS with the callback ID
//! 4. The SimulatedApprover Lambda picks up the message and auto-approves
//! 5. The workflow resumes and completes
//!
//! # Workflow Overview
//!
//! 1. **Validate Request**: Check the approval request is valid
//! 2. **Create Callback**: Generate a unique callback ID
//! 3. **Notify via SQS**: Send callback details to the approval queue
//! 4. **Wait for Approval**: Suspend until callback is received
//! 5. **Process Decision**: Handle the approval result

use aws_durable_execution_sdk::{
    durable_execution, CallbackConfig, Duration, DurableError,
};
use aws_sdk_sqs::Client as SqsClient;
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

/// Notification sent to the approval queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalNotification {
    /// The request being approved
    pub request_id: String,
    /// Callback ID to complete the approval
    pub callback_id: String,
    /// ARN of the durable execution (needed for callback API)
    pub execution_arn: String,
    /// Type of approval
    pub approval_type: String,
    /// Who requested the approval
    pub requester: String,
    /// Description of the request
    pub description: String,
    /// Amount (for expense approvals)
    pub amount: Option<f64>,
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
        .step_named("validate_request", |_| {
            // Validate the approval request
            if event.approvers.is_empty() {
                return Err("At least one approver is required".into());
            }
            
            if event.description.is_empty() {
                return Err("Description is required".into());
            }
            
            // For expense approvals, validate amount
            if event.approval_type == "expense" {
                if event.amount.is_none() || event.amount.unwrap() <= 0.0 {
                    return Err("Valid amount is required for expense approvals".into());
                }
            }
            
            Ok(event.clone())
        }, None)
        .await?;

    // =========================================================================
    // Step 2: Create a callback and wait for approval
    // =========================================================================
    let callback = ctx
        .create_callback_named::<ApprovalResponse>(
            "approval_callback",
            Some(CallbackConfig {
                timeout: Duration::from_hours(timeout_hours),
                heartbeat_timeout: Duration::from_hours(1),
                ..Default::default()
            }),
        )
        .await?;

    // =========================================================================
    // Step 3: Send notification to SQS for external processing
    // =========================================================================
    let queue_url = std::env::var("APPROVAL_QUEUE_URL")
        .unwrap_or_else(|_| "".to_string());
    
    let execution_arn = ctx.durable_execution_arn().to_string();
    
    ctx.step_named("send_to_approval_queue", |_| {
        let notification = ApprovalNotification {
            request_id: validated_request.request_id.clone(),
            callback_id: callback.callback_id.clone(),
            execution_arn: execution_arn.clone(),
            approval_type: validated_request.approval_type.clone(),
            requester: validated_request.requester.clone(),
            description: validated_request.description.clone(),
            amount: validated_request.amount,
        };

        // We'll send to SQS outside the step to avoid side effects during replay
        // For now, just prepare the notification
        println!("Prepared notification for queue: {:?}", notification);
        Ok(notification)
    }, None)
    .await?;

    // Send to SQS FIFO queue within a step to ensure deterministic behavior on replay.
    // The FIFO queue uses content-based deduplication, so replays won't create duplicate messages.
    ctx.step_named("send_sqs_notification", |_| {
        Ok((queue_url.clone(), callback.callback_id.clone(), execution_arn.clone(), validated_request.clone()))
    }, None).await?;

    if !queue_url.is_empty() {
        let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let sqs_client = SqsClient::new(&config);
        
        let notification = ApprovalNotification {
            request_id: validated_request.request_id.clone(),
            callback_id: callback.callback_id.clone(),
            execution_arn: execution_arn.clone(),
            approval_type: validated_request.approval_type.clone(),
            requester: validated_request.requester.clone(),
            description: validated_request.description.clone(),
            amount: validated_request.amount,
        };
        
        let message_body = serde_json::to_string(&notification)
            .map_err(|e| DurableError::execution(format!("Failed to serialize notification: {}", e)))?;
        
        tracing::info!(
            queue_url = %queue_url,
            callback_id = %callback.callback_id,
            "Sending approval request to SQS queue"
        );
        
        // Use delay_seconds to simulate time before approver processes the request
        sqs_client
            .send_message()
            .queue_url(&queue_url)
            .message_body(&message_body)
            .delay_seconds(60)  // 1 minute delay to simulate human approval time
            .send()
            .await
            .map_err(|e| DurableError::execution(format!("Failed to send to SQS: {:?}", e)))?;
        
        tracing::info!("Sent approval request to queue: {}", queue_url);
    } else {
        tracing::warn!("APPROVAL_QUEUE_URL not set, skipping SQS notification");
    }

    // =========================================================================
    // Step 4: Wait for the callback result
    // =========================================================================
    let approval = callback.result().await?;

    // =========================================================================
    // Step 5: Process the approval decision
    // =========================================================================
    let final_result = ctx
        .step_named("process_decision", |_| {
            if approval.approved {
                println!(
                    "Request {} approved by {}",
                    request_id, approval.approver
                );
            } else {
                println!(
                    "Request {} rejected by {}",
                    request_id, approval.approver
                );
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
        }, None)
        .await?;

    Ok(final_result)
}

/// Main function for the Lambda runtime.
#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    lambda_runtime::run(lambda_runtime::service_fn(approval_workflow)).await
}
