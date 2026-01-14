# Implementation Tasks

## Task 1: Add Wait-for-Condition Handler Unit Tests

**Requirements:** 1.1, 1.2, 1.3, 1.4, 1.5, 1.6

**File:** `aws-durable-execution-sdk/src/handlers/wait_for_condition.rs`

### Subtasks

- [x] 1.1: Add test for initial condition check execution on new operation
- [x] 1.2: Add test for RETRY action checkpoint with state payload
- [x] 1.3: Add test for SUCCEED action checkpoint when condition passes
- [x] 1.4: Add test for suspension when replaying PENDING status
- [x] 1.5: Add test for FAIL action checkpoint when retries exhausted
- [x] 1.6: Add test verifying StepContext.retry_payload contains previous state
- [x] 1.7: Add test for condition function receiving None on first attempt
- [x] 1.8: Add test for condition function error handling

---

## Task 2: Add Property Tests for Operation Types

**Requirements:** 2.1, 2.2, 2.3, 2.4, 2.5, 2.6

**File:** `aws-durable-execution-sdk/src/operation.rs`

### Subtasks

- [x] 2.1: Add proptest for OperationType serialization round-trip (Property 1)
- [x] 2.2: Add proptest for OperationStatus serialization round-trip (Property 2)
- [x] 2.3: Add proptest for OperationAction serialization round-trip (Property 3)
- [x] 2.4: Add proptest for terminal status classification (Property 4)
- [x] 2.5: Add proptest for Operation serialization round-trip (Property 5)
- [x] 2.6: Add proptest strategy for generating valid Operation instances

---

## Task 3: Add Property Tests for Error Types

**Requirements:** 3.1, 3.2, 3.3, 3.4, 3.5, 3.6

**File:** `aws-durable-execution-sdk/src/error.rs`

### Subtasks

- [x] 3.1: Add proptest for DurableError to ErrorObject conversion (Property 6)
- [x] 3.2: Add proptest for ErrorObject field validation (non-empty strings)
- [x] 3.3: Add proptest for SizeLimit error classification (Property 7)
- [x] 3.4: Add proptest for Throttling error classification (Property 7)
- [x] 3.5: Add proptest for ResourceNotFound error classification (Property 7)
- [x] 3.6: Add proptest for retriable error classification (Property 7)

---

## Task 4: Add Property Tests for Newtype Wrappers

**Requirements:** 4.1, 4.2, 4.3, 4.4, 4.5, 4.6

**File:** `aws-durable-execution-sdk/src/types.rs`

### Subtasks

- [x] 4.1: Add proptest for OperationId validation and round-trip (Property 8)
- [x] 4.2: Add proptest for ExecutionArn validation and round-trip (Property 9)
- [x] 4.3: Add proptest for CallbackId validation and round-trip (Property 10)
- [x] 4.4: Add proptest for empty string validation errors (Property 8)
- [x] 4.5: Add proptest for invalid ARN validation errors (Property 9)
- [x] 4.6: Add proptest for newtype HashMap key behavior (Property 11)

---

## Task 5: Add Property Tests for Configuration

**Requirements:** 5.1, 5.2, 5.3, 5.4

**File:** `aws-durable-execution-sdk/src/config.rs`

### Subtasks

- [x] 5.1: Add proptest for StepConfig validity
- [x] 5.2: Add proptest for CallbackConfig with positive timeout values
- [x] 5.3: Add proptest for Duration conversion round-trip (Property 12)
- [x] 5.4: Add proptest for RetryStrategy consistency

---

## Task 6: Add Property Tests for Checkpoint Batching

**Requirements:** 8.1, 8.2, 8.3, 8.4

**File:** `aws-durable-execution-sdk/src/state/batcher.rs`

### Subtasks

- [x] 6.1: Add proptest for parent-before-child ordering (Property 13)
- [x] 6.2: Add proptest for execution-last ordering (Property 14)
- [x] 6.3: Add proptest for operation_id uniqueness (Property 15)
- [x] 6.4: Add proptest for batch/unbatch round-trip preservation

---

## Task 7: Add Property Tests for Serialization

**Requirements:** 9.1, 9.2, 9.3, 9.4

**File:** `aws-durable-execution-sdk/src/serdes.rs`

### Subtasks

- [x] 7.1: Add proptest for JSON serialization round-trip (Property 16)
- [x] 7.2: Add proptest for Operation JSON round-trip (Property 5)
- [x] 7.3: Add proptest for timestamp format equivalence (Property 17)
- [x] 7.4: Add proptest for OperationUpdate serialization validity

---

## Task 8: Add Gap Tests for Callback Handler

**Requirements:** 10.1, 10.2, 10.3, 10.4

**File:** `aws-durable-execution-sdk/src/handlers/callback.rs`

### Subtasks

- [x] 8.1: Add test for CallbackOptions in checkpoint (timeout + heartbeat_timeout)
- [x] 8.2: Add test for missing CallbackDetails error handling
- [x] 8.3: Add test for replaying FAILED callback returns callback_id
- [x] 8.4: Add test for replaying TIMED_OUT callback returns timeout error

---

## Task 9: Add Gap Tests for Wait Handler

**Requirements:** 11.1, 11.2, 11.3

**File:** `aws-durable-execution-sdk/src/handlers/wait.rs`

### Subtasks

- [x] 9.1: Add test for double status check (before and after checkpoint)
- [x] 9.2: Add test for cancelling non-existent wait (no-op success)
- [x] 9.3: Add test for cancelling non-WAIT operation type (validation error)

---

## Task 10: Add Gap Tests for Invoke Handler

**Requirements:** 12.1, 12.2, 12.3

**File:** `aws-durable-execution-sdk/src/handlers/invoke.rs`

### Subtasks

- [x] 10.1: Add test for TIMED_OUT status handling
- [x] 10.2: Add test for STOPPED status handling
- [x] 10.3: Add test for replaying STARTED invoke (suspension)

---

## Task 11: Create Integration Test Infrastructure

**Requirements:** 13.1, 13.2, 13.3, 13.4

**Files:** 
- `aws-durable-execution-sdk/tests/common/mod.rs` (new)

### Subtasks

- [x] 11.1: Create tests/common/mod.rs with shared test utilities
- [x] 11.2: Add helper for creating mock ExecutionState with operations
- [x] 11.3: Add helper for creating mock CheckpointResponse variants
- [x] 11.4: Add proptest strategies for Operation and OperationUpdate generation

---

## Task 12: Add Replay Scenario Integration Tests

**Requirements:** 6.1, 6.2, 6.3, 6.4, 6.5

**File:** `aws-durable-execution-sdk/tests/replay_scenarios.rs` (new)

### Subtasks

- [x] 12.1: Add test for replaying workflow with multiple completed steps
- [x] 12.2: Add test for replaying workflow with pending operation (suspension)
- [x] 12.3: Add test for replaying workflow with mixed operation types
- [x] 12.4: Add test for non-deterministic change detection
- [x] 12.5: Add test for replaying workflow with nested contexts

---

## Task 13: Add Concurrency Pattern Integration Tests

**Requirements:** 7.1, 7.2, 7.3, 7.4, 7.5

**File:** `aws-durable-execution-sdk/tests/concurrency_patterns.rs` (new)

### Subtasks

- [x] 13.1: Add test for map operation processing multiple items
- [x] 13.2: Add test for parallel operation with multiple branches
- [x] 13.3: Add test for map operation with failures within tolerance
- [x] 13.4: Add test for parallel operation branch failure propagation
- [x] 13.5: Add test for replaying partially completed map

---

## Task 14: Add Documentation Tests

**Requirements:** 14.1, 14.2, 14.3, 14.4

**Files:** Various public API files

### Subtasks

- [x] 14.1: Add doctests for public API functions in lib.rs
- [x] 14.2: Add doctests for public struct constructors
- [x] 14.3: Add doctests demonstrating common usage patterns
- [x] 14.4: Verify all doctests pass with `cargo test --doc`

---

## Implementation Order

Recommended order based on dependencies and priority:

1. **Task 11** - Create integration test infrastructure (enables Tasks 12, 13)
2. **Task 1** - Wait-for-condition tests (highest priority gap - 0 tests)
3. **Tasks 2-7** - Property tests (can be parallelized)
4. **Tasks 8-10** - Handler gap tests (can be parallelized)
5. **Tasks 12-13** - Integration tests (depends on Task 11)
6. **Task 14** - Documentation tests (final polish)

## Test Execution Commands

```bash
# Run all tests
cargo test

# Run specific test file
cargo test --test replay_scenarios

# Run property tests with more iterations
PROPTEST_CASES=1000 cargo test

# Run tests with output
cargo test -- --nocapture

# Run doctests only
cargo test --doc
```
