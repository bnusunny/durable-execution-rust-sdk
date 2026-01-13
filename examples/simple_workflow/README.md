# Simple Workflow Example

A SAM-based serverless application demonstrating the AWS Durable Execution SDK for Rust. This example implements an order processing workflow that survives Lambda restarts by checkpointing each step.

## Project Structure

- `rust_app/src/main.rs` - Order processing workflow implementation
- `rust_app/Cargo.toml` - Rust project configuration
- `template.yaml` - SAM template defining AWS resources
- `events/` - Sample test events

## The Workflow

The `process_order` function uses the `#[durable_execution]` macro to create a durable Lambda handler that executes 4 checkpointed stages:

1. **Validate Order** - Checks that items exist and amount > 0
2. **Process Payment** - Simulates charging the customer
3. **Wait for Confirmation** - Pauses execution for 5 seconds using `ctx.wait()`
4. **Fulfill Order** - Creates tracking info and completes the order

Each step uses `ctx.step_named()` which automatically checkpoints the result. If Lambda restarts mid-workflow, it resumes from the last completed checkpoint rather than re-running everything.

## SDK Features Demonstrated

- `#[durable_execution]` macro - transforms an async function into a durable Lambda handler
- `ctx.step_named()` - executes and checkpoints a step
- `ctx.wait()` - suspends execution for a duration (Lambda will be re-invoked after)
- `DurableError` - error handling with termination reasons

## Requirements

- [Rust](https://www.rust-lang.org/) v1.66.0 or newer
- [SAM CLI](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/serverless-sam-cli-install.html)
- [Docker](https://hub.docker.com/search/?type=edition&offering=community)
- [cargo-lambda](https://github.com/cargo-lambda/cargo-lambda) for cross-compilation

## Deploy

```bash
sam build
sam deploy
```

Or use `sam deploy --guided` for interactive configuration.

## Test Locally

Build the application:

```bash
sam build
```

Invoke the function with a test event:

```bash
sam local invoke OrderProcessingFunction --event events/event.json
```

## Sample Event

```json
{
  "order_id": "ORD-12345",
  "customer_id": "CUST-001",
  "amount_cents": 9999,
  "items": [
    {
      "sku": "WIDGET-001",
      "quantity": 2,
      "unit_price_cents": 4999
    }
  ]
}
```

## Sample Response

```json
{
  "status": "completed",
  "order_id": "ORD-12345",
  "transaction_id": "txn_00000192abc123...",
  "tracking_number": "TRK00000192abc123...",
  "estimated_delivery": "2024-01-15"
}
```

## Infrastructure

The `template.yaml` defines a Lambda function with:

- `DurableConfig.ExecutionTimeout`: 3600 seconds (1 hour)
- `DurableConfig.RetentionPeriodInDays`: 7 days
- Build method: `rust-cargolambda`
- Architecture: x86_64

## Cleanup

```bash
sam delete
```

## Resources

- [AWS SAM Developer Guide](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/what-is-sam.html)
- [AWS Durable Execution SDK Documentation](../../aws-durable-execution-sdk/docs/)
