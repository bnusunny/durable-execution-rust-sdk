//! Replay-Safe UUID Generation Example
//!
//! Generating deterministic UUIDs that are safe for replay.

use aws_durable_execution_sdk::{
    durable_execution, DurableError,
    replay_safe::{uuid_from_operation, uuid_string_from_operation, uuid_to_string},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityWithId {
    pub id: String,
    pub name: String,
    pub created_at_seed: u64,
}

/// Generate replay-safe UUIDs.
///
/// When you need unique identifiers in a durable workflow,
/// use replay-safe UUID generation to ensure the same IDs
/// are generated on replay.
#[durable_execution]
pub async fn handler(_event: serde_json::Value, ctx: DurableContext) -> Result<Vec<EntityWithId>, DurableError> {
    let mut entities = Vec::new();

    // Generate deterministic UUIDs based on operation ID
    for i in 0..3 {
        let op_id = ctx.next_operation_id();
        
        // Generate UUID from operation ID and seed
        let uuid = uuid_string_from_operation(&op_id, i);
        
        // Create entity with deterministic ID
        let entity: EntityWithId = ctx
            .step_named(&format!("create_entity_{}", i), |_| {
                Ok(EntityWithId {
                    id: uuid.clone(),
                    name: format!("Entity {}", i),
                    created_at_seed: i,
                })
            }, None)
            .await?;
        
        entities.push(entity);
    }

    Ok(entities)
}

/// Demonstrate UUID generation functions.
pub fn uuid_examples() {
    let operation_id = "my-operation-123";
    
    // Generate UUID bytes
    let uuid_bytes = uuid_from_operation(operation_id, 0);
    
    // Convert to string
    let uuid_string = uuid_to_string(&uuid_bytes);
    println!("UUID: {}", uuid_string);
    
    // Or use convenience function
    let uuid = uuid_string_from_operation(operation_id, 0);
    println!("UUID (convenience): {}", uuid);
    
    // Same inputs always produce the same UUID
    let uuid2 = uuid_string_from_operation(operation_id, 0);
    assert_eq!(uuid, uuid2);
    
    // Different seeds produce different UUIDs
    let uuid3 = uuid_string_from_operation(operation_id, 1);
    assert_ne!(uuid, uuid3);
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
