//! QuickJS runtime for tool-backed TypeScript snippets.
//!
//! This crate executes TypeScript snippets against the host-independent
//! contracts from `tack-codemode`. It uses Oxc to strip TypeScript syntax,
//! evaluates code in an isolated QuickJS context, exposes host tool calls,
//! `emit`, and `console.log`, and enforces runtime and payload limits.

mod runtime;

use async_trait::async_trait;
use rquickjs::{AsyncContext, AsyncRuntime, CatchResultExt, Promise};
use serde_json::Value;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;
use tack_codemode::{
    BuiltinAwareInvoker, CodeRuntime, FailurePhase, RegistryAwareInvoker, RuntimeFailure,
    RuntimeInput, RuntimeOutput, ToolInvoker, validate_aliases,
};
use tokio::time;

use runtime::{
    CapturedOutput, bootstrap_script, enforce_output_limit, install_host_api, runtime_error,
    runtime_limits_error, transform_typescript, wrap_user_code,
};

#[derive(Clone, Debug, Default)]
pub struct QuickJsRuntime;

#[async_trait(?Send)]
impl CodeRuntime for QuickJsRuntime {
    async fn execute(
        &self,
        input: RuntimeInput,
        invoker: Arc<dyn ToolInvoker>,
    ) -> Result<RuntimeOutput, RuntimeFailure> {
        execute_async(input, invoker).await
    }
}

async fn execute_async(
    input: RuntimeInput,
    invoker: Arc<dyn ToolInvoker>,
) -> Result<RuntimeOutput, RuntimeFailure> {
    validate_input(&input)?;

    let transformed = transform_typescript(&input.source)?;
    let captured = CapturedOutput::new(input.limits.max_output_bytes);
    let call_count = Arc::new(Mutex::new(0usize));
    let timed_out = Arc::new(AtomicBool::new(false));
    let invoker = runtime_invoker(input.registry.clone(), invoker);

    let runtime = AsyncRuntime::new().map_err(runtime_error)?;
    runtime.set_memory_limit(input.limits.memory_bytes).await;
    runtime.set_max_stack_size(input.limits.stack_bytes).await;
    install_interrupt_handler(&runtime, &input, timed_out.clone()).await;
    let context = AsyncContext::full(&runtime).await.map_err(runtime_error)?;

    let output = run_with_timeout(
        &context,
        &input,
        &transformed,
        invoker,
        call_count,
        captured,
        timed_out,
    )
    .await;
    cleanup_runtime(&runtime).await;
    output
}

fn validate_input(input: &RuntimeInput) -> Result<(), RuntimeFailure> {
    validate_aliases(&input.aliases)?;
    input.limits.validate().map_err(runtime_limits_error)?;
    input.registry.validate().map_err(|error| {
        RuntimeFailure::new(
            FailurePhase::Runtime,
            format!("invalid tool registry: {error}"),
        )
    })
}

fn runtime_invoker(
    registry: tack_codemode::ToolRegistry,
    invoker: Arc<dyn ToolInvoker>,
) -> Arc<dyn ToolInvoker> {
    Arc::new(BuiltinAwareInvoker::new(
        registry.clone(),
        Arc::new(RegistryAwareInvoker::new(registry, invoker)),
    ))
}

async fn install_interrupt_handler(
    runtime: &AsyncRuntime,
    input: &RuntimeInput,
    timed_out: Arc<AtomicBool>,
) {
    let timeout = input.limits.timeout();
    let started_at = Instant::now();
    runtime
        .set_interrupt_handler(Some(Box::new(move || {
            if started_at.elapsed() >= timeout {
                timed_out.store(true, Ordering::Relaxed);
                true
            } else {
                false
            }
        })))
        .await;
}

async fn run_with_timeout(
    context: &AsyncContext,
    input: &RuntimeInput,
    transformed: &str,
    invoker: Arc<dyn ToolInvoker>,
    call_count: Arc<Mutex<usize>>,
    captured: CapturedOutput,
    timed_out: Arc<AtomicBool>,
) -> Result<RuntimeOutput, RuntimeFailure> {
    let timeout = input.limits.timeout();
    let captured_for_run = captured.clone();
    let run = context.async_with(async |ctx| {
        install_host_api(&ctx, input, invoker, call_count, captured_for_run.clone())?;
        evaluate_bootstrap(&ctx, input, &captured_for_run, &timed_out)?;
        let output = evaluate_user_code(&ctx, transformed, &captured_for_run, &timed_out).await?;
        enforce_output_limit(&output, input.limits.max_output_bytes)?;
        Ok(output)
    });

    tokio::pin!(run);
    tokio::select! {
        output = &mut run => output,
        _ = time::sleep(timeout) => {
            timed_out.store(true, Ordering::Relaxed);
            Err(captured.failure(FailurePhase::Timeout, "execution timed out"))
        }
    }
}

fn evaluate_bootstrap(
    ctx: &rquickjs::Ctx<'_>,
    input: &RuntimeInput,
    captured: &CapturedOutput,
    timed_out: &AtomicBool,
) -> Result<(), RuntimeFailure> {
    let bootstrap = bootstrap_script(&input.aliases, input.limits.max_tool_request_bytes);
    ctx.eval::<(), _>(bootstrap)
        .catch(ctx)
        .map_err(|error| captured.caught_failure(error, timed_out))
}

async fn evaluate_user_code(
    ctx: &rquickjs::Ctx<'_>,
    transformed: &str,
    captured: &CapturedOutput,
    timed_out: &AtomicBool,
) -> Result<RuntimeOutput, RuntimeFailure> {
    let wrapped = wrap_user_code(transformed);
    let promise = ctx
        .eval::<Promise, _>(wrapped)
        .catch(ctx)
        .map_err(|error| captured.caught_failure(error, timed_out))?;
    let result_json = promise
        .into_future::<String>()
        .await
        .catch(ctx)
        .map_err(|error| captured.caught_failure(error, timed_out))?;
    let result = serde_json::from_str::<Value>(&result_json)
        .ok()
        .and_then(|value| value.get("value").cloned())
        .unwrap_or(Value::Null);
    captured.clone().into_output(result)
}

async fn cleanup_runtime(runtime: &AsyncRuntime) {
    runtime.set_interrupt_handler(None).await;
    runtime.idle().await;
}

#[cfg(test)]
mod tests;
