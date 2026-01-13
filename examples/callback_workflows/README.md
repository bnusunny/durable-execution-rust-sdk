# Callback Workflows Example

This example demonstrates approval workflows using callbacks with the AWS Durable Execution SDK. It includes a simulated approver that automatically processes approval requests, making the example self-contained and testable.

## Architecture

```
┌─────────────────────┐     ┌─────────────────────┐     ┌─────────────────────┐
│  ApprovalWorkflow   │────▶│  ApprovalQueue      │────▶│  SimulatedApprover  │
│  (Durable Lambda)   │     │  (SQS Standard)     │     │  (Lambda)           │
└─────────────────────┘     └─────────────────────┘     └─────────────────────┘
         │                                                       │
         │                  ┌─────────────────────┐              │
         └──────────────────│  Callback API       │◀─────────────┘
           (suspends)       │  (Lambda Service)   │   (completes)
                            └─────────────────────┘
```

1. **ApprovalWorkflow**: A durable Lambda that creates a callback and sends approval requests to SQS
2. **ApprovalQueue**: Standard SQS queue with 1-minute delay to simulate human approval time
3. **SimulatedApprover**: Lambda triggered by SQS that auto-approves requests under $10,000

## Files

- `rust_app/src/main.rs` - Approval workflow with callback pattern
- `rust_app/src/simulated_approver.rs` - Simulated external approver
- `template.yaml` - SAM template defining all AWS resources
- `events/event.json` - Test event (auto-approved, amount < $10k)
- `events/event_large_amount.json` - Test event (rejected, amount > $10k)

## Requirements

- Rust 1.66.0+
- [SAM CLI](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/serverless-sam-cli-install.html)
- [cargo-lambda](https://github.com/cargo-lambda/cargo-lambda)
- Docker

## Deploy

```bash
sam build
sam deploy --guided
```

## Test

> **Note**: Durable execution features (checkpointing, callbacks) require the actual Lambda service. You must deploy the stack before testing. Local invoke (`sam local invoke`) won't work because the checkpoint API requires a valid Lambda ARN.

First, ensure you've deployed the stack with `sam deploy`. 

Since durable functions with long execution timeouts (>15 min) require async invocation, use:

```bash
sam remote invoke ApprovalWorkflowFunction --event-file events/event.json --parameter InvocationType=Event
```

To check the execution status:

```bash
aws lambda list-durable-executions --function-name <function-name>
```

Test with a large amount that will be rejected:

```bash
sam remote invoke ApprovalWorkflowFunction --event-file events/event_large_amount.json --parameter InvocationType=Event
```

## How It Works

1. The workflow validates the approval request
2. Creates a callback with a 24-hour timeout
3. Uses a deterministic step to prepare the SQS notification
4. Sends the callback details to the SQS queue with a 1-minute delay
5. Suspends execution, waiting for the callback
6. After the delay, the SimulatedApprover Lambda picks up the SQS message
7. It decides to approve (amount ≤ $10k) or reject (amount > $10k)
8. Calls the Lambda callback API to complete the approval
9. The workflow resumes and processes the decision

### Deterministic Replay Handling

The workflow uses `step_named("send_sqs_notification")` before sending to SQS to ensure deterministic behavior during replay. This prevents duplicate messages from being sent if the Lambda is replayed from a checkpoint.

## Customization

To implement real approval logic, replace the `SimulatedApprover` with:
- A human approval UI that calls the callback API
- An email-based approval system
- A Slack bot integration
- Any external system that can make HTTP calls

The callback API endpoint format:
```
POST https://lambda.{region}.amazonaws.com/2025-12-01/durable-execution-callbacks/{callback_id}/succeed
```

## Cleanup

```bash
sam delete
```
