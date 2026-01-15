# Design Document: Promise Combinator Macros

## Overview

This document describes the technical design for implementing ergonomic macros (`all!`, `any!`, `race!`, `all_settled!`) that enable combining heterogeneous futures in the AWS Durable Execution SDK for Rust.

## Architecture

The macros are implemented as declarative macros (`macro_rules!`) that:
1. Accept a context and variadic futures
2. Box each future to erase concrete types
3. Delegate to the existing method-based API

```
┌─────────────────────────────────────────────────────────────┐
│                      User Code                              │
│  all!(ctx, ctx.step(...), ctx.step(...), ctx.step(...))     │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                    Macro Expansion                          │
│  - Box::pin(fut1), Box::pin(fut2), Box::pin(fut3)           │
│  - Collect into Vec<Pin<Box<dyn Future<...>>>>              │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                  Existing Method API                        │
│  ctx.all(boxed_futures)                                     │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                  Promise Handler                            │
│  all_handler / any_handler / race_handler / all_settled     │
└─────────────────────────────────────────────────────────────┘
```

## Components and Interfaces

### Macro Definitions

All macros follow the same pattern:

```rust
#[macro_export]
macro_rules! all {
    ($ctx:expr, $($fut:expr),+ $(,)?) => {{
        let futures: ::std::vec::Vec<
            ::std::pin::Pin<
                ::std::boxed::Box<
                    dyn ::std::future::Future<
                        Output = $crate::error::DurableResult<_>
                    > + ::std::marker::Send
                >
            >
        > = ::std::vec![
            $(::std::boxed::Box::pin($fut)),+
        ];
        $ctx.all(futures)
    }};
}
```

### Macro Syntax

Each macro accepts:
- `$ctx:expr` - The DurableContext instance
- `$($fut:expr),+` - One or more future expressions
- `$(,)?` - Optional trailing comma

### Type Erasure Strategy

The macros use `Box::pin()` to convert each concrete future type into a trait object:

```rust
// Before (doesn't compile - different types):
vec![
    ctx.step(|_| Ok(1), None),  // Type A
    ctx.step(|_| Ok(2), None),  // Type B
]

// After (compiles - same trait object type):
vec![
    Box::pin(ctx.step(|_| Ok(1), None)),  // Pin<Box<dyn Future<...>>>
    Box::pin(ctx.step(|_| Ok(2), None)),  // Pin<Box<dyn Future<...>>>
]
```

## Data Models

No new data models are required. The macros use existing types:

- `DurableResult<T>` - Result type alias
- `BatchResult<T>` - Result type for `all_settled`
- `Pin<Box<dyn Future<Output = DurableResult<T>> + Send>>` - Boxed future type

## Correctness Properties

*A property is a characteristic or behavior that should hold true across all valid executions of a system-essentially, a formal statement about what the system should do. Properties serve as the bridge between human-readable specifications and machine-verifiable correctness guarantees.*

### Property 1: Macro expansion equivalence
*For any* set of futures passed to a macro, the macro expansion SHALL produce the same result as manually boxing the futures and calling the corresponding method.
**Validates: Requirements 1.1, 2.1, 3.1, 4.1**

### Property 2: Result order preservation
*For any* ordered sequence of futures passed to `all!` or `all_settled!`, the results SHALL be returned in the same order as the input futures.
**Validates: Requirements 1.5, 4.4**

### Property 3: Type inference consistency
*For any* set of futures with the same output type `T`, the macro SHALL successfully infer the type without explicit annotation.
**Validates: Requirements 6.1, 6.2**

### Property 4: Compile-time type mismatch detection
*For any* set of futures with incompatible output types, the macro SHALL produce a compile-time error.
**Validates: Requirements 6.1**

## Error Handling

The macros delegate error handling to the underlying method implementations:

| Macro | Success | Failure |
|-------|---------|---------|
| `all!` | `Ok(Vec<T>)` | `Err` on first failure |
| `any!` | `Ok(T)` on first success | `Err` if all fail |
| `race!` | First result (success or failure) | First result (success or failure) |
| `all_settled!` | `Ok(BatchResult<T>)` | Never fails (collects all outcomes) |

## Testing Strategy

### Unit Tests

1. **Basic functionality**: Test each macro with simple futures
2. **Multiple futures**: Test with varying numbers of futures (1, 2, 5, 10)
3. **Mixed success/failure**: Test error propagation behavior
4. **Type inference**: Test that type inference works correctly

### Property-Based Tests

1. **Result order preservation**: Generate random futures and verify order is preserved
2. **Equivalence with manual boxing**: Compare macro results with manually boxed futures

### Compile-Time Tests

1. **Type mismatch detection**: Verify compile errors for incompatible types (using `trybuild` or similar)
2. **Minimum futures**: Verify at least one future is required

### Integration Tests

1. **With DurableContext**: Test macros with actual durable operations
2. **With step operations**: Test combining different step operations
3. **With nested contexts**: Test macros in child contexts

## File Structure

```
aws-durable-execution-sdk/src/
├── lib.rs              # Re-exports macros
├── macros.rs           # NEW: Macro definitions
└── ...
```

## Implementation Notes

### Why Declarative Macros?

Declarative macros (`macro_rules!`) are chosen over procedural macros because:
1. Simpler implementation - no separate crate needed
2. Faster compilation - no proc-macro overhead
3. Sufficient for the use case - just need to box and collect futures

### Fully Qualified Paths

The macros use fully qualified paths (e.g., `::std::vec::Vec`) to avoid conflicts with user-defined types or imports.

### Trailing Comma Support

The `$(,)?` pattern allows optional trailing commas for better ergonomics:
```rust
all!(ctx,
    fut1,
    fut2,
    fut3,  // trailing comma OK
)
```
