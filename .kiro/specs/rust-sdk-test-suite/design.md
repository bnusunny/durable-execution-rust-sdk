# Design Document: Rust SDK Test Suite Enhancement

## Overview

This design document specifies the test suite enhancements for the AWS Lambda Durable Execution Rust SDK. The SDK already has 450+ unit tests across 25 modules. This design focuses on:

1. Filling test coverage gaps (wait_for_condition handler has 0 tests)
2. Expanding property-based testing using proptest
3. Adding integration tests for complex replay scenarios
4. Improving test infrastructure with reusable helpers and strategies

## Architecture

### Test Organization

```
aws-durable-execution-sdk/
├── src/
│   ├── handlers/
│   │   ├── wait_for_condition.rs  # Add unit tests (currently 0)
│   │   └── ...                    # Existing unit tests
│   ├── test_helpers.rs            # NEW: Shared test utilities (for unit tests)
│   └── ...
├── tests/                         # NEW: Integration tests directory
│   ├── common/
│   │   └── mod.rs                 # Shared test utilities for integration tests
│   ├── replay_scenarios.rs        # Multi-step replay integration tests
│   └── concurrency_patterns.rs    # Map/parallel integration tests
└── Cargo.toml
```

### Test Types and Locations

| Test Type | Location | Description |
|-----------|----------|-------------|
| Unit Tests | `src/**/*.rs` (in `#[cfg(test)]` modules) | Test individual functions/modules in isolation |
| Property Tests | `src/**/*.rs` (using proptest) | Test properties across generated inputs |
| Integration Tests | `tests/*.rs` | Test multiple components working together |

### Integration Test Structure

Integration tests in `tests/` directory:
- Have access to the crate's public API only
- Can test end-to-end workflows
- Are compiled as separate binaries
- Run with `cargo test --test <test_name>`
- **Use MockDurableServiceClient** - NO actual AWS accounts required
- Test the SDK's internal logic and state management, not AWS API connectivity

**Note**: These are "integration tests" in the Rust sense (testing multiple modules together via public API), not cloud integration tests. All tests use the existing `MockDurableServiceClient` to simulate checkpoint API responses. This matches the Python SDK's approach where `tests/e2e/` also uses mocks.

### Future: Cloud End-to-End Tests (Out of Scope)

Real AWS service integration tests are **out of scope** for this spec. They would require:
- AWS account credentials and IAM permissions
- Deployed Lambda functions with durable execution enabled
- Test infrastructure for managing cloud resources
- Longer execution times and potential costs

These could be added in a separate spec focused on cloud testing infrastructure.

### Testing Framework

- **Unit Tests**: Rust's built-in `#[test]` and `#[tokio::test]`
- **Property-Based Tests**: `proptest` crate (already in use)
- **Mocking**: `MockDurableServiceClient` (already implemented)
- **Async Runtime**: `tokio` (already in use)

## Components and Interfaces

### Test Helper Module

```rust
// src/test_helpers.rs (new module)

use crate::client::{CheckpointResponse, MockDurableServiceClient, NewExecutionState};
use crate::lambda::InitialExecutionState;
use crate::operation::{Operation, OperationType, OperationStatus};
use crate::state::ExecutionState;
use std::sync::Arc;

/// Creates a mock client with default checkpoint responses
pub fn create_mock_client() -> Arc<MockDurableServiceClient> {
    Arc::new(
        MockDurableServiceClient::new()
            .with_checkpoint_response(Ok(CheckpointResponse::new("token-1")))
    )
}

/// Creates an ExecutionState with pre-configured operations
pub fn create_test_state_with_operations(
    operations: Vec<Operation>,
) -> Arc<ExecutionState> {
    let client = create_mock_client();
    let initial_state = InitialExecutionState::with_operations(operations);
    Arc::new(ExecutionState::new(
        "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
        "initial-token",
        initial_state,
        client,
    ))
}

/// Creates an Operation with specified type and status
pub fn create_operation(
    id: &str,
    op_type: OperationType,
    status: OperationStatus,
) -> Operation {
    let mut op = Operation::new(id, op_type);
    op.status = status;
    op
}
```

### Proptest Strategies Module

```rust
// Strategies for generating test data (in test modules)

use proptest::prelude::*;
use crate::operation::{OperationType, OperationStatus, OperationAction, Operation};

/// Strategy for generating valid OperationType values
pub fn operation_type_strategy() -> impl Strategy<Value = OperationType> {
    prop_oneof![
        Just(OperationType::Execution),
        Just(OperationType::Step),
        Just(OperationType::Wait),
        Just(OperationType::Callback),
        Just(OperationType::Invoke),
        Just(OperationType::Context),
    ]
}

/// Strategy for generating valid OperationStatus values
pub fn operation_status_strategy() -> impl Strategy<Value = OperationStatus> {
    prop_oneof![
        Just(OperationStatus::Started),
        Just(OperationStatus::Pending),
        Just(OperationStatus::Ready),
        Just(OperationStatus::Succeeded),
        Just(OperationStatus::Failed),
        Just(OperationStatus::Cancelled),
        Just(OperationStatus::TimedOut),
        Just(OperationStatus::Stopped),
    ]
}

/// Strategy for generating non-empty strings (for IDs)
pub fn non_empty_string_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_-]{1,64}".prop_map(|s| s)
}

/// Strategy for generating valid ExecutionArn strings
pub fn execution_arn_strategy() -> impl Strategy<Value = String> {
    (
        prop_oneof![Just("aws"), Just("aws-cn"), Just("aws-us-gov")],
        "[a-z]{2}-[a-z]+-[0-9]",
        "[0-9]{12}",
        "[a-zA-Z0-9_-]{1,32}",
        "[a-zA-Z0-9]{8,32}",
    ).prop_map(|(partition, region, account, func, exec_id)| {
        format!("arn:{}:lambda:{}:{}:function:{}:durable:{}", 
                partition, region, account, func, exec_id)
    })
}
```

## Data Models

### Test Fixtures

The test suite uses the existing data models from the SDK:
- `Operation` - Represents a checkpointed operation
- `OperationUpdate` - Represents a checkpoint update request
- `CheckpointResponse` - Response from checkpoint API
- `ExecutionState` - Manages operation state during execution
- `DurableError` - Error types for the SDK

No new data models are required for testing.

## Correctness Properties

*A property is a characteristic or behavior that should hold true across all valid executions of a system—essentially, a formal statement about what the system should do. Properties serve as the bridge between human-readable specifications and machine-verifiable correctness guarantees.*

### Property 1: OperationType Serialization Round-Trip

*For any* OperationType value, serializing to JSON then deserializing SHALL produce the same value.

**Validates: Requirements 2.1**

### Property 2: OperationStatus Serialization Round-Trip

*For any* OperationStatus value, serializing to JSON then deserializing SHALL produce the same value.

**Validates: Requirements 2.2**

### Property 3: OperationAction Serialization Round-Trip

*For any* OperationAction value, serializing to JSON then deserializing SHALL produce the same value.

**Validates: Requirements 2.3**

### Property 4: Terminal Status Classification

*For any* OperationStatus that is terminal (Succeeded, Failed, Cancelled, TimedOut, Stopped), is_terminal() SHALL return true, and for non-terminal statuses (Started, Pending, Ready), is_terminal() SHALL return false.

**Validates: Requirements 2.4, 2.5**

### Property 5: Operation Serialization Round-Trip

*For any* Operation instance with valid fields, serializing to JSON then deserializing SHALL produce an equivalent Operation.

**Validates: Requirements 2.6, 9.2**

### Property 6: DurableError to ErrorObject Conversion

*For any* DurableError variant, converting to ErrorObject SHALL produce an ErrorObject with non-empty error_type and error_message fields.

**Validates: Requirements 3.1, 3.2**

### Property 7: Error Type Classification Consistency

*For any* SizeLimit error, is_size_limit() SHALL return true and is_retriable() SHALL return false. *For any* Throttling error, is_throttling() SHALL return true. *For any* ResourceNotFound error, is_resource_not_found() SHALL return true and is_retriable() SHALL return false.

**Validates: Requirements 3.3, 3.4, 3.5, 3.6**

### Property 8: OperationId Validation and Round-Trip

*For any* non-empty string, OperationId::new() SHALL succeed and the resulting OperationId SHALL round-trip through serde. *For any* empty string, OperationId::new() SHALL return ValidationError.

**Validates: Requirements 4.1, 4.4**

### Property 9: ExecutionArn Validation and Round-Trip

*For any* string matching the ARN pattern (arn:<partition>:lambda:<region>:<account>:function:<name>:durable:<id>), ExecutionArn::new() SHALL succeed and round-trip through serde. *For any* string not matching this pattern, ExecutionArn::new() SHALL return ValidationError.

**Validates: Requirements 4.2, 4.5**

### Property 10: CallbackId Validation and Round-Trip

*For any* non-empty string, CallbackId::new() SHALL succeed and round-trip through serde.

**Validates: Requirements 4.3**

### Property 11: Newtype HashMap Key Behavior

*For any* newtype instance (OperationId, ExecutionArn, CallbackId), inserting into a HashMap and retrieving by an equal key SHALL return the same value.

**Validates: Requirements 4.6**

### Property 12: Duration Conversion Round-Trip

*For any* Duration value, converting to seconds and creating a new Duration from those seconds SHALL produce an equivalent Duration.

**Validates: Requirements 5.3**

### Property 13: Checkpoint Batch Ordering - Parent Before Child

*For any* batch of OperationUpdate instances, if a child operation (with parent_id) is present, its parent CONTEXT START must appear before it in the batch.

**Validates: Requirements 8.1**

### Property 14: Checkpoint Batch Ordering - Execution Last

*For any* batch of OperationUpdate instances containing an EXECUTION completion (SUCCEED or FAIL), the EXECUTION update SHALL be the last item in the batch.

**Validates: Requirements 8.2**

### Property 15: Checkpoint Batch Uniqueness

*For any* batch of OperationUpdate instances, each operation_id SHALL appear at most once, except for STEP and CONTEXT operations which may have both START and completion (SUCCEED/FAIL/RETRY) in the same batch.

**Validates: Requirements 8.3**

### Property 16: JSON Serialization Round-Trip

*For any* valid JSON-serializable Rust value (primitives, Vec, HashMap), serializing then deserializing SHALL produce an equivalent value.

**Validates: Requirements 9.1**

### Property 17: Timestamp Format Equivalence

*For any* timestamp value (i64 milliseconds), deserializing from the integer format or from an equivalent ISO 8601 string SHALL produce the same result.

**Validates: Requirements 9.3**

### Property 18: Wait-for-Condition State Preservation

*For any* state payload passed to RETRY action, the next attempt SHALL receive that payload via StepContext.retry_payload.

**Validates: Requirements 1.2, 1.6**

## Error Handling

### Test Error Scenarios

The test suite must verify error handling for:

1. **Validation Errors**: Invalid inputs (empty strings, invalid ARNs, duration < 1 second)
2. **Serialization Errors**: Invalid JSON, type mismatches
3. **Non-Deterministic Errors**: Operation type mismatch during replay
4. **Checkpoint Errors**: API failures, invalid tokens
5. **Timeout Errors**: Callback and invoke timeouts
6. **Suspension**: Pending operations that require suspension

### Error Assertion Patterns

```rust
// Pattern for testing error types
match result {
    Err(DurableError::Validation { message }) => {
        assert!(message.contains("expected text"));
    }
    _ => panic!("Expected Validation error"),
}

// Pattern for testing suspension
assert!(matches!(result, Err(DurableError::Suspend { .. })));
```

## Testing Strategy

### Dual Testing Approach

The test suite uses both unit tests and property-based tests:

- **Unit tests**: Verify specific examples, edge cases, and error conditions
- **Property tests**: Verify universal properties across all inputs using proptest

### Property-Based Testing Configuration

- **Framework**: proptest crate
- **Minimum iterations**: 100 per property test
- **Tag format**: `// Feature: rust-sdk-test-suite, Property N: <property_text>`

### Test Categories

1. **Gap Tests**: Fill coverage gaps identified in requirements
   - wait_for_condition handler (0 tests → comprehensive coverage)
   - Callback handler edge cases
   - Wait handler edge cases
   - Invoke handler edge cases

2. **Property Tests**: Expand proptest coverage
   - Serialization round-trips
   - Error classification
   - Newtype validation
   - Batch ordering invariants

3. **Integration Tests**: End-to-end workflow tests
   - Multi-step replay scenarios
   - Concurrency patterns (map/parallel)

### Test File Organization

```
aws-durable-execution-sdk/
├── src/
│   ├── handlers/
│   │   └── wait_for_condition.rs  # Add #[cfg(test)] mod tests
│   ├── operation.rs               # Add property tests
│   ├── error.rs                   # Add property tests
│   ├── types.rs                   # Add property tests (some exist)
│   ├── state/
│   │   └── batcher.rs             # Add property tests for ordering
│   └── serdes.rs                  # Expand property tests
└── tests/                         # NEW: Integration tests
    ├── common/
    │   └── mod.rs                 # Shared fixtures and helpers
    ├── replay_scenarios.rs        # Requirement 6: Replay integration tests
    └── concurrency_patterns.rs    # Requirement 7: Map/parallel tests
```

### Mock Client Usage

All handler tests use `MockDurableServiceClient` to simulate API responses:

```rust
let client = Arc::new(
    MockDurableServiceClient::new()
        .with_checkpoint_response(Ok(CheckpointResponse::new("token-1")))
        .with_checkpoint_response(Ok(CheckpointResponse::new("token-2")))
);
```
