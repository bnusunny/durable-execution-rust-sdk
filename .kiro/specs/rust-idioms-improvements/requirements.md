# Requirements Document

## Introduction

This specification covers improvements to the AWS Durable Execution SDK for Rust by leveraging idiomatic Rust language features. The improvements focus on type safety, API clarity, and performance optimizations that can be implemented incrementally without breaking changes.

## Glossary

- **Newtype**: A Rust pattern where a single-field tuple struct wraps another type to provide type safety and semantic meaning
- **Trait_Alias**: A pattern using blanket implementations to create shorthand for common trait bound combinations
- **Sealed_Trait**: A trait that cannot be implemented outside its defining crate, using a private supertrait
- **Memory_Ordering**: Atomic operation synchronization guarantees (Relaxed, Acquire, Release, SeqCst)
- **OperationId**: A unique identifier for an operation within a durable execution
- **ExecutionArn**: The Amazon Resource Name identifying a durable execution
- **CallbackId**: A unique identifier for a callback operation
- **SDK**: The AWS Durable Execution SDK for Rust

## Requirements

### Requirement 1: Newtype Pattern for Domain Identifiers

**User Story:** As a developer using the SDK, I want type-safe domain identifiers, so that the compiler prevents me from accidentally mixing different ID types.

#### Acceptance Criteria

1. THE SDK SHALL provide an `OperationId` newtype wrapping `String` for operation identifiers
2. THE SDK SHALL provide an `ExecutionArn` newtype wrapping `String` for execution ARNs
3. THE SDK SHALL provide an `CallbackId` newtype wrapping `String` for callback identifiers
4. WHEN a newtype is created, THE SDK SHALL validate the value is non-empty
5. THE SDK SHALL implement `Display`, `Debug`, `Clone`, `PartialEq`, `Eq`, `Hash` for all newtypes
6. THE SDK SHALL implement `AsRef<str>` and `Deref<Target=str>` for convenient string access
7. THE SDK SHALL implement `From<String>` and `From<&str>` for easy construction
8. THE SDK SHALL implement `Serialize` and `Deserialize` for all newtypes to maintain JSON compatibility
9. THE SDK SHALL update internal APIs to use newtypes while maintaining backward compatibility via `impl Into<NewType>` bounds

### Requirement 2: Trait Aliases for Common Bounds

**User Story:** As a developer extending the SDK, I want cleaner trait bounds, so that function signatures are more readable and maintainable.

#### Acceptance Criteria

1. THE SDK SHALL provide a `DurableValue` trait alias for `Serialize + DeserializeOwned + Send`
2. THE SDK SHALL provide a `StepFn` trait alias for step function bounds
3. WHEN defining public APIs, THE SDK SHALL use trait aliases instead of repeated bound combinations
4. THE SDK SHALL document trait aliases with examples showing equivalent expanded bounds
5. THE SDK SHALL ensure trait aliases work correctly with generic type inference

**Implementation Note:** The original design proposed `DurableValue: Serialize + DeserializeOwned + Send + Sync + 'static`, but this was adjusted to `Serialize + DeserializeOwned + Send` to maintain backward compatibility with existing code that uses closures borrowing from the environment.

### Requirement 3: Sealed Traits for Internal Implementations

**User Story:** As an SDK maintainer, I want to prevent external implementations of core traits, so that I can evolve the API without breaking external code.

#### Acceptance Criteria

1. THE SDK SHALL implement the sealed trait pattern for the `Logger` trait
2. THE SDK SHALL implement the sealed trait pattern for the `SerDes` trait
3. THE SDK SHALL implement the sealed trait pattern for the `RetryStrategy` trait
4. WHEN a trait is sealed, THE SDK SHALL use a private `Sealed` supertrait in a private module
5. THE SDK SHALL document that these traits are sealed and cannot be implemented externally
6. THE SDK SHALL provide factory functions or builders for users who need custom behavior

### Requirement 4: Atomic Memory Ordering Optimization

**User Story:** As a performance-conscious developer, I want the SDK to use appropriate memory orderings, so that atomic operations have minimal overhead.

#### Acceptance Criteria

1. WHEN incrementing simple counters without synchronization requirements, THE SDK SHALL use `Ordering::Relaxed`
2. WHEN storing values that other threads will read, THE SDK SHALL use `Ordering::Release`
3. WHEN loading values that were stored by other threads, THE SDK SHALL use `Ordering::Acquire`
4. WHEN both storing and loading require synchronization, THE SDK SHALL use `Ordering::AcqRel`
5. THE SDK SHALL only use `Ordering::SeqCst` when sequential consistency is required
6. THE SDK SHALL document the memory ordering choice for each atomic operation
7. THE SDK SHALL maintain correctness guarantees while optimizing ordering

### Requirement 5: Result Type Aliases

**User Story:** As a developer using the SDK, I want semantic result types, so that function signatures clearly communicate their purpose.

#### Acceptance Criteria

1. THE SDK SHALL provide a `DurableResult<T>` type alias for `Result<T, DurableError>`
2. THE SDK SHALL provide a `StepResult<T>` type alias for step operation results
3. THE SDK SHALL provide a `CheckpointResult<T>` type alias for checkpoint operation results
4. THE SDK SHALL use these type aliases consistently in public APIs
5. THE SDK SHALL document type aliases with their expanded forms

### Requirement 6: Enum Discriminant Optimization

**User Story:** As a performance-conscious developer, I want compact enum representations, so that memory usage is minimized.

#### Acceptance Criteria

1. THE SDK SHALL add `#[repr(u8)]` to `OperationStatus` enum for compact representation
2. THE SDK SHALL add `#[repr(u8)]` to `OperationType` enum for compact representation
3. THE SDK SHALL add `#[repr(u8)]` to `OperationAction` enum for compact representation
4. THE SDK SHALL add `#[repr(u8)]` to `TerminationReason` enum for compact representation
5. WHEN adding repr attributes, THE SDK SHALL assign explicit discriminant values for stability
6. THE SDK SHALL ensure serde serialization continues to use string representations for JSON compatibility
7. THE SDK SHALL add tests verifying enum size is 1 byte after optimization
