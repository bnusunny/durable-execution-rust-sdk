//! Compile-fail test: all_settled! macro should reject futures with incompatible output types.
//!
//! Property 4: Compile-time type mismatch detection
//! Validates: Requirements 6.1, 6.2

use std::future::Future;
use std::pin::Pin;
use aws_durable_execution_sdk::all_settled;
use aws_durable_execution_sdk::error::DurableError;
use aws_durable_execution_sdk::concurrency::BatchResult;

type DynFut<T> = Pin<Box<dyn Future<Output = Result<T, DurableError>> + Send>>;

// Mock context for testing
struct MockContext;

impl MockContext {
    async fn all_settled<T, Fut>(&self, _futures: Vec<Fut>) -> Result<BatchResult<T>, DurableError>
    where
        T: serde::Serialize + serde::de::DeserializeOwned + Send + Clone + 'static,
        Fut: Future<Output = Result<T, DurableError>> + Send + 'static,
    {
        unimplemented!()
    }
}

fn main() {
    let ctx = MockContext;

    // This should fail to compile: mixing i32 and String output types
    let f1: DynFut<i32> = Box::pin(async { Ok(42i32) });
    let f2: DynFut<String> = Box::pin(async { Ok("hello".into()) });
    let _ = all_settled!(ctx, f1, f2);
}
