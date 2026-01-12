# Implementation Plan: Rust Idioms Improvements

## Overview

This implementation plan breaks down the six Rust idiomatic improvements into incremental tasks. The approach starts with foundational types and progresses through trait aliases, sealed traits, atomic optimizations, result aliases, and enum optimizations.

## Tasks

- [x] 1. Implement Newtype Pattern for Domain Identifiers
  - [x] 1.1 Create types module with OperationId newtype
    - Create `src/types.rs` module
    - Implement `OperationId` struct wrapping `String`
    - Implement `Display`, `Debug`, `Clone`, `PartialEq`, `Eq`, `Hash`
    - Implement `AsRef<str>` and `Deref<Target=str>`
    - Implement `From<String>` and `From<&str>`
    - Implement `Serialize`/`Deserialize` with `#[serde(transparent)]`
    - Add validation for non-empty values in `TryFrom`
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.8_

  - [x] 1.2 Implement ExecutionArn newtype
    - Add `ExecutionArn` struct to `src/types.rs`
    - Implement all required traits (same as OperationId)
    - Add ARN format validation in `TryFrom`
    - _Requirements: 1.1, 1.2, 1.5, 1.6, 1.7, 1.8_

  - [x] 1.3 Implement CallbackId newtype
    - Add `CallbackId` struct to `src/types.rs`
    - Implement all required traits (same as OperationId)
    - _Requirements: 1.1, 1.3, 1.5, 1.6, 1.7, 1.8_

  - [x] 1.4 Write unit tests for newtypes
    - Test construction from String and &str
    - Test validation rejects empty strings
    - Test Display and Debug formatting
    - Test Deref and AsRef access
    - Test serde round-trip serialization
    - Test Hash and Eq for HashMap usage

  - [x] 1.5 Update internal APIs to use newtypes
    - Update `OperationIdentifier` to use `OperationId`
    - Update `ExecutionState` to use `ExecutionArn`
    - Update callback handler to use `CallbackId`
    - Use `impl Into<NewType>` bounds for backward compatibility
    - _Requirements: 1.9_

  - [x] 1.6 Re-export newtypes from lib.rs
    - Add `pub mod types;` to lib.rs
    - Re-export `OperationId`, `ExecutionArn`, `CallbackId` at crate root

- [x] 2. Implement Trait Aliases for Common Bounds
  - [x] 2.1 Create traits module with DurableValue alias
    - Create `src/traits.rs` module
    - Define `DurableValue` trait with bounds: `Serialize + DeserializeOwned + Send`
    - Add blanket implementation for all types meeting bounds
    - Add comprehensive documentation with examples
    - **Design Decision**: Use `Serialize + DeserializeOwned + Send` (without `Sync` or `'static`) to maintain backward compatibility with closures that borrow from environment
    - _Requirements: 2.1, 2.4_

  - [x] 2.2 Implement StepFn trait alias
    - Define `StepFn<T>` trait: `FnOnce(StepContext) -> Result<T, Box<dyn Error + Send + Sync>> + Send`
    - Add blanket implementation
    - Document equivalent expanded bounds
    - **Design Decision**: Omit `'static` bound to allow closures to borrow from environment
    - _Requirements: 2.2, 2.4_

  - [x] 2.3 Update public APIs to use trait aliases
    - Update `DurableContext::step` to use `DurableValue` and `StepFn`
    - Update `DurableContext::step_named` to use `DurableValue` and `StepFn`
    - Update `step_handler` and internal functions (`execute_at_most_once`, `execute_at_least_once`)
    - Verify type inference works correctly - all examples compile without changes
    - _Requirements: 2.3, 2.5_

  - [x] 2.4 Write tests for trait alias type inference
    - Add tests for generic functions with trait aliases
    - Add tests for closures that borrow from environment (`test_step_fn_borrowing_closure`)
    - Verify all existing tests pass, confirming backward compatibility
    - Tests in `src/traits.rs`: primitives, collections, custom structs, closures, named functions

  - [x] 2.5 Re-export trait aliases from lib.rs
    - Add `pub mod traits;` to lib.rs
    - Re-export `DurableValue`, `StepFn` at crate root
    - Update module documentation to include traits module

- [x] 3. Implement Sealed Traits for Internal Implementations
  - [x] 3.1 Create sealed module with private Sealed trait
    - Create private `sealed` module in appropriate location
    - Define `pub trait Sealed {}` in private module
    - _Requirements: 3.4_

  - [x] 3.2 Seal the Logger trait
    - Add `private::Sealed` as supertrait to `Logger`
    - Implement `Sealed` for `TracingLogger`
    - Implement `Sealed` for `ReplayAwareLogger`
    - Update documentation to note trait is sealed
    - _Requirements: 3.1, 3.5_

  - [x] 3.3 Seal the SerDes trait
    - Add `private::Sealed` as supertrait to `SerDes`
    - Implement `Sealed` for `JsonSerDes`
    - Update documentation to note trait is sealed
    - _Requirements: 3.2, 3.5_

  - [x] 3.4 Seal the RetryStrategy trait (if exists)
    - Add `private::Sealed` as supertrait
    - Implement `Sealed` for all provided implementations
    - Update documentation
    - _Requirements: 3.3, 3.5_

  - [x] 3.5 Provide factory functions for custom behavior
    - Add `custom_logger` factory function
    - Add `custom_serdes` factory function if needed
    - Document usage patterns for customization
    - _Requirements: 3.6_

  - [x] 3.6 Write tests verifying sealed behavior
    - Verify internal implementations work
    - Document that external implementations are prevented (compile-time)

- [x] 4. Implement Atomic Memory Ordering Optimization
  - [x] 4.1 Analyze current atomic usage
    - Review all `Ordering::SeqCst` usage in codebase
    - Document synchronization requirements for each
    - Identify candidates for relaxed orderings

  - [x] 4.2 Optimize step counter atomics
    - Change `step_counter.fetch_add` to use `Ordering::Relaxed`
    - Add comment explaining why Relaxed is sufficient
    - _Requirements: 4.1, 4.6_

  - [x] 4.3 Optimize replay flag atomics
    - Change replay flag loads to use `Ordering::Acquire`
    - Change replay flag stores to use `Ordering::Release`
    - Add comments explaining acquire/release semantics
    - _Requirements: 4.2, 4.3, 4.6_

  - [x] 4.4 Optimize checkpoint token atomics
    - Use `Ordering::AcqRel` for read-modify-write operations
    - Add comments explaining synchronization requirements
    - _Requirements: 4.4, 4.6_

  - [x] 4.5 Review remaining SeqCst usage
    - Identify any remaining SeqCst that is truly required
    - Document why SeqCst is necessary for those cases
    - _Requirements: 4.5, 4.6_

  - [x] 4.6 Write tests verifying correctness
    - Add concurrent tests for step counter
    - Add concurrent tests for replay flag
    - Verify no race conditions introduced
    - _Requirements: 4.7_

- [x] 5. Implement Result Type Aliases
  - [x] 5.1 Define DurableResult type alias
    - Add `pub type DurableResult<T> = Result<T, DurableError>;` to error.rs
    - Add documentation with expanded form
    - _Requirements: 5.1, 5.5_

  - [x] 5.2 Define StepResult type alias
    - Add `pub type StepResult<T> = Result<T, DurableError>;` to error.rs
    - Add documentation
    - _Requirements: 5.2, 5.5_

  - [x] 5.3 Define CheckpointResult type alias
    - Add `pub type CheckpointResult<T> = Result<T, DurableError>;` to error.rs
    - Add documentation
    - _Requirements: 5.3, 5.5_

  - [x] 5.4 Update public APIs to use type aliases
    - Update `DurableContext` methods to use `DurableResult`
    - Update step handler to use `StepResult`
    - Update checkpoint methods to use `CheckpointResult`
    - _Requirements: 5.4_

  - [x] 5.5 Re-export type aliases from lib.rs
    - Ensure `DurableResult`, `StepResult`, `CheckpointResult` are re-exported

- [x] 6. Implement Enum Discriminant Optimization
  - [x] 6.1 Add repr(u8) to OperationStatus
    - Add `#[repr(u8)]` attribute to `OperationStatus` enum
    - Assign explicit discriminant values (0-7)
    - Verify serde serialization unchanged
    - _Requirements: 6.1, 6.5, 6.6_

  - [x] 6.2 Add repr(u8) to OperationType
    - Add `#[repr(u8)]` attribute to `OperationType` enum
    - Assign explicit discriminant values (0-5)
    - Verify serde serialization unchanged
    - _Requirements: 6.2, 6.5, 6.6_

  - [x] 6.3 Add repr(u8) to OperationAction
    - Add `#[repr(u8)]` attribute to `OperationAction` enum
    - Assign explicit discriminant values (0-4)
    - Verify serde serialization unchanged
    - _Requirements: 6.3, 6.5, 6.6_

  - [x] 6.4 Add repr(u8) to TerminationReason
    - Add `#[repr(u8)]` attribute to `TerminationReason` enum
    - Assign explicit discriminant values (0-8)
    - Verify serde serialization unchanged
    - _Requirements: 6.4, 6.5, 6.6_

  - [x] 6.5 Write size verification tests
    - Add tests using `std::mem::size_of::<EnumType>()`
    - Verify each enum is 1 byte after optimization
    - _Requirements: 6.7_

  - [x] 6.6 Write serde compatibility tests
    - Verify JSON serialization uses string representations
    - Test round-trip serialization/deserialization
    - _Requirements: 6.6_

- [x] 7. Documentation and Final Verification
  - [x] 7.1 Update crate-level documentation
    - Document new types in lib.rs overview
    - Add examples using newtypes and trait aliases
    - Document sealed trait pattern and factory functions

  - [x] 7.2 Add migration guide
    - Document backward compatibility guarantees
    - Provide examples of migrating from String to newtypes
    - Document any breaking changes (if any)

  - [x] 7.3 Run full test suite
    - Ensure all existing tests pass
    - Run clippy and fix any warnings
    - Verify documentation examples compile

  - [x] 7.4 Verify backward compatibility
    - Test that existing code using String compiles
    - Test that existing serialized data deserializes correctly
    - Verify no runtime behavior changes

## Notes

- Tasks are ordered by dependency: newtypes first, then traits that may use them
- Each task includes requirement traceability to requirements.md
- Backward compatibility is maintained throughout via `impl Into<T>` bounds
- Sealed traits require careful documentation to guide users toward factory functions
- Atomic ordering changes require careful analysis to avoid introducing bugs
- Enum repr changes should not affect serialization due to serde rename attributes

### Design Decisions

**Task 2 - Trait Aliases**: The trait aliases use bounds that match the original API to maintain backward compatibility:
- `DurableValue` = `Serialize + DeserializeOwned + Send` (without `Sync` or `'static`)
- `StepFn<T>` = `FnOnce(StepContext) -> Result<T, Box<dyn Error + Send + Sync>> + Send` (without `'static`)

This allows existing closures that borrow from the surrounding scope to continue working. Adding `'static` would require all closures to use `move` and own their captured values, breaking existing code.
