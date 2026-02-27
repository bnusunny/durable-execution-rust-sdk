//! Procedural macros for AWS Durable Execution SDK
//!
//! This crate provides the `#[durable_execution]` attribute macro
//! for creating durable Lambda handler functions.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, spanned::Spanned, FnArg, ItemFn, Pat, ReturnType};

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
            "durable_execution handler must be an async function",
        )
        .to_compile_error()
        .into();
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
                "durable_execution handler cannot have self parameter",
            )
            .to_compile_error()
            .into();
        }
    };

    // Extract event parameter name
    let event_param_name = match &args[0] {
        FnArg::Typed(pat_type) => match pat_type.pat.as_ref() {
            Pat::Ident(ident) => &ident.ident,
            _ => {
                return syn::Error::new(
                    pat_type.pat.span(),
                    "event parameter must be a simple identifier",
                )
                .to_compile_error()
                .into();
            }
        },
        _ => unreachable!(),
    };

    // Extract context parameter name
    let ctx_param_name = match &args[1] {
        FnArg::Typed(pat_type) => match pat_type.pat.as_ref() {
            Pat::Ident(ident) => &ident.ident,
            _ => {
                return syn::Error::new(
                    pat_type.pat.span(),
                    "context parameter must be a simple identifier",
                )
                .to_compile_error()
                .into();
            }
        },
        FnArg::Receiver(_) => {
            return syn::Error::new(
                args[1].span(),
                "durable_execution handler cannot have self parameter",
            )
            .to_compile_error()
            .into();
        }
    };

    // Extract return type
    let return_type = match &input_fn.sig.output {
        ReturnType::Type(_, ty) => ty.as_ref(),
        ReturnType::Default => {
            return syn::Error::new(
                input_fn.sig.output.span(),
                "durable_execution handler must return Result<T, DurableError>",
            )
            .to_compile_error()
            .into();
        }
    };

    // Generate the inner function name
    let inner_fn_name = format_ident!("__{}_inner", fn_name);

    // Generate the wrapper function — delegates to run_durable_handler
    let output = quote! {
        // The inner function containing the user's logic
        #(#fn_attrs)*
        async fn #inner_fn_name(
            #event_param_name: #event_type,
            #ctx_param_name: ::aws_durable_execution_sdk::DurableContext,
        ) -> #return_type
        #fn_block

        // The Lambda handler wrapper — thin delegation to the runtime
        #fn_vis async fn #fn_name(
            lambda_event: ::lambda_runtime::LambdaEvent<::aws_durable_execution_sdk::DurableExecutionInvocationInput>,
        ) -> ::std::result::Result<::aws_durable_execution_sdk::DurableExecutionInvocationOutput, ::lambda_runtime::Error> {
            ::aws_durable_execution_sdk::run_durable_handler(lambda_event, #inner_fn_name).await
        }
    };

    output.into()
}
