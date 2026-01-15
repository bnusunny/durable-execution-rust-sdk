# AWS Durable Execution SDK Examples

This directory contains examples demonstrating various features of the AWS Durable Execution SDK.

## Example Categories

### Hello World
- `hello_world/` - Minimal durable function with no operations

### Step Operations
- `step/basic/` - Basic step usage with checkpointing
- `step/named/` - Named steps for better observability
- `step/with_config/` - Step configuration (semantics, retry)

### Wait Operations
- `wait/basic/` - Basic wait/pause functionality
- `wait/named/` - Named waits for debugging
- `wait/extended_duration/` - Extended duration helpers (weeks, months, years)

### Callback Operations
- `callback/simple/` - Basic callback creation
- `callback/with_timeout/` - Callback timeout and heartbeat configuration
- `callback/concurrent/` - Multiple concurrent callbacks

### Map Operations
- `map/basic/` - Basic array processing
- `map/with_concurrency/` - Concurrency limits
- `map/failure_tolerance/` - Tolerated failure percentage
- `map/min_successful/` - Quorum/minimum successful requirement

### Parallel Operations
- `parallel/basic/` - Basic parallel branches
- `parallel/first_successful/` - Complete on first success
- `parallel/heterogeneous/` - Different operation types in branches

### Child Context
- `child_context/basic/` - Isolated nested workflows
- `child_context/nested/` - Multi-level nesting

### Comprehensive Examples
- `comprehensive/order_workflow.rs` - Full order processing workflow
- `simple_workflow.rs` - Original order processing example
- `parallel_processing.rs` - Original batch processing example
- `callback_workflow.rs` - Original approval workflow example

### Error Handling
- `error_handling/step_error.rs` - Error handling patterns

## Running Examples

```bash
# Build a specific example
cargo build --example step_basic

# Run with cargo lambda
cargo lambda build --release --example step_basic

# List all examples
cargo build --examples
```

## Example Structure

Each example follows a consistent pattern:
1. Module documentation explaining the feature
2. Type definitions (input/output structs)
3. Handler function with `#[durable_execution]` macro
4. Main function for Lambda runtime
