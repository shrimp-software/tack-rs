//! QuickJS runtime for tool-backed TypeScript snippets.
//!
//! This crate executes TypeScript snippets against the host-independent
//! contracts from `tack-codemode`. It uses Oxc to strip TypeScript syntax,
//! evaluates code in an isolated QuickJS context, exposes host tool calls,
//! `emit`, and `console.log`, and enforces runtime and payload limits.

mod runtime;

use async_trait::async_trait;
use rquickjs::{
    AsyncContext, AsyncRuntime, CatchResultExt, Promise,
    prelude::{Async, Func},
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;
use tack_codemode::{
    BuiltinAwareInvoker, CodeRuntime, FailurePhase, RegistryAwareInvoker, RuntimeFailure,
    RuntimeInput, RuntimeOutput, ToolCall, ToolCallOutput, ToolInvoker, validate_aliases,
};
use tokio::time;

use runtime::{
    CapturedOutput, bootstrap_script, enforce_output_limit, runtime_error, runtime_limits_error,
    transform_typescript, wrap_user_code,
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
        input.limits.validate().map_err(runtime_limits_error)?;
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

fn install_host_api(
    ctx: &rquickjs::Ctx<'_>,
    input: &RuntimeInput,
    invoker: Arc<dyn ToolInvoker>,
    call_count: Arc<Mutex<usize>>,
    captured: CapturedOutput,
) -> Result<(), RuntimeFailure> {
    let global = ctx.globals();
    let captured_for_emit = captured.clone();
    global
        .set(
            "__tackEmitJson",
            Func::from(move |value: String| {
                captured_for_emit.emit_json(&value);
            }),
        )
        .map_err(runtime_error)?;

    let captured_for_log = captured.clone();
    global
        .set(
            "__tackLogLine",
            Func::from(move |value: String| {
                captured_for_log.log_line(value);
            }),
        )
        .map_err(runtime_error)?;

    let handler = ToolCallHandler::new(invoker, call_count, input);
    global
        .set(
            "__tackCallJson",
            Func::from(Async(move |path: String, input_json: String| {
                let handler = handler.clone();
                async move { handler.call(path, input_json).await }
            })),
        )
        .map_err(runtime_error)
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

#[derive(Clone)]
struct ToolCallHandler {
    invoker: Arc<dyn ToolInvoker>,
    call_count: Arc<Mutex<usize>>,
    max_calls: usize,
    tool_call_timeout: std::time::Duration,
    max_request: usize,
    max_response: usize,
}

impl ToolCallHandler {
    fn new(
        invoker: Arc<dyn ToolInvoker>,
        call_count: Arc<Mutex<usize>>,
        input: &RuntimeInput,
    ) -> Self {
        Self {
            invoker,
            call_count,
            max_calls: input.limits.max_tool_calls,
            tool_call_timeout: input.limits.tool_call_timeout(),
            max_request: input.limits.max_tool_request_bytes,
            max_response: input.limits.max_tool_response_bytes,
        }
    }

    async fn call(&self, path: String, input_json: String) -> String {
        match self.build_call(path, input_json) {
            Ok(call) => self.invoke(call).await,
            Err(message) => serialize_tool_output(&ToolCallOutput::failure(message)),
        }
    }

    fn build_call(&self, path: String, input_json: String) -> Result<ToolCall, String> {
        self.reserve_call()?;
        if input_json.len() > self.max_request {
            return Err("tool request too large".to_string());
        }
        let input = serde_json::from_str::<Value>(&input_json)
            .map_err(|error| format!("tool request is not valid JSON: {error}"))?;
        Ok(ToolCall { path, input })
    }

    fn reserve_call(&self) -> Result<(), String> {
        let mut count = self.call_count.lock().expect("call count lock");
        *count += 1;
        if *count > self.max_calls {
            Err("tool call limit exceeded".to_string())
        } else {
            Ok(())
        }
    }

    async fn invoke(&self, call: ToolCall) -> String {
        let output = match time::timeout(self.tool_call_timeout, self.invoker.call(call)).await {
            Ok(output) => output,
            Err(_) => ToolCallOutput::failure("tool call timed out"),
        };
        let output_json = serialize_tool_output(&output);
        if output_json.len() > self.max_response {
            serialize_tool_output(&ToolCallOutput::failure("tool response too large"))
        } else {
            output_json
        }
    }
}

fn serialize_tool_output(output: &ToolCallOutput) -> String {
    serde_json::to_string(output).unwrap_or_else(|error| {
        json!({"ok":false,"error":{"message":error.to_string()}}).to_string()
    })
}

#[cfg(test)]
mod tests;
