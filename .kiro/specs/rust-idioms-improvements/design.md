# Design Document: Rust Idioms Improvements

## Overview

This document describes the technical design for implementing six Rust idiomatic improvements to the AWS Durable Execution SDK. These improvements leverage Rust language features to enhance type safety, API clarity, and performance without breaking changes.

## Design Decisions

### 1. Newtype Pattern for Domain Identifiers

#### Current State
The SDK uses raw `String` types for domain identifiers:
- `operation_id: String`
- `durable_execution_arn: String` (in ExecutionState)
- `callback_id: String`

This allows accidental mixing of different ID types at compile time.

#### Proposed Design

Create newtype wrappers in a new `src/types.rs` module:

```rust
/// A unique identifier for an operation within a durable execution.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OperationId(String);

/// The Amazon Resource Name identifying a durable execution.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExecutionArn(String);

/// A unique identifier for a callback operation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CallbackId(String);
```

Each newtype will implement:
- `Display`, `Debug`, `Clone`, `PartialEq`, `Eq`, `Hash`
- `AsRef<str>` and `Deref<Target=str>` for string access
- `From<String>`, `From<&str>`, `TryFrom` with validation
- `Serialize`/`Deserialize` with `#[serde(transparent)]`

#### Backward Compatibility Strategy
- Accept `impl Into<OperationId>` in public APIs
- Implement `From<String>` for seamless migration
- Existing code using `String` will continue to work

### 2. Trait Aliases for Common Bounds

#### Current State
Common trait bound combinations are repeated throughout the codebase:
- `Serialize + DeserializeOwned + Send`
- Step function bounds: `FnOnce(StepContext) -> Result<T, Box<dyn Error + Send + Sync>> + Send`

#### Implemented Design

Created trait aliases in `src/traits.rs`:

```rust
/// Trait alias for values that can be durably stored and retrieved.
///
/// This combines the necessary bounds for serialization, deserialization,
/// and thread-safe sending across async boundaries.
pub trait DurableValue: Serialize + DeserializeOwned + Send {}

// Blanket implementation
impl<T> DurableValue for T where T: Serialize + DeserializeOwned + Send {}

/// Trait alias for step function bounds.
pub trait StepFn<T>: FnOnce(StepContext) -> Result<T, Box<dyn std::error::Error + Send + Sync>> + Send {}

impl<T, F> StepFn<T> for F where F: FnOnce(StepContext) -> Result<T, Box<dyn std::error::Error + Send + Sync>> + Send {}
```

#### Design Decision: No `'static` or `Sync` Requirements

The original design proposed `DurableValue: Serialize + DeserializeOwned + Send + Sync + 'static` and `StepFn` with `+ 'static`. However, during implementation, we discovered this would be a **breaking change** because:

1. **Closures borrowing from environment**: The existing API allows closures to borrow from the surrounding scope. Adding `'static` would require all closures to use `move` and own their captured values.

2. **Backward compatibility**: The original `step` method had bounds `T: Serialize + DeserializeOwned + Send` (without `Sync` or `'static`). Changing this would break existing code.

3. **Example code**: All existing examples use closures that borrow from the environment, which would fail to compile with `'static` bounds.

**Final bounds match the original API:**
- `DurableValue` = `Serialize + DeserializeOwned + Send`
- `StepFn<T>` = `FnOnce(StepContext) -> Result<T, Box<dyn Error + Send + Sync>> + Send`

#### Usage
```rust
// Before
pub async fn step<T, F>(&self, f: F, config: Option<StepConfig>) -> Result<T, DurableError>
where
    T: serde::Serialize + serde::de::DeserializeOwned + Send,
    F: FnOnce(StepContext) -> Result<T, Box<dyn std::error::Error + Send + Sync>> + Send,

// After
pub async fn step<T: DurableValue, F: StepFn<T>>(&self, f: F, config: Option<StepConfig>) -> Result<T, DurableError>
```

#### Benefits
- **Cleaner signatures**: Function signatures are more readable
- **Consistent bounds**: All step-related functions use the same trait aliases
- **Documentation**: Trait aliases serve as documentation for the required bounds
- **No breaking changes**: Existing code continues to work without modification

### 3. Sealed Traits for Internal Implementations

#### Current State
Traits like `Logger`, `SerDes`, and `RetryStrategy` can be implemented by external code, which limits API evolution.

#### Proposed Design

Use the sealed trait pattern with a private module:

```rust
// In src/sealed.rs (private module)
pub trait Sealed {}

// In src/context.rs
mod private {
    pub trait Sealed {}
}

/// Logger trait for structured logging in durable executions.
///
/// This trait is sealed and cannot be implemented outside this crate.
/// Use the provided implementations or factory functions for custom behavior.
pub trait Logger: private::Sealed + Send + Sync {
    fn debug(&self, message: &str, info: &LogInfo);
    fn info(&self, message: &str, info: &LogInfo);
    fn warn(&self, message: &str, info: &LogInfo);
    fn error(&self, message: &str, info: &LogInfo);
}

// Implement Sealed for all provided logger types
impl private::Sealed for TracingLogger {}
impl private::Sealed for ReplayAwareLogger {}
```

#### Factory Functions
Provide factory functions for users who need custom behavior:
```rust
/// Creates a custom logger that wraps a closure.
pub fn custom_logger<F>(f: F) -> impl Logger
where
    F: Fn(&str, &LogInfo) + Send + Sync + 'static
```

### 4. Atomic Memory Ordering Optimization

#### Current State
The SDK uses `Ordering::SeqCst` for all atomic operations:
```rust
let counter = self.step_counter.fetch_add(1, Ordering::SeqCst);
```

#### Proposed Design

Use appropriate orderings based on synchronization requirements:

| Operation | Current | Proposed | Rationale |
|-----------|---------|----------|-----------|
| Step counter increment | SeqCst | Relaxed | No synchronization needed, just unique values |
| Replay flag read | SeqCst | Acquire | Need to see writes from other threads |
| Replay flag write | SeqCst | Release | Need writes to be visible to readers |
| Checkpoint token update | SeqCst | AcqRel | Both read and write synchronization |

```rust
// Step counter - only needs uniqueness, not synchronization
let counter = self.step_counter.fetch_add(1, Ordering::Relaxed);

// Replay flag - needs acquire/release semantics
pub fn is_replay(&self) -> bool {
    self.is_replay.load(Ordering::Acquire)
}

pub fn set_replay(&self, value: bool) {
    self.is_replay.store(value, Ordering::Release);
}
```

### 5. Result Type Aliases

#### Current State
Result types are verbose:
```rust
fn step<T>(...) -> Result<T, DurableError>
fn checkpoint(...) -> Result<CheckpointResponse, DurableError>
```

#### Proposed Design

Define semantic type aliases in `src/error.rs`:

```rust
/// Result type for durable operations.
pub type DurableResult<T> = Result<T, DurableError>;

/// Result type for step operations.
pub type StepResult<T> = Result<T, DurableError>;

/// Result type for checkpoint operations.
pub type CheckpointResult<T> = Result<T, DurableError>;
```

#### Usage
```rust
// Before
pub async fn step<T>(&self, f: F, config: Option<StepConfig>) -> Result<T, DurableError>

// After
pub async fn step<T>(&self, f: F, config: Option<StepConfig>) -> DurableResult<T>
```

### 6. Enum Discriminant Optimization

#### Current State
Enums use default representation:
```rust
pub enum OperationStatus {
    Started,
    Pending,
    Ready,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
    Stopped,
}
```

#### Proposed Design

Add `#[repr(u8)]` with explicit discriminants:

```rust
/// The status of an operation in a durable execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum OperationStatus {
    Started = 0,
    Pending = 1,
    Ready = 2,
    Succeeded = 3,
    Failed = 4,
    Cancelled = 5,
    TimedOut = 6,
    Stopped = 7,
}
```

Apply to:
- `OperationStatus` (8 variants → 1 byte)
- `OperationType` (6 variants → 1 byte)
- `OperationAction` (5 variants → 1 byte)
- `TerminationReason` (9 variants → 1 byte)

#### Serde Compatibility
The `#[serde(rename = "...")]` attributes ensure JSON serialization continues to use string representations.

## Module Structure

```
src/
├── types.rs          # NEW: Newtype definitions (OperationId, ExecutionArn, CallbackId)
├── traits.rs         # NEW: Trait aliases (DurableValue, StepFn)
├── sealed.rs         # NEW: Private sealed trait module
├── error.rs          # MODIFIED: Add result type aliases
├── operation.rs      # MODIFIED: Add #[repr(u8)] to enums
├── context.rs        # MODIFIED: Use newtypes and trait aliases
├── state/
│   └── execution_state.rs  # MODIFIED: Optimize atomic orderings
└── lib.rs            # MODIFIED: Re-export new types
```

## Testing Strategy

### Unit Tests
- Newtype construction and validation
- Trait alias type inference
- Enum size verification (`std::mem::size_of`)
- Serde round-trip for newtypes

### Property Tests
- Newtype serialization round-trip
- Operation ID generation with newtypes

### Integration Tests
- Backward compatibility with existing code using `String`
- API ergonomics with new types

## Migration Path

1. **Phase 1**: Add new types alongside existing code
   - Create `types.rs`, `traits.rs`, `sealed.rs`
   - Add result type aliases
   - Add `#[repr(u8)]` to enums

2. **Phase 2**: Update internal usage
   - Use newtypes in internal APIs
   - Apply trait aliases to public APIs
   - Optimize atomic orderings

3. **Phase 3**: Documentation
   - Update rustdoc with examples
   - Add migration guide for users

## Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Breaking changes for users | Use `impl Into<NewType>` bounds for backward compatibility |
| Sealed traits limit extensibility | Provide factory functions for custom behavior |
| Memory ordering bugs | Careful analysis of synchronization requirements; add comments |
| Enum repr changes affect serialization | Verify serde continues to use string representation |

## Dependencies

No new dependencies required. Uses only:
- `serde` (existing)
- `std::sync::atomic` (existing)
- `std::ops::Deref` (std)
