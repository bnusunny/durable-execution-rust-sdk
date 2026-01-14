# Requirements Document

## Introduction

This document specifies the requirements for enhancing the test suite for the AWS Lambda Durable Execution Rust SDK. The SDK already has approximately 450+ unit tests across 25 modules, including some property-based tests using proptest. This specification focuses on:

1. **Gap Analysis**: Identifying missing test coverage compared to Python/Node.js SDKs
2. **Property-Based Testing**: Expanding proptest coverage for correctness properties
3. **Integration Testing**: Adding end-to-end workflow tests
4. **Test Organization**: Ensuring consistent test patterns across modules

The test patterns are informed by the existing Python and Node.js SDK test suites.

## Existing Test Coverage Summary

| Module | Unit Tests | Property Tests |
|--------|-----------|----------------|
| handlers/step.rs | 26 | Yes |
| handlers/wait.rs | 11 | No |
| handlers/callback.rs | 11 | No |
| handlers/invoke.rs | 9 | No |
| handlers/map.rs | 8 | No |
| handlers/parallel.rs | 5 | No |
| handlers/promise.rs | 17 | Yes |
| handlers/replay.rs | 21 | Yes |
| handlers/child.rs | 11 | No |
| handlers/wait_for_condition.rs | 0 | No |
| state/execution_state.rs | 49 | Yes |
| state/batcher.rs | 27 | Yes |
| operation.rs | 57 | No |
| context.rs | 59 | Yes |
| concurrency.rs | 44 | Yes |
| serdes.rs | 24 | Yes |
| duration.rs | 23 | Yes |
| lambda.rs | 35 | Yes |
| error.rs | 25 | No |
| types.rs | 42 | No |
| config.rs | 21 | No |
| client.rs | 20 | No |

## Glossary

- **SDK**: The AWS Lambda Durable Execution Rust SDK under test
- **Checkpoint**: The process of persisting operation state to the Lambda durable execution service
- **Replay**: Re-execution from the beginning while skipping completed operations using checkpointed results
- **Operation**: A fundamental unit representing checkpoints within a durable function (STEP, WAIT, CALLBACK, CHAINED_INVOKE, CONTEXT, EXECUTION)
- **Durable_Execution**: The end-to-end lifecycle of a durable function using checkpoints
- **Test_Suite**: The collection of unit tests and property-based tests validating SDK behavior
- **Mock_Client**: A test double that simulates the Lambda durable execution service API
- **Property_Test**: A test that verifies a property holds for all generated inputs using proptest
- **CheckpointedResult**: The result of checking for an existing checkpoint, containing operation state
- **OperationIdentifier**: A struct containing operation_id, parent_id, and optional name
- **StepSemantics**: Execution semantics (AT_MOST_ONCE_PER_RETRY or AT_LEAST_ONCE_PER_RETRY)
- **SuspendExecution**: A signal to pause execution and return control to Lambda runtime
- **Gap_Test**: A test that fills coverage gaps identified by comparing with Python/Node.js SDKs

## Requirements

### Requirement 1: Wait-for-Condition Handler Testing (Gap)

**User Story:** As a developer, I want tests for the wait_for_condition handler, so that I can ensure polling with state accumulation works correctly.

#### Acceptance Criteria

1. WHEN a wait-for-condition operation starts, THE Test_Suite SHALL verify the initial check is executed
2. WHEN a wait-for-condition check returns retry with state, THE Test_Suite SHALL verify RETRY action is checkpointed with the state payload
3. WHEN a wait-for-condition check succeeds, THE Test_Suite SHALL verify the final result is returned and SUCCEED is checkpointed
4. WHEN replaying a wait-for-condition in PENDING status, THE Test_Suite SHALL verify execution suspends until retry timer
5. WHEN a wait-for-condition exhausts retries, THE Test_Suite SHALL verify FAIL action is checkpointed with the final error
6. WHEN the condition function receives previous state, THE Test_Suite SHALL verify StepContext.retry_payload contains the serialized state

### Requirement 2: Property-Based Tests for Operation Types

**User Story:** As a developer, I want property-based tests for operation types and statuses, so that I can ensure correct behavior across all possible states.

#### Acceptance Criteria

1. FOR ALL OperationType values, serializing then deserializing SHALL produce the same value (round-trip property)
2. FOR ALL OperationStatus values, serializing then deserializing SHALL produce the same value (round-trip property)
3. FOR ALL OperationAction values, serializing then deserializing SHALL produce the same value (round-trip property)
4. FOR ALL terminal statuses, is_terminal SHALL return true
5. FOR ALL non-terminal statuses, is_terminal SHALL return false
6. FOR ALL Operation instances with valid fields, serializing then deserializing SHALL produce an equivalent Operation

### Requirement 3: Property-Based Tests for Error Types

**User Story:** As a developer, I want property-based tests for error types, so that I can ensure error classification and conversion work correctly.

#### Acceptance Criteria

1. FOR ALL DurableError variants, converting to ErrorObject SHALL produce valid ErrorType and ErrorMessage
2. FOR ALL ErrorObject instances, the error_type and error_message fields SHALL be non-empty strings
3. FOR ALL SizeLimit errors, is_size_limit SHALL return true and is_retriable SHALL return false
4. FOR ALL Throttling errors, is_throttling SHALL return true
5. FOR ALL ResourceNotFound errors, is_resource_not_found SHALL return true and is_retriable SHALL return false
6. FOR ALL Checkpoint errors with is_retriable=true, is_retriable SHALL return true

### Requirement 4: Property-Based Tests for Newtype Wrappers

**User Story:** As a developer, I want property-based tests for newtype wrappers, so that I can ensure type safety and serialization correctness.

#### Acceptance Criteria

1. FOR ALL non-empty strings, OperationId::new SHALL succeed and round-trip through serde
2. FOR ALL valid ARN strings, ExecutionArn::new SHALL succeed and round-trip through serde
3. FOR ALL non-empty strings, CallbackId::new SHALL succeed and round-trip through serde
4. FOR ALL empty strings, OperationId::new SHALL return ValidationError
5. FOR ALL strings not matching ARN pattern, ExecutionArn::new SHALL return ValidationError
6. FOR ALL newtype instances, using them as HashMap keys SHALL work correctly (Hash + Eq)

### Requirement 5: Property-Based Tests for Configuration

**User Story:** As a developer, I want property-based tests for configuration types, so that I can ensure configuration validation works correctly.

#### Acceptance Criteria

1. FOR ALL valid StepConfig instances, the configuration SHALL be usable without panics
2. FOR ALL valid CallbackConfig instances with positive timeout values, the configuration SHALL be valid
3. FOR ALL Duration values, converting to seconds and back SHALL preserve the value
4. FOR ALL RetryStrategy configurations, the strategy SHALL produce consistent retry decisions

### Requirement 6: Integration Tests for Replay Scenarios

**User Story:** As a developer, I want integration tests for replay scenarios, so that I can ensure the SDK correctly handles multi-step workflow replays.

#### Acceptance Criteria

1. WHEN replaying a workflow with multiple completed steps, THE Test_Suite SHALL verify all steps return cached results without re-execution
2. WHEN replaying a workflow with a pending operation, THE Test_Suite SHALL verify execution suspends at the pending operation
3. WHEN replaying a workflow with mixed operation types, THE Test_Suite SHALL verify each operation type is handled correctly
4. WHEN a replay encounters a non-deterministic change, THE Test_Suite SHALL verify NonDeterministic error is raised
5. WHEN replaying a workflow with nested contexts, THE Test_Suite SHALL verify parent-child relationships are preserved

### Requirement 7: Integration Tests for Concurrency Patterns

**User Story:** As a developer, I want integration tests for map and parallel operations, so that I can ensure concurrent processing works correctly end-to-end.

#### Acceptance Criteria

1. WHEN a map operation processes multiple items, THE Test_Suite SHALL verify all items are processed and results collected
2. WHEN a parallel operation executes multiple branches, THE Test_Suite SHALL verify all branches complete and results are combined
3. WHEN a map operation has failures within tolerance, THE Test_Suite SHALL verify the batch completes with partial success
4. WHEN a parallel operation has a branch failure, THE Test_Suite SHALL verify error propagation follows completion policy
5. WHEN replaying a partially completed map, THE Test_Suite SHALL verify only incomplete items are re-processed

### Requirement 8: Property-Based Tests for Checkpoint Batching

**User Story:** As a developer, I want property-based tests for checkpoint batching, so that I can ensure batch ordering rules are always satisfied.

#### Acceptance Criteria

1. FOR ALL batches of operation updates, child operations SHALL appear after their parent CONTEXT start
2. FOR ALL batches containing EXECUTION completion, the EXECUTION update SHALL be last
3. FOR ALL batches, each operation_id SHALL appear at most once (except STEP/CONTEXT start+complete)
4. FOR ALL valid operation update sequences, batching then unbatching SHALL preserve order

### Requirement 9: Property-Based Tests for Serialization

**User Story:** As a developer, I want property-based tests for serialization, so that I can ensure data integrity across serialize/deserialize cycles.

#### Acceptance Criteria

1. FOR ALL valid JSON-serializable Rust values, serializing then deserializing SHALL produce an equivalent value
2. FOR ALL Operation instances with valid fields, JSON round-trip SHALL preserve all fields
3. FOR ALL timestamp values (i64), deserializing from integer or ISO 8601 string SHALL produce the same result
4. FOR ALL OperationUpdate instances, serialization SHALL produce valid JSON matching API schema

### Requirement 10: Gap Tests for Callback Handler

**User Story:** As a developer, I want additional tests for callback handler edge cases, so that I can ensure parity with Python/Node.js SDK test coverage.

#### Acceptance Criteria

1. WHEN callback configuration includes timeout and heartbeat_timeout, THE Test_Suite SHALL verify CallbackOptions are included in the checkpoint
2. WHEN a callback operation has missing CallbackDetails in checkpoint response, THE Test_Suite SHALL verify appropriate error is returned
3. WHEN replaying a FAILED callback, THE Test_Suite SHALL verify the callback_id is still returned (error deferred to result())
4. WHEN replaying a TIMED_OUT callback, THE Test_Suite SHALL verify timeout error is returned from result()

### Requirement 11: Gap Tests for Wait Handler

**User Story:** As a developer, I want additional tests for wait handler edge cases, so that I can ensure parity with Python/Node.js SDK test coverage.

#### Acceptance Criteria

1. WHEN a new wait operation is created, THE Test_Suite SHALL verify status is checked twice (before and after checkpoint) for immediate response handling
2. WHEN cancelling a non-existent wait, THE Test_Suite SHALL verify the operation succeeds as no-op
3. WHEN cancelling a non-WAIT operation type, THE Test_Suite SHALL verify validation error is returned

### Requirement 12: Gap Tests for Invoke Handler

**User Story:** As a developer, I want additional tests for invoke handler edge cases, so that I can ensure parity with Python/Node.js SDK test coverage.

#### Acceptance Criteria

1. WHEN an invoke operation times out, THE Test_Suite SHALL verify TIMED_OUT status is handled correctly
2. WHEN an invoke operation is stopped externally, THE Test_Suite SHALL verify STOPPED status is handled correctly
3. WHEN replaying a STARTED invoke, THE Test_Suite SHALL verify execution suspends

### Requirement 13: Test Infrastructure Improvements

**User Story:** As a developer, I want improved test infrastructure, so that I can write tests more easily and consistently.

#### Acceptance Criteria

1. THE Test_Suite SHALL provide test helper functions for creating mock ExecutionState with pre-configured operations
2. THE Test_Suite SHALL provide test helper functions for creating mock CheckpointResponse with various states
3. THE Test_Suite SHALL provide proptest strategies for generating valid Operation instances
4. THE Test_Suite SHALL provide proptest strategies for generating valid OperationUpdate instances

### Requirement 14: Documentation Tests

**User Story:** As a developer, I want documentation tests (doctests), so that I can ensure code examples in documentation are correct.

#### Acceptance Criteria

1. THE Test_Suite SHALL include doctests for all public API functions
2. THE Test_Suite SHALL include doctests for all public struct constructors
3. THE Test_Suite SHALL include doctests demonstrating common usage patterns
4. WHEN running cargo test, THE Test_Suite SHALL verify all doctests pass
