# Requirements Document

## Introduction

This document defines the requirements for adding async closure support to the durable execution SDK's `step`, `step_named`, and `wait_for_condition` operations. Currently these operations only accept synchronous closures, forcing users into workarounds like `tokio::runtime::Handle::block_on` when calling async libraries (AWS SDK, HTTP clients). The feature modifies these methods in-place to accept async closures using the `F + Fut` two-generic pattern, which is a breaking change acceptable for the alpha API.

## Glossary

- **DurableContext**: The primary context object that provides durable execution operations (`step`, `step_named`, `wait_for_condition`, etc.) to user workflow code
- **Step_Handler**: The internal handler function that manages checkpoint/replay logic for step operations
- **WFC_Handler**: The internal handler function (`wait_for_condition_handler`) that manages checkpoint/replay logic for wait-for-condition operations
- **StepFn**: The existing synchronous trait alias for step closures, to be removed
- **F_Fut_Pattern**: The two-generic-parameter pattern (`F: FnOnce(...) -> Fut + Send`, `Fut: Future<...> + Send`) used for async closure bounds, already established by `map`, `parallel`, and `run_in_child_context`
- **Checkpoint**: A persisted record of an operation's state (START, SUCCEED, FAIL, RETRY) used for replay
- **Replay**: The process of returning cached checkpoint results without re-executing the closure
- **StepContext**: The context object passed to step closures, containing operation metadata
- **DurableValue**: A trait alias for types that are `Serialize + DeserializeOwned + Send`
- **AtMostOnce_Semantics**: Step execution semantics where a START checkpoint is created before the closure runs, ensuring the closure executes at most once per retry
- **AtLeastOnce_Semantics**: Step execution semantics where the closure runs before any checkpoint is created, allowing re-execution on retry
- **WaitForConditionConfig**: Configuration for polling-based condition checks, including initial state, wait strategy, and timing

## Requirements

### Requirement 1: Async Closure Acceptance

**User Story:** As a developer, I want `step`, `step_named`, and `wait_for_condition` to accept async closures, so that I can use async libraries (AWS SDK, HTTP clients) directly inside step operations without workarounds.

#### Acceptance Criteria

1. WHEN a user passes an async closure to `step`, THE DurableContext SHALL accept the closure using the F_Fut_Pattern with bounds `F: FnOnce(StepContext) -> Fut + Send` and `Fut: Future<Output = Result<T, Box<dyn Error + Send + Sync>>> + Send`
2. WHEN a user passes an async closure to `step_named`, THE DurableContext SHALL accept the closure using the same F_Fut_Pattern as `step`
3. WHEN a user passes an async closure to `wait_for_condition`, THE DurableContext SHALL accept the closure using the F_Fut_Pattern with bounds `F: Fn(&S, &WaitForConditionContext) -> Fut + Send + Sync` and `Fut: Future<Output = Result<T, Box<dyn Error + Send + Sync>>> + Send`
4. WHEN the async closure is invoked during first execution, THE Step_Handler SHALL `.await` the returned future to obtain the result

### Requirement 2: StepFn Trait Removal

**User Story:** As a SDK maintainer, I want the synchronous `StepFn` trait alias removed, so that the API consistently uses the F_Fut_Pattern matching other async methods in the SDK.

#### Acceptance Criteria

1. WHEN the feature is implemented, THE SDK SHALL remove or deprecate the synchronous StepFn trait alias from `traits.rs`
2. WHEN StepFn is removed, THE SDK SHALL replace all call sites that used `F: StepFn<T>` with explicit F_Fut_Pattern bounds

### Requirement 3: Replay Equivalence

**User Story:** As a developer, I want replay behavior to remain identical after the async change, so that my existing workflows continue to function correctly during replay.

#### Acceptance Criteria

1. WHEN a SUCCEED checkpoint exists for an operation ID, THE Step_Handler SHALL return the cached result without invoking the async closure
2. WHEN a FAIL checkpoint exists for an operation ID, THE Step_Handler SHALL return the cached error without invoking the async closure
3. WHEN a SUCCEED checkpoint exists for a wait_for_condition operation, THE WFC_Handler SHALL return the cached result without invoking the async check closure
4. WHEN a FAIL checkpoint exists for a wait_for_condition operation, THE WFC_Handler SHALL return the cached error without invoking the async check closure

### Requirement 4: Non-Determinism Detection

**User Story:** As a developer, I want the SDK to detect non-deterministic replay scenarios, so that I am alerted when workflow code changes between executions.

#### Acceptance Criteria

1. WHEN a checkpoint exists with a different OperationType than Step for a step operation, THE Step_Handler SHALL return a NonDeterministic error without invoking the async closure

### Requirement 5: Step Execution Semantics Preservation

**User Story:** As a developer, I want at-most-once and at-least-once step semantics to be preserved with async closures, so that the durability guarantees remain unchanged.

#### Acceptance Criteria

1. WHILE AtMostOnce_Semantics is configured, THE Step_Handler SHALL create a START checkpoint before invoking the async closure
2. WHILE AtLeastOnce_Semantics is configured, THE Step_Handler SHALL invoke the async closure before creating any checkpoint
3. WHEN the async closure succeeds, THE Step_Handler SHALL serialize the result and create a SUCCEED checkpoint
4. WHEN the async closure returns an error, THE Step_Handler SHALL wrap the error in a FAIL checkpoint

### Requirement 6: Closure Consumption Guarantees

**User Story:** As a developer, I want the type system to enforce correct closure usage, so that step closures are consumed once and condition-check closures remain callable across polls.

#### Acceptance Criteria

1. THE DurableContext SHALL require `step` and `step_named` closures to be `FnOnce`, ensuring the closure is consumed on first execution
2. THE DurableContext SHALL require `wait_for_condition` check closures to be `Fn` (not `FnOnce`), allowing the closure to be called on each poll attempt across Lambda invocations

### Requirement 7: Checkpoint Format Compatibility

**User Story:** As a developer, I want checkpoints created by async steps to be identical in format to those from sync steps, so that existing checkpoint histories remain replayable.

#### Acceptance Criteria

1. THE Step_Handler SHALL use the same OperationType::Step for checkpoints created by async closures as previously used by sync closures
2. THE Step_Handler SHALL use the same JSON serialization format for checkpoint payloads as the previous sync implementation
3. WHEN replaying checkpoints created by the previous sync implementation, THE Step_Handler SHALL deserialize and return the cached values correctly

### Requirement 8: Thread Safety Bounds

**User Story:** As a developer, I want the SDK to enforce thread safety for async closures, so that closures and their futures are safe to use in the tokio async runtime.

#### Acceptance Criteria

1. THE DurableContext SHALL require all async closures passed to `step` and `step_named` to be `Send`
2. THE DurableContext SHALL require all futures returned by async closures to be `Send`
3. THE DurableContext SHALL require `wait_for_condition` check closures to be both `Send` and `Sync`

### Requirement 9: Operation ID Determinism

**User Story:** As a developer, I want operation IDs to be generated identically regardless of closure type, so that deterministic replay ordering is preserved.

#### Acceptance Criteria

1. THE DurableContext SHALL generate operation IDs for async step operations using the same `next_operation_identifier` mechanism as the previous sync implementation

### Requirement 10: Wait For Condition Async Polling

**User Story:** As a developer, I want `wait_for_condition` to support async check closures, so that I can poll external async APIs during condition checks.

#### Acceptance Criteria

1. WHEN the async check closure returns `Ok(result)`, THE WFC_Handler SHALL create a SUCCEED checkpoint with the serialized result
2. WHEN the async check closure returns an error and the wait strategy returns Continue, THE WFC_Handler SHALL create a RETRY checkpoint with the next state and delay, then return a Suspend error
3. WHEN the async check closure returns an error and the wait strategy returns Done, THE WFC_Handler SHALL create a FAIL checkpoint and return an Execution error
4. THE WFC_Handler SHALL call the async check closure at most once per Lambda invocation

### Requirement 11: Error Handling

**User Story:** As a developer, I want async closure errors to be handled consistently with the previous sync behavior, so that error recovery and replay work as expected.

#### Acceptance Criteria

1. WHEN an async closure returns `Err(Box<dyn Error + Send + Sync>)`, THE Step_Handler SHALL wrap the error in `DurableError::UserCode` with the error message
2. WHEN an async closure returns an error, THE Step_Handler SHALL checkpoint the error as a FAIL operation
3. IF the checkpoint creation fails after a successful async closure execution, THEN THE Step_Handler SHALL return a `DurableError::Checkpoint` error
4. IF an async closure panics during `.await`, THEN THE Step_Handler SHALL allow the panic to propagate through the tokio runtime

### Requirement 12: Zero New Dependencies

**User Story:** As a SDK maintainer, I want the async step feature to use only existing dependencies, so that the SDK footprint remains unchanged.

#### Acceptance Criteria

1. THE SDK SHALL implement async step support using only `std::future::Future` from the standard library and the existing `tokio` workspace dependency
