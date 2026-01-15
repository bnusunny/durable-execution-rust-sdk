//! Compile-fail test: all_settled! macro should reject futures with incompatible output types.
//! 
//! Property 4: Compile-time type mismatch detection
//! Validates: Requirements 6.1, 6.2

use std::future::Future;
use std::pin::Pin;
use aws_durable_execution_sdk::all_settled;
use aws_durable_execution_sdk::error::DurableError;
use aws_durable_execution_sdk::concurrency::BatchResult;

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
    let _ = all_settled!(ctx,
        Box::pin(async { Ok::<_, DurableError>(42i32) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
        Box::pin(async { Ok::<_, DurableError>("hello".to_string()) }) as Pin<Box<dyn Future<Output = Result<String, DurableError>> + Send>>,
    );
}
