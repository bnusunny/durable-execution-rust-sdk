# Parallel Processing Example

This example demonstrates how to process multiple items in parallel using the AWS Durable Execution SDK. It showcases:

- Using `ctx.map()` to process collections in parallel
- Configuring concurrency limits
- Handling partial failures with failure tolerance
- Different completion strategies (quorum, strict, tolerance-based)

## Project Structure

- `rust_app/src/main.rs` - Lambda function with parallel processing workflows
- `rust_app/Cargo.toml` - Rust project configuration
- `template.yaml` - SAM template defining AWS resources
- `events/` - Sample test events

## Workflows

### `process_batch` (Main Workflow)

Processes a batch of items with configurable concurrency and failure tolerance:

1. Validates the batch input
2. Processes items in parallel using `ctx.map()` with max 5 concurrent operations
3. Allows up to 10% failures before failing the entire batch
4. Aggregates and returns results

### `quorum_processing`

Demonstrates minimum successful requirement pattern - completes when a majority of items succeed.

### `strict_processing`

Requires all items to succeed (zero failure tolerance) with limited concurrency.

## Input Format

```json
{
  "batch_id": "batch-001",
  "items": [
    { "id": "item-1", "data": "hello", "priority": 5 },
    { "id": "item-2", "data": "world", "priority": 3 }
  ],
  "max_concurrency": 5
}
```

## Requirements

- Rust 1.66.0 or newer
- [SAM CLI](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/serverless-sam-cli-install.html)
- [cargo-lambda](https://github.com/cargo-lambda/cargo-lambda) for cross-compilation
- Docker (for local testing)

## Build and Deploy

```bash
# Build the application
sam build

# Deploy to AWS
sam deploy --guided
```

## Local Testing

```bash
# Build
sam build

# Invoke locally with test event
sam local invoke ParallelProcessingFunction --event events/event.json
```

## Cleanup

```bash
sam delete
```
