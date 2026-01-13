//! Simulated Approver Lambda
//!
//! This Lambda function simulates an external approver by:
//! 1. Receiving approval requests from SQS
//! 2. Auto-approving them after a brief delay
//! 3. Calling the Lambda callback API to complete the approval
//!
//! In a real application, this would be replaced by a human approval UI,
//! email-based approval, Slack bot, or other external system.

use aws_credential_types::provider::ProvideCredentials;
use aws_lambda_events::event::sqs::SqsEvent;
use chrono::Utc;
use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde::{Deserialize, Serialize};

/// Notification received from the approval workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalNotification {
    pub request_id: String,
    pub callback_id: String,
    pub execution_arn: String,
    pub approval_type: String,
    pub requester: String,
    pub description: String,
    pub amount: Option<f64>,
}

/// Response sent back to the approval workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResponse {
    pub approved: bool,
    pub approver: String,
    pub comments: Option<String>,
    pub decision_timestamp: String,
}

/// Handler for SQS events containing approval requests.
async fn handler(event: LambdaEvent<SqsEvent>) -> Result<(), Error> {
    for record in event.payload.records {
        let body = match &record.body {
            Some(b) => b,
            None => {
                tracing::warn!("Skipping record with no body");
                continue;
            }
        };

        let notification: ApprovalNotification = match serde_json::from_str(body) {
            Ok(n) => n,
            Err(e) => {
                tracing::error!("Failed to parse notification: {}", e);
                continue;
            }
        };

        tracing::info!(
            request_id = %notification.request_id,
            callback_id = %notification.callback_id,
            callback_id_len = notification.callback_id.len(),
            "Processing approval request"
        );

        // Simulate approval decision logic
        let (approved, comments) = simulate_approval_decision(&notification);

        let response = ApprovalResponse {
            approved,
            approver: "simulated-approver@example.com".to_string(),
            comments: Some(comments),
            decision_timestamp: Utc::now().to_rfc3339(),
        };

        // Call the Lambda callback API to complete the approval
        match complete_callback(&notification, &response).await {
            Ok(_) => {
                tracing::info!(
                    request_id = %notification.request_id,
                    approved = approved,
                    "Successfully completed callback"
                );
            }
            Err(e) => {
                tracing::error!(
                    request_id = %notification.request_id,
                    error = %e,
                    "Failed to complete callback"
                );
                return Err(e.into());
            }
        }
    }

    Ok(())
}

/// Simulates approval decision logic.
/// In this example, we auto-approve everything under $10,000.
fn simulate_approval_decision(notification: &ApprovalNotification) -> (bool, String) {
    match notification.amount {
        Some(amount) if amount > 10000.0 => (
            false,
            format!(
                "Amount ${:.2} exceeds auto-approval limit of $10,000",
                amount
            ),
        ),
        Some(amount) => (
            true,
            format!("Auto-approved: ${:.2} is within limits", amount),
        ),
        None => (true, "Auto-approved: no amount specified".to_string()),
    }
}

/// Calls the Lambda callback API to complete the approval.
async fn complete_callback(
    notification: &ApprovalNotification,
    response: &ApprovalResponse,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // The Result is sent as binary data (the raw JSON bytes)
    let payload = serde_json::to_vec(response)?;

    // The callback API endpoint format:
    // POST /2025-12-01/durable-execution-callbacks/{CallbackId}/succeed
    
    let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string());
    
    // The callback ID must be URL-encoded since it contains base64 characters like / and +
    let encoded_callback_id = urlencoding::encode(&notification.callback_id);
    let url = format!(
        "https://lambda.{}.amazonaws.com/2025-12-01/durable-execution-callbacks/{}/succeed",
        region, encoded_callback_id
    );

    tracing::info!(
        execution_arn = %notification.execution_arn,
        callback_id = %notification.callback_id,
        callback_id_len = notification.callback_id.len(),
        encoded_callback_id_len = encoded_callback_id.len(),
        payload_len = payload.len(),
        url = %url,
        "Completing callback via Lambda API"
    );

    // Use the AWS SDK's credentials to sign the request
    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    
    // Make a signed HTTP request to the callback API
    let http_client = reqwest::Client::new();
    
    // Get credentials for signing
    let credentials = config
        .credentials_provider()
        .ok_or("No credentials provider")?
        .provide_credentials()
        .await
        .map_err(|e| format!("Failed to get credentials: {}", e))?;

    // Sign the request using SigV4
    use aws_sigv4::http_request::{sign, SignableBody, SignableRequest, SigningSettings};
    use aws_sigv4::sign::v4;
    use std::time::SystemTime;

    let identity = aws_smithy_runtime_api::client::identity::Identity::from(credentials);
    let signing_settings = SigningSettings::default();
    let signing_params = v4::SigningParams::builder()
        .identity(&identity)
        .region(&region)
        .name("lambda")
        .time(SystemTime::now())
        .settings(signing_settings)
        .build()?;

    let signable_request = SignableRequest::new(
        "POST",
        &url,
        std::iter::empty::<(&str, &str)>(),
        SignableBody::Bytes(&payload),
    )?;

    let (signing_instructions, _signature) = sign(signable_request, &signing_params.into())?.into_parts();

    // Build the HTTP request - use application/octet-stream for binary data
    let mut request = http_client
        .post(&url)
        .header("Content-Type", "application/octet-stream")
        .body(payload.clone());

    // Apply signing headers
    let mut temp_request = http::Request::builder()
        .method("POST")
        .uri(&url)
        .body(())?;
    
    signing_instructions.apply_to_request_http1x(&mut temp_request);
    
    for (name, value) in temp_request.headers() {
        request = request.header(name.as_str(), value.to_str()?);
    }

    let response = request.send().await?;
    
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Callback API returned {}: {}", status, body).into());
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    lambda_runtime::run(service_fn(handler)).await
}
