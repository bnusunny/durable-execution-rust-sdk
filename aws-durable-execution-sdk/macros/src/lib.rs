//! Procedural macros for AWS Durable Execution SDK
//!
//! This crate provides the `#[durable_execution]` attribute macro
//! for creating durable Lambda handler functions.

use proc_macro::TokenStream;
use quote::{quote, format_ident};
use syn::{parse_macro_input, ItemFn, FnArg, Pat, ReturnType, spanned::Spanned};

/// Attribute macro that transforms an async function into a durable Lambda handler.
///
/// This macro wraps your async function to integrate with AWS Lambda's durable execution
/// service. It handles:
/// - Parsing `DurableExecutionInvocationInput` from the Lambda event
/// - Creating `ExecutionState` and `DurableContext` for the handler
/// - Processing results, errors, and suspend signals
/// - Returning `DurableExecutionInvocationOutput` with appropriate status
///
/// # Function Signature
///
/// The decorated function must have the following signature:
/// ```rust,ignore
/// async fn handler_name(event: EventType, ctx: DurableContext) -> Result<ResultType, DurableError>
/// ```
///
/// Where:
/// - `EventType` must implement `serde::Deserialize`
/// - `ResultType` must implement `serde::Serialize`
///
/// # Example
///
/// ```rust,ignore
/// use aws_durable_execution_sdk::{durable_execution, DurableContext, DurableError};
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Deserialize)]
/// struct MyEvent {
///     order_id: String,
/// }
///
/// #[derive(Serialize)]
/// struct MyResult {
///     status: String,
/// }
///
/// #[durable_execution]
/// async fn my_handler(event: MyEvent, ctx: DurableContext) -> Result<MyResult, DurableError> {
///     // Your workflow logic here
///     let result = ctx.step(|_| Ok("processed".to_string()), None).await?;
///     Ok(MyResult { status: result })
/// }
/// ```
///
/// # Generated Code
///
/// The macro generates a Lambda handler function that:
/// 1. Receives `DurableExecutionInvocationInput` from Lambda
/// 2. Extracts the user's event payload from the input
/// 3. Creates an `ExecutionState` with the checkpoint token and initial operations
/// 4. Creates a `DurableContext` for the user's handler
/// 5. Calls the user's handler function
/// 6. Converts the result to `DurableExecutionInvocationOutput`:
///    - `Ok(result)` → `SUCCEEDED` status with serialized result
///    - `Err(DurableError::Suspend { .. })` → `PENDING` status
///    - `Err(other)` → `FAILED` status with error details
///
/// # Requirements
///
/// - 15.1: THE Lambda_Integration SHALL provide a `#[durable_execution]` attribute macro for handler functions
/// - 15.3: THE Lambda_Integration SHALL create ExecutionState and DurableContext for the handler
#[proc_macro_attribute]
pub fn durable_execution(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    
    // Validate the function is async
    if input_fn.sig.asyncness.is_none() {
        return syn::Error::new(
            input_fn.sig.fn_token.span(),
            "durable_execution handler must be an async function"
        ).to_compile_error().into();
    }
    
    // Extract function components
    let fn_name = &input_fn.sig.ident;
    let fn_vis = &input_fn.vis;
    let fn_block = &input_fn.block;
    let fn_attrs = &input_fn.attrs;
    
    // Parse function arguments - expect (event: EventType, ctx: DurableContext)
    let args: Vec<_> = input_fn.sig.inputs.iter().collect();
    
    if args.len() != 2 {
        return syn::Error::new(
            input_fn.sig.inputs.span(),
            "durable_execution handler must have exactly 2 arguments: (event: EventType, ctx: DurableContext)"
        ).to_compile_error().into();
    }
    
    // Extract event type from first argument
    let event_type = match &args[0] {
        FnArg::Typed(pat_type) => &pat_type.ty,
        FnArg::Receiver(_) => {
            return syn::Error::new(
                args[0].span(),
                "durable_execution handler cannot have self parameter"
            ).to_compile_error().into();
        }
    };
    
    // Extract event parameter name
    let event_param_name = match &args[0] {
        FnArg::Typed(pat_type) => {
            match pat_type.pat.as_ref() {
                Pat::Ident(ident) => &ident.ident,
                _ => {
                    return syn::Error::new(
                        pat_type.pat.span(),
                        "event parameter must be a simple identifier"
                    ).to_compile_error().into();
                }
            }
        }
        _ => unreachable!(),
    };
    
    // Extract context parameter name
    let ctx_param_name = match &args[1] {
        FnArg::Typed(pat_type) => {
            match pat_type.pat.as_ref() {
                Pat::Ident(ident) => &ident.ident,
                _ => {
                    return syn::Error::new(
                        pat_type.pat.span(),
                        "context parameter must be a simple identifier"
                    ).to_compile_error().into();
                }
            }
        }
        FnArg::Receiver(_) => {
            return syn::Error::new(
                args[1].span(),
                "durable_execution handler cannot have self parameter"
            ).to_compile_error().into();
        }
    };
    
    // Extract return type
    let return_type = match &input_fn.sig.output {
        ReturnType::Type(_, ty) => ty.as_ref(),
        ReturnType::Default => {
            return syn::Error::new(
                input_fn.sig.output.span(),
                "durable_execution handler must return Result<T, DurableError>"
            ).to_compile_error().into();
        }
    };
    
    // Generate the inner function name
    let inner_fn_name = format_ident!("__{}_inner", fn_name);
    
    // Generate the wrapper function
    let output = quote! {
        // The inner function containing the user's logic
        #(#fn_attrs)*
        async fn #inner_fn_name(
            #event_param_name: #event_type,
            #ctx_param_name: ::aws_durable_execution_sdk::DurableContext,
        ) -> #return_type
        #fn_block
        
        // The Lambda handler wrapper
        #fn_vis async fn #fn_name(
            lambda_event: ::lambda_runtime::LambdaEvent<::aws_durable_execution_sdk::DurableExecutionInvocationInput>,
        ) -> ::std::result::Result<::aws_durable_execution_sdk::DurableExecutionInvocationOutput, ::lambda_runtime::Error> {
            use ::aws_durable_execution_sdk::{
                DurableContext, DurableError, DurableExecutionInvocationOutput,
                ExecutionState, ErrorObject, CheckpointBatcherConfig,
                SharedDurableServiceClient, LambdaDurableServiceClient,
                OperationType,
            };
            
            let (durable_input, lambda_context) = lambda_event.into_parts();
            
            // Extract the user's event from the input
            // First try the top-level Input field, then fall back to ExecutionDetails.InputPayload
            let user_event: #event_type = {
                // Try top-level Input first
                if let Some(value) = &durable_input.input {
                    match ::serde_json::from_value(value.clone()) {
                        Ok(event) => event,
                        Err(e) => {
                            return Ok(DurableExecutionInvocationOutput::failed(
                                ErrorObject::new("DeserializationError", format!("Failed to deserialize event from Input: {}", e))
                            ));
                        }
                    }
                } else {
                    // Try to extract from ExecutionDetails.InputPayload of the EXECUTION operation
                    let execution_op = durable_input.initial_execution_state.operations.iter()
                        .find(|op| op.operation_type == OperationType::Execution);
                    
                    if let Some(op) = execution_op {
                        if let Some(details) = &op.execution_details {
                            if let Some(payload) = &details.input_payload {
                                // Parse the JSON string payload
                                match ::serde_json::from_str::<#event_type>(payload) {
                                    Ok(event) => event,
                                    Err(e) => {
                                        return Ok(DurableExecutionInvocationOutput::failed(
                                            ErrorObject::new("DeserializationError", format!("Failed to deserialize event from ExecutionDetails.InputPayload: {}", e))
                                        ));
                                    }
                                }
                            } else {
                                // No InputPayload, try default
                                match ::serde_json::from_value(::serde_json::Value::Null) {
                                    Ok(event) => event,
                                    Err(_) => {
                                        return Ok(DurableExecutionInvocationOutput::failed(
                                            ErrorObject::new("DeserializationError", "No InputPayload in ExecutionDetails and event type does not support default")
                                        ));
                                    }
                                }
                            }
                        } else {
                            // No ExecutionDetails, try default
                            match ::serde_json::from_value(::serde_json::Value::Null) {
                                Ok(event) => event,
                                Err(_) => {
                                    return Ok(DurableExecutionInvocationOutput::failed(
                                        ErrorObject::new("DeserializationError", "No ExecutionDetails in EXECUTION operation and event type does not support default")
                                    ));
                                }
                            }
                        }
                    } else {
                        // No EXECUTION operation found, try default
                        match ::serde_json::from_value(::serde_json::Value::Null) {
                            Ok(event) => event,
                            Err(_) => {
                                return Ok(DurableExecutionInvocationOutput::failed(
                                    ErrorObject::new("DeserializationError", "No input provided and event type does not support default")
                                ));
                            }
                        }
                    }
                }
            };
            
            // Create the service client
            let aws_config = ::aws_config::load_defaults(::aws_config::BehaviorVersion::latest()).await;
            let service_client: SharedDurableServiceClient = ::std::sync::Arc::new(
                LambdaDurableServiceClient::from_aws_config(&aws_config)
            );
            
            // Create ExecutionState with batcher
            let batcher_config = CheckpointBatcherConfig::default();
            let (state, mut batcher) = ExecutionState::with_batcher(
                &durable_input.durable_execution_arn,
                &durable_input.checkpoint_token,
                durable_input.initial_execution_state,
                service_client,
                batcher_config,
                100, // queue buffer size
            );
            let state = ::std::sync::Arc::new(state);
            
            // Spawn the checkpoint batcher task
            let batcher_handle = ::tokio::spawn(async move {
                batcher.run().await;
            });
            
            // Create DurableContext
            let durable_ctx = DurableContext::from_lambda_context(state.clone(), lambda_context);
            
            // Call the user's handler
            let result = #inner_fn_name(user_event, durable_ctx).await;
            
            // Process the result while state is still alive (for large response checkpointing)
            const MAX_RESPONSE_SIZE: usize = 6 * 1024 * 1024;
            
            let output = match result {
                Ok(value) => {
                    // Serialize the result
                    match ::serde_json::to_string(&value) {
                        Ok(json) => {
                            // Check if response is too large (>6MB)
                            if json.len() > MAX_RESPONSE_SIZE {
                                // For large responses, checkpoint the result before returning
                                // Generate a unique operation ID for the result checkpoint
                                let result_op_id = format!("__result__{}", ::std::time::SystemTime::now()
                                    .duration_since(::std::time::UNIX_EPOCH)
                                    .map(|d| d.as_nanos())
                                    .unwrap_or(0));
                                
                                // Create an operation update to checkpoint the large result
                                let update = ::aws_durable_execution_sdk::OperationUpdate::succeed(
                                    &result_op_id,
                                    ::aws_durable_execution_sdk::OperationType::Execution,
                                    Some(json.clone()),
                                );
                                
                                // Checkpoint the result synchronously
                                match state.create_checkpoint(update, true).await {
                                    Ok(()) => {
                                        // Return a reference to the checkpointed result
                                        DurableExecutionInvocationOutput::succeeded(Some(
                                            format!("{{\"__checkpointed_result__\":\"{}\",\"size\":{}}}", result_op_id, json.len())
                                        ))
                                    }
                                    Err(e) => {
                                        // If checkpointing fails, return an error
                                        DurableExecutionInvocationOutput::failed(
                                            ErrorObject::new(
                                                "CheckpointError",
                                                format!("Failed to checkpoint large result: {}", e)
                                            )
                                        )
                                    }
                                }
                            } else {
                                DurableExecutionInvocationOutput::succeeded(Some(json))
                            }
                        }
                        Err(e) => {
                            DurableExecutionInvocationOutput::failed(
                                ErrorObject::new("SerializationError", format!("Failed to serialize result: {}", e))
                            )
                        }
                    }
                }
                Err(DurableError::Suspend { .. }) => {
                    // Execution suspended - return PENDING status
                    DurableExecutionInvocationOutput::pending()
                }
                Err(error) => {
                    // Execution failed - return FAILED status with error details
                    DurableExecutionInvocationOutput::failed(ErrorObject::from(&error))
                }
            };
            
            // Drop the state to close the checkpoint queue and stop the batcher
            drop(state);
            
            // Wait for batcher to finish (with timeout)
            let _ = ::tokio::time::timeout(
                ::std::time::Duration::from_secs(5),
                batcher_handle
            ).await;
            
            Ok(output)
        }
    };
    
    output.into()
}
