# Migration Guide: Rust Idioms Improvements

This guide documents the Rust idiomatic improvements introduced in the SDK and how to migrate existing code to use the new features.

## Overview

The SDK has been enhanced with several Rust idiomatic improvements:

1. **Newtype Pattern** - Type-safe domain identifiers
2. **Trait Aliases** - Cleaner function signatures
3. **Sealed Traits** - API stability guarantees
4. **Result Type Aliases** - Semantic result types
5. **Enum Optimizations** - Compact memory representation
6. **Atomic Optimizations** - Improved performance

## Backward Compatibility

**All changes are backward compatible.** Existing code will continue to work without modification. The new features are additive and opt-in.

## 1. Newtype Pattern for Domain Identifiers

### What Changed

The SDK now provides type-safe wrappers for string identifiers:

- `OperationId` - Operation identifiers
- `ExecutionArn` - Execution ARNs
- `CallbackId` - Callback identifiers

### Migration

**Before (still works):**
```rust
let operation_id: String = "op-123".to_string();
let execution_arn: String = "arn:aws:lambda:...".to_string();
```

**After (recommended):**
```rust
use aws_durable_execution_sdk::{OperationId, ExecutionArn, CallbackId};

// From String or &str (no validation)
let operation_id = OperationId::from("op-123");
let execution_arn = ExecutionArn::from("arn:aws:lambda:us-east-1:123456789012:function:my-func:durable:abc123");

// With validation
let operation_id = OperationId::new("op-123").unwrap();
let execution_arn = ExecutionArn::new("arn:aws:lambda:us-east-1:123456789012:function:my-func:durable:abc123").unwrap();
```

### Benefits

- **Compile-time safety**: Can't accidentally pass an `ExecutionArn` where an `OperationId` is expected
- **Validation**: `new()` methods validate input (e.g., non-empty, valid ARN format)
- **Transparent serialization**: JSON serialization unchanged (uses `#[serde(transparent)]`)

### Using Newtypes as Strings

All newtypes implement `Deref<Target=str>` and `AsRef<str>`:

```rust
let op_id = OperationId::from("op-123");

// String methods work directly
assert!(op_id.starts_with("op-"));
assert_eq!(op_id.len(), 6);

// Convert to &str
let s: &str = op_id.as_ref();
let s: &str = &*op_id;  // via Deref
```

### Using Newtypes in Collections

```rust
use std::collections::HashMap;

let mut map: HashMap<OperationId, String> = HashMap::new();
map.insert(OperationId::from("op-1"), "result".to_string());
```

## 2. Trait Aliases

### What Changed

Common trait bound combinations are now available as trait aliases:

- `DurableValue` = `Serialize + DeserializeOwned + Send`
- `StepFn<T>` = `FnOnce(StepContext) -> Result<T, Box<dyn Error + Send + Sync>> + Send`

### Migration

**Before:**
```rust
fn process<T>(value: T) 
where 
    T: serde::Serialize + serde::de::DeserializeOwned + Send 
{
    // ...
}

fn execute_step<T, F>(func: F)
where
    T: serde::Serialize + serde::de::DeserializeOwned + Send,
    F: FnOnce(StepContext) -> Result<T, Box<dyn std::error::Error + Send + Sync>> + Send,
{
    // ...
}
```

**After (recommended):**
```rust
use aws_durable_execution_sdk::{DurableValue, StepFn};

fn process<T: DurableValue>(value: T) {
    // ...
}

fn execute_step<T: DurableValue, F: StepFn<T>>(func: F) {
    // ...
}
```

### Benefits

- **Cleaner signatures**: Function signatures are more readable
- **Consistent bounds**: All step-related functions use the same trait aliases
- **Self-documenting**: Trait names describe their purpose

## 3. Sealed Traits

### What Changed

The following traits are now sealed and cannot be implemented outside the SDK:

- `Logger` - Structured logging
- `SerDes` - Serialization/deserialization

### Migration

If you were implementing these traits directly, use the factory functions instead:

**Before (no longer possible):**
```rust
struct MyLogger;

impl Logger for MyLogger {
    fn debug(&self, message: &str, info: &LogInfo) { /* ... */ }
    fn info(&self, message: &str, info: &LogInfo) { /* ... */ }
    fn warn(&self, message: &str, info: &LogInfo) { /* ... */ }
    fn error(&self, message: &str, info: &LogInfo) { /* ... */ }
}
```

**After:**
```rust
use aws_durable_execution_sdk::{custom_logger, simple_custom_logger, LogInfo};

// Simple logger with one function for all levels
let logger = simple_custom_logger(|level, msg, info| {
    println!("[{}] {}: {:?}", level, msg, info);
});

// Full custom logger with separate functions
let logger = custom_logger(
    |msg, info| println!("[DEBUG] {}", msg),
    |msg, info| println!("[INFO] {}", msg),
    |msg, info| println!("[WARN] {}", msg),
    |msg, info| println!("[ERROR] {}", msg),
);
```

**For SerDes:**
```rust
use aws_durable_execution_sdk::serdes::{custom_serdes, SerDesContext, SerDesError};

let serdes = custom_serdes::<MyType, _, _>(
    |value, ctx| { /* serialize */ Ok(serde_json::to_string(value)?) },
    |data, ctx| { /* deserialize */ Ok(serde_json::from_str(data)?) },
);
```

### Benefits

- **API stability**: SDK can evolve without breaking external implementations
- **Flexibility**: Factory functions provide the same customization capabilities

## 4. Result Type Aliases

### What Changed

Semantic result type aliases are now available:

- `DurableResult<T>` = `Result<T, DurableError>`
- `StepResult<T>` = `Result<T, DurableError>`
- `CheckpointResult<T>` = `Result<T, DurableError>`

### Migration

**Before:**
```rust
fn process_order(order_id: &str) -> Result<String, DurableError> {
    Ok(format!("Processed: {}", order_id))
}
```

**After (recommended):**
```rust
use aws_durable_execution_sdk::DurableResult;

fn process_order(order_id: &str) -> DurableResult<String> {
    Ok(format!("Processed: {}", order_id))
}
```

### Benefits

- **Semantic clarity**: Function signatures communicate their purpose
- **Consistency**: All SDK functions use the same type aliases

## 5. Enum Optimizations

### What Changed

Status enums now use `#[repr(u8)]` for compact memory representation:

- `OperationStatus` - 1 byte (was variable)
- `OperationType` - 1 byte (was variable)
- `OperationAction` - 1 byte (was variable)
- `TerminationReason` - 1 byte (was variable)

### Migration

**No migration required.** This is a transparent optimization.

### Benefits

- **Memory efficiency**: Each enum instance uses only 1 byte
- **Cache efficiency**: More enum values fit in CPU cache
- **Serialization unchanged**: JSON still uses string representations

## 6. Atomic Optimizations

### What Changed

Atomic operations now use appropriate memory orderings:

- Step counters: `Ordering::Relaxed` (was `SeqCst`)
- Replay flags: `Ordering::Acquire`/`Release` (was `SeqCst`)
- Checkpoint tokens: `Ordering::AcqRel` (was `SeqCst`)

### Migration

**No migration required.** This is a transparent optimization.

### Benefits

- **Performance**: Reduced synchronization overhead
- **Correctness preserved**: All synchronization requirements are still met

## Summary

| Feature | Breaking Change | Migration Required |
|---------|-----------------|-------------------|
| Newtypes | No | Optional (recommended) |
| Trait Aliases | No | Optional (recommended) |
| Sealed Traits | Yes* | Only if implementing traits directly |
| Result Aliases | No | Optional (recommended) |
| Enum Repr | No | None |
| Atomic Ordering | No | None |

*Sealed traits are a breaking change only for code that directly implements `Logger` or `SerDes`. Use the factory functions instead.

## Questions?

If you encounter any issues during migration, please open an issue on the GitHub repository.
