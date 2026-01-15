//! Compile-fail tests for promise combinator macros.
//!
//! These tests verify that the macros produce compile-time errors when
//! futures have incompatible output types.
//!
//! Property 4: Compile-time type mismatch detection
//! Validates: Requirements 6.1, 6.2

/// Test that all promise combinator macros reject type mismatches at compile time.
///
/// This test uses trybuild to verify that the following scenarios produce compile errors:
/// - all! macro with mixed i32 and String futures
/// - any! macro with mixed i32 and String futures
/// - race! macro with mixed i32 and String futures
/// - all_settled! macro with mixed i32 and String futures
#[test]
fn test_macro_type_mismatch_compile_errors() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/*.rs");
}
