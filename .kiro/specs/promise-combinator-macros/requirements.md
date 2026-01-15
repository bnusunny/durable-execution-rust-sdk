# Requirements Document

## Introduction

This feature improves the ergonomics of promise combinators (`all`, `any`, `race`, `all_settled`) in the AWS Durable Execution SDK for Rust.

### Problem Statement

The current method-based API (`ctx.all()`, `ctx.any()`, etc.) has a significant usability limitation: it requires all futures in the vector to have the **exact same concrete type**. In Rust, each closure has a unique anonymous type, which means this common pattern doesn't compile:

```rust
// This doesn't compile - each closure has a unique type!
let futures = vec![
    ctx.step(|_| Ok(1), None),  // Type: impl Future<...> with closure type A
    ctx.step(|_| Ok(2), None),  // Type: impl Future<...> with closure type B (different!)
];
let results = ctx.all(futures).await?;
```

### When the Current API Works

The existing method-based API (`ctx.all()`, etc.) IS useful for:
1. **Homogeneous futures from iterators/loops** - when the same closure is applied multiple times
2. **Pre-boxed futures** - when futures are already `Pin<Box<dyn Future<...>>>`
3. **Programmatically generated futures** - when futures come from a single source

### Proposed Solution

Add declarative macros (`all!`, `any!`, `race!`, `all_settled!`) that automatically box futures to erase their types, enabling the common use case of combining different step operations:

```rust
// This will work with macros!
let results = all!(ctx,
    ctx.step(|_| Ok(1), None),
    ctx.step(|_| Ok(2), None),
    ctx.step(|_| Ok(3), None),
).await?;
```

The macros complement (not replace) the existing method-based API.

## Glossary

- **Promise_Combinator_Macro**: A declarative macro that accepts multiple futures and combines them using the underlying promise combinator methods
- **Type_Erasure**: The process of converting concrete future types into trait objects (`Pin<Box<dyn Future>>`) to allow heterogeneous futures in a collection
- **DurableContext**: The main context object that provides durable execution operations
- **BoxedFuture**: A type alias for `Pin<Box<dyn Future<Output = DurableResult<T>> + Send>>`
- **DurableResult**: The SDK's result type alias for `Result<T, DurableError>`
- **Homogeneous_Futures**: Futures that all have the exact same concrete type (e.g., from a loop with the same closure)
- **Heterogeneous_Futures**: Futures with different concrete types (e.g., different closures or different operations)

## Requirements

### Requirement 1: all! Macro

**User Story:** As a developer, I want to use an `all!` macro to wait for multiple heterogeneous futures to complete, so that I can combine different step operations without manual type erasure.

#### Acceptance Criteria

1. WHEN a developer uses `all!(ctx, fut1, fut2, fut3)`, THE Macro SHALL box each future and call `ctx.all()` with the boxed futures
2. WHEN all futures succeed, THE Macro SHALL return `Ok(Vec<T>)` containing all results in order
3. WHEN any future fails, THE Macro SHALL return `Err` with the first error encountered
4. THE Macro SHALL support any number of futures (1 or more)
5. THE Macro SHALL preserve the order of results matching the order of input futures
6. THE Macro SHALL work with futures that return different closure types but the same output type `T`

### Requirement 2: any! Macro

**User Story:** As a developer, I want to use an `any!` macro to get the first successful result from multiple futures, so that I can implement fallback patterns easily.

#### Acceptance Criteria

1. WHEN a developer uses `any!(ctx, fut1, fut2, fut3)`, THE Macro SHALL box each future and call `ctx.any()` with the boxed futures
2. WHEN any future succeeds, THE Macro SHALL return `Ok(T)` with the first successful result
3. WHEN all futures fail, THE Macro SHALL return `Err` containing information about all failures
4. THE Macro SHALL support any number of futures (1 or more)
5. THE Macro SHALL work with futures that return different closure types but the same output type `T`

### Requirement 3: race! Macro

**User Story:** As a developer, I want to use a `race!` macro to get the result of the first future to settle (success or failure), so that I can implement timeout or racing patterns.

#### Acceptance Criteria

1. WHEN a developer uses `race!(ctx, fut1, fut2, fut3)`, THE Macro SHALL box each future and call `ctx.race()` with the boxed futures
2. WHEN any future settles first, THE Macro SHALL return that future's result (success or failure)
3. THE Macro SHALL support any number of futures (1 or more)
4. THE Macro SHALL work with futures that return different closure types but the same output type `T`

### Requirement 4: all_settled! Macro

**User Story:** As a developer, I want to use an `all_settled!` macro to wait for all futures to complete regardless of success or failure, so that I can collect all outcomes for analysis.

#### Acceptance Criteria

1. WHEN a developer uses `all_settled!(ctx, fut1, fut2, fut3)`, THE Macro SHALL box each future and call `ctx.all_settled()` with the boxed futures
2. THE Macro SHALL return `BatchResult<T>` containing outcomes for all futures
3. THE Macro SHALL support any number of futures (1 or more)
4. THE Macro SHALL preserve the order of results matching the order of input futures
5. THE Macro SHALL work with futures that return different closure types but the same output type `T`

### Requirement 5: Macro Export and Documentation

**User Story:** As a developer, I want the macros to be easily accessible and well-documented, so that I can discover and use them effectively.

#### Acceptance Criteria

1. THE SDK SHALL export all macros from the crate root
2. THE SDK SHALL provide documentation with examples for each macro
3. THE SDK SHALL include the macros in the main `aws-durable-execution-sdk` crate (not a separate macros crate)
4. THE Documentation SHALL explain when to use macros vs the method-based API
5. THE Documentation SHALL show examples of combining different step operations
6. THE Documentation SHALL clarify that the method-based API is still useful for homogeneous futures (from loops/iterators)

### Requirement 6: Type Safety

**User Story:** As a developer, I want the macros to maintain type safety, so that I get compile-time errors for type mismatches.

#### Acceptance Criteria

1. WHEN futures have incompatible output types, THE Macro SHALL produce a compile-time error
2. THE Macro SHALL require all futures to have the same output type `T`
3. THE Macro SHALL work with any type `T` that implements `Serialize + DeserializeOwned + Send + Clone + 'static`
