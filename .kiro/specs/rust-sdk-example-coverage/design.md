# Design Document: Rust SDK Example Coverage

## Overview

This design covers adding ~45 new example binaries, history files, and replay tests to the Rust durable execution SDK to close the coverage gap with the JS SDK. All SDK APIs are already implemented — this work is purely about adding example code and test coverage for features that lack demonstrations.

Each new example follows the established "Example Suite" pattern: a standalone binary in `examples/src/bin/{category}/{name}/`, a history JSON file in `examples/tests/history/`, and a replay test in `examples/tests/{category}_tests.rs`. History files are auto-generated via `GENERATE_HISTORY=true`, not hand-written.

The work is organized into 5 implementation phases to keep PRs manageable:
1. **Phase 1**: Wait-for-callback & callback creation examples (11 examples)
2. **Phase 2**: Parallel & map operation examples (12 examples)
3. **Phase 3**: Promise combinator & child context examples (6 examples)
4. **Phase 4**: Context validation & concurrency examples (7 examples)
5. **Phase 5**: Step & advanced feature examples (9 examples)

## Architecture

The architecture is additive — no existing code changes. New files slot into the existing structure:

```
examples/
├── Cargo.toml                          # Add [[bin]] entries for each new example
├── src/bin/
│   ├── callback/
│   │   ├── wait_for_callback/          # Existing
│   │   │   ├── wait_for_callback.rs    # Existing
│   │   │   ├── wfc_timeout.rs          # NEW
│   │   │   ├── wfc_failure.rs          # NEW
│   │   │   ├── wfc_heartbeat.rs        # NEW
│   │   │   ├── wfc_nested.rs           # NEW
│   │   │   ├── wfc_multiple.rs         # NEW
│   │   │   ├── wfc_child_context.rs    # NEW
│   │   │   └── wfc_custom_serdes.rs    # NEW
│   │   ├── heartbeat/                  # NEW directory
│   │   │   └── callback_heartbeat.rs
│   │   ├── failure/                    # NEW directory
│   │   │   └── callback_failure.rs
│   │   ├── custom_serdes/              # NEW directory
│   │   │   └── callback_custom_serdes.rs
│   │   └── mixed_ops/                  # NEW directory
│   │       └── callback_mixed_ops.rs
│   ├── parallel/
│   │   ├── empty_array/                # NEW
│   │   ├── with_wait/                  # NEW
│   │   ├── error_preservation/         # NEW
│   │   ├── min_successful/             # NEW
│   │   ├── min_successful_callback/    # NEW
│   │   ├── tolerated_failure_count/    # NEW
│   │   ├── tolerated_failure_pct/      # NEW
│   │   └── failure_threshold_exceeded/ # NEW
│   ├── map/
│   │   ├── empty_array/                # NEW
│   │   ├── error_preservation/         # NEW
│   │   ├── high_concurrency/           # NEW
│   │   └── large_scale/               # NEW
│   ├── promise_combinators/
│   │   ├── all_with_wait/              # NEW
│   │   ├── race_with_timeout/          # NEW
│   │   └── replay_behavior/            # NEW
│   ├── child_context/
│   │   ├── large_data/                 # NEW
│   │   ├── checkpoint_size_limit/      # NEW
│   │   └── with_failing_step/          # NEW
│   ├── context_validation/             # NEW category
│   │   ├── parent_in_child/
│   │   ├── parent_in_step/
│   │   └── parent_in_wait_condition/
│   ├── concurrency/                    # NEW category
│   │   ├── concurrent_operations/
│   │   ├── concurrent_wait/
│   │   ├── concurrent_callback_wait/
│   │   └── concurrent_callback_submitter/
│   ├── step/
│   │   ├── retry_exponential_backoff/  # NEW
│   │   ├── retry_with_filter/          # NEW
│   │   └── error_determinism/          # NEW
│   └── advanced/                       # NEW category
│       ├── checkpointing_eager/
│       ├── checkpointing_batched/
│       ├── checkpointing_optimistic/
│       ├── retry_exhaustion/
│       ├── large_payload/
│       └── handler_error/
├── tests/
│   ├── callback_tests.rs              # Extend with new wait_for_callback + callback tests
│   ├── parallel_map_tests.rs          # Extend with new parallel + map tests
│   ├── promise_combinator_tests.rs    # Extend with new combinator tests
│   ├── child_context_tests.rs         # Extend with new child context tests
│   ├── step_tests.rs                  # Extend with new step tests
│   ├── context_validation_tests.rs    # NEW test file
│   ├── concurrency_tests.rs           # NEW test file
│   ├── advanced_tests.rs              # NEW test file
│   └── history/
│       ├── wfc_timeout.history.json           # NEW (all below)
│       ├── wfc_failure.history.json
│       ├── wfc_heartbeat.history.json
│       ├── ... (one per example)
│       └── handler_error.history.json
```

### Decision: New test files vs extending existing

New categories (`context_validation`, `concurrency`, `advanced`) get their own test files. Existing categories (`callback`, `parallel`, `map`, `promise_combinators`, `child_context`, `step`) extend their existing test files. This keeps test organization aligned with the category structure.

### Decision: wait_for_callback examples placement

The 7 new `wait_for_callback` examples are placed in the existing `callback/wait_for_callback/` directory since they are all variations of the same API. This avoids creating 7 new subdirectories for closely related examples.

## Components and Interfaces

### Example Binary Component

Each example binary follows this interface:

```rust
//! {Description of the feature being demonstrated}
//!
//! {Additional context about the pattern}

use aws_durable_execution_sdk::{durable_execution, DurableContext, DurableError, /* feature-specific imports */};
use serde::{Deserialize, Serialize};

// Input/output types as needed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExampleInput { /* ... */ }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExampleOutput { /* ... */ }

/// Handler doc comment explaining the demonstrated feature.
#[durable_execution]
pub async fn handler(
    event: ExampleInput,  // or serde_json::Value for simple cases
    ctx: DurableContext,
) -> Result<ExampleOutput, DurableError> {
    // Feature demonstration code
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
```

### Replay Test Component

Each replay test follows this interface:

```rust
/// Handler from {category}/{name} example (without macro for testing)
async fn example_handler(
    event: InputType,
    ctx: DurableContext,
) -> Result<OutputType, DurableError> {
    // Same logic as the example binary, without #[durable_execution] macro
}

#[tokio::test]
async fn test_example_name() {
    LocalDurableTestRunner::<InputType, OutputType>::setup_test_environment(
        TestEnvironmentConfig { skip_time: true, checkpoint_delay: None }
    ).await.unwrap();

    let mut runner = LocalDurableTestRunner::new(example_handler);
    let result = runner.run(input).await.unwrap();

    // Status assertion (Succeeded, Running, or Failed depending on example)
    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
    // Result-specific assertions
    // Operation type/count assertions

    // History file comparison
    assert_nodejs_event_signatures(&result, "tests/history/{name}.history.json");
    // Or for concurrent operations:
    // assert_nodejs_event_signatures_unordered(&result, "tests/history/{name}.history.json");

    LocalDurableTestRunner::<InputType, OutputType>::teardown_test_environment().await.unwrap();
}
```

### Shared Helper Types

Some example groups share similar types. Rather than creating a shared module, each test file defines its own types (matching the existing pattern). Examples that share patterns:

- **Parallel completion config examples** (`min_successful`, `tolerated_failure_count`, `tolerated_failure_percentage`, `failure_threshold_exceeded`): All use similar task vectors and `CompletionConfig` variations
- **Checkpointing mode examples** (`eager`, `batched`, `optimistic`): All use `StepConfig` with different `CheckpointingMode` values
- **Context validation examples**: All demonstrate error behavior when misusing context scoping

### History File Generation

History files are generated, not hand-written:

1. Write the example handler and test
2. Run the test with `GENERATE_HISTORY=true` to produce the `.history.json` file
3. Subsequent test runs compare against the generated history

For concurrent/map operations, use `assert_nodejs_event_signatures_unordered()` or `assert_event_signatures_unordered()` since operation ordering is non-deterministic.

## Data Models

### Example Input/Output Types by Category

**Wait-for-Callback examples:**
- Timeout: `() → Result<String, DurableError>` (callback times out)
- Failure: `() → Result<String, DurableError>` (callback returns error)
- Heartbeat: `() → Result<String, DurableError>` (callback with heartbeats)
- Nested/Child: `() → Result<String, DurableError>` (callback in child context)
- Multiple: `() → Result<Vec<String>, DurableError>` (multiple callbacks)
- Custom serdes: `() → Result<CustomPayload, DurableError>` (custom serialization)

**Parallel examples:**
- Empty array: `() → Result<Vec<String>, DurableError>` (empty batch result)
- Error preservation: `() → Result<BatchResult, DurableError>` (partial failures)
- Min successful: `() → Result<Vec<String>, DurableError>` (early completion)
- Tolerated failure: `() → Result<Vec<String>, DurableError>` (failure tolerance)

**Map examples:**
- Empty array: `() → Result<Vec<String>, DurableError>` (empty batch)
- Large scale: `Vec<i32> → Result<Vec<String>, DurableError>` (50+ items)

**Context validation examples:**
- All: `() → Result<String, DurableError>` (expected to fail with context error)

**Advanced examples:**
- Large payload: `LargeInput → Result<LargeOutput, DurableError>`
- Handler error: `() → Result<String, DurableError>` (returns DurableError)

### Configuration Types Used

| Example | Config Type | Key Fields |
|---------|------------|------------|
| wfc_timeout | `CallbackConfig` | `timeout: Duration` |
| wfc_heartbeat | `CallbackConfig` | `heartbeat_timeout: Duration` |
| parallel/min_successful | `CompletionConfig` | `with_min_successful(n)` |
| parallel/tolerated_failure_count | `CompletionConfig` | `tolerated_failure_count` |
| parallel/tolerated_failure_pct | `CompletionConfig` | `tolerated_failure_percentage` |
| map/high_concurrency | `MapConfig` | `max_concurrency: 10+` |
| map/large_scale | `MapConfig` | `item_batcher: ItemBatcher` |
| step/retry_exponential | `StepConfig` | `retry_strategy: ExponentialBackoff` |
| step/retry_with_filter | `StepConfig` | `retryable_error_filter: RetryableErrorFilter` |
| advanced/checkpointing_* | `StepConfig` | `checkpointing_mode: CheckpointingMode::*` |
| child_context/large_data | `ChildConfig` | `set_summary_generator()` |
| child_context/with_failing_step | `ChildConfig` | `set_error_mapper()` |


## Correctness Properties

*A property is a characteristic or behavior that should hold true across all valid executions of a system — essentially, a formal statement about what the system should do. Properties serve as the bridge between human-readable specifications and machine-verifiable correctness guarantees.*

### Property 1: Example Suite Completeness

*For any* example binary registered in `Cargo.toml`, there must exist a corresponding history file in `examples/tests/history/` and a replay test function in the appropriate `examples/tests/{category}_tests.rs` file that references that history file.

**Validates: Requirements 1.8, 2.5, 3.9, 4.5, 5.4, 6.4, 7.4, 8.5, 9.4, 10.7**

### Property 2: Example File Structure

*For any* example source file (`.rs` file referenced by a `[[bin]]` entry in `Cargo.toml`), the file must contain: (a) a module-level doc comment starting with `//!`, (b) a function annotated with `#[durable_execution]`, and (c) a `#[tokio::main] async fn main()` that calls `lambda_runtime::run()`.

**Validates: Requirements 11.2, 11.3, 11.4**

### Property 3: Example Path Convention

*For any* new example binary, its path in `Cargo.toml` must match the pattern `src/bin/{category}/{name}/{example_name}.rs` where `{category}` is one of the defined category directories.

**Validates: Requirements 11.1**

### Property 4: Replay Test Assertion Usage

*For any* replay test function in the test suite, the test body must call one of `assert_event_signatures()`, `assert_nodejs_event_signatures()`, `assert_event_signatures_unordered()`, or `assert_nodejs_event_signatures_unordered()` with a path to a history file.

**Validates: Requirements 11.5**

### Property 5: Example Registration and History Completeness

*For any* example binary, its name must appear as a `[[bin]]` entry in `Cargo.toml`, and its corresponding history file must be placed in `examples/tests/history/` with a name that matches the example.

**Validates: Requirements 11.6, 11.7**

### Property 6: Replay Determinism

*For any* example handler, running the handler through `LocalDurableTestRunner` and comparing the resulting operation signatures against the recorded history file must produce an exact match (or unordered match for concurrent operations), confirming deterministic replay behavior.

**Validates: Requirements 1.8, 2.5, 3.9, 4.5, 5.4, 6.4, 7.4, 8.5, 9.4, 10.7**

## Error Handling

### Error Categories in Examples

1. **Callback failures** (Req 1.2, 2.2): Examples demonstrate catching `DurableError` from failed callbacks. The handler should match on the error and produce a meaningful result or propagate it.

2. **Parallel/Map partial failures** (Req 3.3, 3.6-3.8, 4.2): Examples demonstrate `BatchItem::get_error()` for individual item failures. The `CompletionConfig` controls whether partial failures cause overall failure.

3. **Context misuse errors** (Req 7.1-7.3): Examples demonstrate that using a parent context in the wrong scope produces a `DurableError`. These tests assert `ExecutionStatus::Failed` and verify the error is deterministic across replays.

4. **Step retry exhaustion** (Req 10.4): Example demonstrates a step that fails all retry attempts. The final error surfaces to the handler.

5. **Handler-level errors** (Req 10.6): Example demonstrates a handler returning `DurableError::Execution` directly, verifying it serializes correctly in the execution output.

### Error Handling Patterns

All examples follow the existing SDK error handling pattern:
- Steps return `Result<T, DurableError>` — errors are propagated with `?` or matched explicitly
- Parallel/map operations return `BatchResult<T>` — individual errors accessed via `BatchItem::get_error()`
- Child contexts propagate errors through `ChildConfig::set_error_mapper()` when configured
- Tests assert either `ExecutionStatus::Succeeded` or `ExecutionStatus::Failed` depending on the example's purpose

## Testing Strategy

### Dual Testing Approach

**Replay tests (primary):** Each example has a replay test that runs the handler through `LocalDurableTestRunner` and compares operation signatures against a recorded history file. This verifies deterministic replay behavior — the core correctness guarantee of the durable execution SDK.

**Property-based tests:** Structural properties (Properties 1-5) can be verified using property-based testing with `proptest` (already a dev-dependency). These tests verify that the example suite maintains structural consistency.

### Replay Test Details

- Each test uses `LocalDurableTestRunner::setup_test_environment()` / `teardown_test_environment()`
- Config: `TestEnvironmentConfig { skip_time: true, checkpoint_delay: None }`
- History comparison: `assert_nodejs_event_signatures()` for ordered operations, `assert_nodejs_event_signatures_unordered()` for concurrent/map operations
- History generation: Run with `GENERATE_HISTORY=true` to produce history files
- Each test asserts execution status, result values, and operation types

### Property-Based Test Configuration

- Library: `proptest` (already in `[dev-dependencies]`)
- Minimum 100 iterations per property test
- Each test tagged with: **Feature: rust-sdk-example-coverage, Property {N}: {title}**

### Property Test Implementation Plan

Properties 1-5 are structural properties that verify the example suite's consistency. They can be implemented as tests that enumerate all registered binaries and verify the structural invariants hold for each one. While these are "for all" properties over a finite set (the list of examples), they serve as regression guards ensuring no example is added without its complete suite.

Property 6 (replay determinism) is inherently tested by every individual replay test — each `assert_nodejs_event_signatures()` call is a determinism check. No separate property test is needed; the replay tests collectively validate this property.

### Test File Organization

| Test File | Examples Covered |
|-----------|-----------------|
| `callback_tests.rs` (extend) | wfc_timeout, wfc_failure, wfc_heartbeat, wfc_nested, wfc_multiple, wfc_child_context, wfc_custom_serdes, callback_heartbeat, callback_failure, callback_custom_serdes, callback_mixed_ops |
| `parallel_map_tests.rs` (extend) | parallel_empty_array, parallel_with_wait, parallel_error_preservation, parallel_min_successful, parallel_min_successful_callback, parallel_tolerated_failure_count, parallel_tolerated_failure_pct, parallel_failure_threshold_exceeded, map_empty_array, map_error_preservation, map_high_concurrency, map_large_scale |
| `promise_combinator_tests.rs` (extend) | all_with_wait, race_with_timeout, replay_behavior |
| `child_context_tests.rs` (extend) | large_data, checkpoint_size_limit, with_failing_step |
| `step_tests.rs` (extend) | retry_exponential_backoff, retry_with_filter, error_determinism |
| `context_validation_tests.rs` (new) | parent_in_child, parent_in_step, parent_in_wait_condition |
| `concurrency_tests.rs` (new) | concurrent_operations, concurrent_wait, concurrent_callback_wait, concurrent_callback_submitter |
| `advanced_tests.rs` (new) | checkpointing_eager, checkpointing_batched, checkpointing_optimistic, retry_exhaustion, large_payload, handler_error |

### Implementation Phases

**Phase 1 — Callbacks (11 examples):**
7 wait_for_callback variants + 4 callback creation examples. All extend `callback_tests.rs`.

**Phase 2 — Parallel & Map (12 examples):**
8 parallel examples + 4 map examples. All extend `parallel_map_tests.rs`.

**Phase 3 — Combinators & Child Context (6 examples):**
3 promise combinator examples + 3 child context examples. Extend respective test files.

**Phase 4 — Context Validation & Concurrency (7 examples):**
3 context validation + 4 concurrency examples. Create `context_validation_tests.rs` and `concurrency_tests.rs`.

**Phase 5 — Step & Advanced (9 examples):**
3 step examples + 6 advanced examples. Extend `step_tests.rs`, create `advanced_tests.rs`.

Each phase is a self-contained PR that adds examples, generates history files, and adds replay tests.
