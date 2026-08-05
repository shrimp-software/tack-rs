//! QuickJS runtime for tool-backed TypeScript snippets.
//!
//! This crate executes TypeScript snippets against the host-independent
//! contracts from `tack-codemode`. It uses Oxc to strip TypeScript syntax,
//! evaluates code in an isolated QuickJS context, exposes host tool calls,
//! `emit`, and `console.log`, and enforces runtime and payload limits.

use async_trait::async_trait;
use oxc_allocator::Allocator;
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_data_structures::code_buffer::IndentChar;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{HelperLoaderMode, TransformOptions, Transformer};
use rquickjs::{
    AsyncContext, AsyncRuntime, CatchResultExt, Promise,
    prelude::{Async, Func},
};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;
use tack_codemode::{
    BuiltinAwareInvoker, CodeRuntime, FailurePhase, RegistryAwareInvoker, RuntimeFailure,
    RuntimeInput, RuntimeLimitsError, RuntimeOutput, ToolCall, ToolCallOutput, ToolInvoker,
    validate_aliases,
};
use tokio::time;

const BOOTSTRAP_JS: &str = include_str!("bootstrap.js");

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
    validate_aliases(&input.aliases)?;
    input.limits.validate().map_err(runtime_limits_error)?;
    input.registry.validate().map_err(|error| {
        RuntimeFailure::new(
            FailurePhase::Runtime,
            format!("invalid tool registry: {error}"),
        )
    })?;
    let transformed = transform_typescript(&input.source)?;
    let captured = CapturedOutput::new(input.limits.max_output_bytes);
    let call_count = Arc::new(Mutex::new(0usize));
    let timed_out = Arc::new(AtomicBool::new(false));
    let registry = input.registry.clone();
    let invoker: Arc<dyn ToolInvoker> = Arc::new(BuiltinAwareInvoker::new(
        registry.clone(),
        Arc::new(RegistryAwareInvoker::new(registry, invoker)),
    ));

    let runtime = AsyncRuntime::new().map_err(runtime_error)?;
    runtime.set_memory_limit(input.limits.memory_bytes).await;
    runtime.set_max_stack_size(input.limits.stack_bytes).await;
    let timeout = input.limits.timeout();
    let started_at = Instant::now();
    let timed_out_for_interrupt = timed_out.clone();
    runtime
        .set_interrupt_handler(Some(Box::new(move || {
            if started_at.elapsed() >= timeout {
                timed_out_for_interrupt.store(true, Ordering::Relaxed);
                true
            } else {
                false
            }
        })))
        .await;
    let context = AsyncContext::full(&runtime).await.map_err(runtime_error)?;

    let output = {
        let captured_for_run = captured.clone();
        let run = context.async_with(async |ctx| {
            let global = ctx.globals();
            let captured_for_emit = captured_for_run.clone();
            global
                .set(
                    "__tackEmitJson",
                    Func::from(move |value: String| {
                        captured_for_emit.emit_json(&value);
                    }),
                )
                .map_err(runtime_error)?;

            let captured_for_log = captured_for_run.clone();
            global
                .set(
                    "__tackLogLine",
                    Func::from(move |value: String| {
                        captured_for_log.log_line(value);
                    }),
                )
                .map_err(runtime_error)?;

            let invoker_for_call = invoker.clone();
            let call_count_for_call = call_count.clone();
            let max_calls = input.limits.max_tool_calls;
            let tool_call_timeout = input.limits.tool_call_timeout();
            let max_request = input.limits.max_tool_request_bytes;
            let max_response = input.limits.max_tool_response_bytes;
            global
                .set(
                    "__tackCallJson",
                    Func::from(Async(move |path: String, input_json: String| {
                        let invoker_for_call = invoker_for_call.clone();
                        let call_count_for_call = call_count_for_call.clone();
                        async move {
                            let limit_failure = |message: &str| {
                                json!({"ok":false,"error":{"message":message}}).to_string()
                            };
                            {
                                let mut count =
                                    call_count_for_call.lock().expect("call count lock");
                                *count += 1;
                                if *count > max_calls {
                                    return limit_failure("tool call limit exceeded");
                                }
                            }
                            if input_json.len() > max_request {
                                return limit_failure("tool request too large");
                            }
                            let parsed = match serde_json::from_str::<Value>(&input_json) {
                                Ok(value) => value,
                                Err(error) => {
                                    return limit_failure(&format!(
                                        "tool request is not valid JSON: {error}"
                                    ));
                                }
                            };
                            let call = ToolCall {
                                path,
                                input: parsed,
                            };
                            let output = match time::timeout(
                                tool_call_timeout,
                                invoker_for_call.call(call),
                            )
                            .await
                            {
                                Ok(output) => output,
                                Err(_) => ToolCallOutput::failure("tool call timed out"),
                            };
                            let output_json =
                                serde_json::to_string(&output).unwrap_or_else(|error| {
                                    json!({"ok":false,"error":{"message":error.to_string()}})
                                        .to_string()
                                });
                            if output_json.len() > max_response {
                                limit_failure("tool response too large")
                            } else {
                                output_json
                            }
                        }
                    })),
                )
                .map_err(runtime_error)?;

            let bootstrap = bootstrap_script(&input.aliases, input.limits.max_tool_request_bytes);
            ctx.eval::<(), _>(bootstrap)
                .catch(&ctx)
                .map_err(|error| captured_for_run.caught_failure(error, &timed_out))?;
            let wrapped = format!(
                "(async function(){{const __tackResultStringify = JSON.stringify; const __tackValue = await {transformed}\n; return __tackResultStringify({{value:__tackValue}});}})()"
            );
            let promise = ctx
                .eval::<Promise, _>(wrapped)
                .catch(&ctx)
                .map_err(|error| captured_for_run.caught_failure(error, &timed_out))?;
            let result_json = promise
                .into_future::<String>()
                .await
                .catch(&ctx)
                .map_err(|error| captured_for_run.caught_failure(error, &timed_out))?;
            let result = serde_json::from_str::<Value>(&result_json)
                .ok()
                .and_then(|value| value.get("value").cloned())
                .unwrap_or(Value::Null);
            let output = captured_for_run.into_output(result)?;
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
    };
    cleanup_runtime(&runtime).await;
    output
}

async fn cleanup_runtime(runtime: &AsyncRuntime) {
    runtime.set_interrupt_handler(None).await;
    runtime.idle().await;
}

fn runtime_limits_error(error: RuntimeLimitsError) -> RuntimeFailure {
    RuntimeFailure::new(FailurePhase::Runtime, error.to_string())
}

/// Tracks captured logs and emitted values as an early output guard.
///
/// The exact serialized `RuntimeOutput` is checked again before returning.
#[derive(Clone)]
struct CapturedOutput {
    emitted: Arc<Mutex<Vec<Value>>>,
    logs: Arc<Mutex<Vec<String>>>,
    bytes: Arc<Mutex<usize>>,
    exceeded: Arc<AtomicBool>,
    max_bytes: usize,
}

impl CapturedOutput {
    fn new(max_bytes: usize) -> Self {
        Self {
            emitted: Arc::new(Mutex::new(Vec::new())),
            logs: Arc::new(Mutex::new(Vec::new())),
            bytes: Arc::new(Mutex::new(0)),
            exceeded: Arc::new(AtomicBool::new(false)),
            max_bytes,
        }
    }

    fn emit_json(&self, json: &str) {
        if !self.reserve(json.len()) {
            return;
        }
        if let Ok(value) = serde_json::from_str::<Value>(json) {
            self.emitted.lock().expect("emitted lock").push(value);
        }
    }

    fn log_line(&self, line: String) {
        if self.reserve(line.len()) {
            self.logs.lock().expect("logs lock").push(line);
        }
    }

    fn into_output(self, result: Value) -> Result<RuntimeOutput, RuntimeFailure> {
        if self.exceeded.load(Ordering::Relaxed) {
            return Err(self.failure(
                FailurePhase::Runtime,
                format!("execution output exceeded {} bytes", self.max_bytes),
            ));
        }
        Ok(RuntimeOutput {
            result,
            emitted: self.emitted.lock().expect("emitted lock").clone(),
            logs: self.logs.lock().expect("logs lock").clone(),
        })
    }

    fn snapshot(&self) -> (Vec<Value>, Vec<String>) {
        (
            self.emitted.lock().expect("emitted lock").clone(),
            self.logs.lock().expect("logs lock").clone(),
        )
    }

    fn failure(&self, phase: FailurePhase, message: impl Into<String>) -> RuntimeFailure {
        let (emitted, logs) = self.snapshot();
        RuntimeFailure::new(phase, message).with_output(emitted, logs)
    }

    fn caught_failure(
        &self,
        error: rquickjs::CaughtError<'_>,
        timed_out: &AtomicBool,
    ) -> RuntimeFailure {
        if timed_out.load(Ordering::Relaxed) {
            self.failure(FailurePhase::Timeout, "execution timed out")
        } else {
            self.failure(FailurePhase::Runtime, error.to_string())
        }
    }

    fn reserve(&self, next_bytes: usize) -> bool {
        let mut used = self.bytes.lock().expect("capture bytes lock");
        let Some(next_total) = used.checked_add(next_bytes) else {
            self.exceeded.store(true, Ordering::Relaxed);
            return false;
        };
        if next_total > self.max_bytes {
            self.exceeded.store(true, Ordering::Relaxed);
            return false;
        }
        *used = next_total;
        true
    }
}

fn bootstrap_script(
    aliases: &std::collections::HashMap<String, String>,
    max_tool_request_bytes: usize,
) -> String {
    let mut script =
        format!("(() => {{\nconst __tackMaxToolRequestBytes = {max_tool_request_bytes};\n");
    script.push_str(BOOTSTRAP_JS);
    for (namespace, prefix) in aliases {
        script.push_str(&format!(
            "__tackDefine(globalThis, {}, {{ value: __tackNamespace({}, true), writable: false, configurable: false }});\n",
            serde_json::to_string(namespace).expect("namespace json"),
            serde_json::to_string(prefix).expect("prefix json")
        ));
    }
    script.push_str(
        r#"
for (const name of ['__tackCallJson', '__tackEmitJson', '__tackLogLine']) {
  __tackDefine(globalThis, name, { value: undefined, writable: false, configurable: false });
}
})();
"#,
    );
    script
}

fn transform_typescript(source: &str) -> Result<String, RuntimeFailure> {
    let wrapped_source = format!("(async function(){{\n{source}\n}})()");
    let allocator = Allocator::default();
    let path = Path::new("codemode.ts");
    let source_type = SourceType::ts();
    let ret = Parser::new(&allocator, &wrapped_source, source_type).parse();
    if !ret.errors.is_empty() {
        return Err(RuntimeFailure::new(
            FailurePhase::Parse,
            format!(
                "{:?}",
                ret.errors[0]
                    .clone()
                    .with_source_code(wrapped_source.clone())
            ),
        ));
    }
    let mut program = ret.program;
    let semantic = SemanticBuilder::new()
        .with_excess_capacity(2.0)
        .build(&program);
    if !semantic.errors.is_empty() {
        return Err(RuntimeFailure::new(
            FailurePhase::Parse,
            format!(
                "{:?}",
                semantic.errors[0]
                    .clone()
                    .with_source_code(wrapped_source.clone())
            ),
        ));
    }
    let mut options = TransformOptions::default();
    options.helper_loader.mode = HelperLoaderMode::External;
    let transform = Transformer::new(&allocator, path, &options)
        .build_with_scoping(semantic.semantic.into_scoping(), &mut program);
    if !transform.errors.is_empty() {
        return Err(RuntimeFailure::new(
            FailurePhase::Parse,
            format!(
                "{:?}",
                transform.errors[0]
                    .clone()
                    .with_source_code(wrapped_source.clone())
            ),
        ));
    }
    let code = Codegen::new()
        .with_options(CodegenOptions {
            indent_char: IndentChar::Space,
            indent_width: 2,
            ..CodegenOptions::default()
        })
        .build(&program)
        .code;
    Ok(code)
}

fn runtime_error(error: rquickjs::Error) -> RuntimeFailure {
    RuntimeFailure::new(FailurePhase::Runtime, error.to_string())
}

fn enforce_output_limit(
    output: &RuntimeOutput,
    max_output_bytes: usize,
) -> Result<(), RuntimeFailure> {
    let bytes = serde_json::to_vec(output).map_err(|error| {
        RuntimeFailure::new(
            FailurePhase::Runtime,
            format!("execution output is not JSON serializable: {error}"),
        )
        .with_output(output.emitted.clone(), output.logs.clone())
    })?;
    if bytes.len() > max_output_bytes {
        return Err(RuntimeFailure::new(
            FailurePhase::Runtime,
            format!("execution output exceeded {max_output_bytes} bytes"),
        )
        .with_output(output.emitted.clone(), output.logs.clone()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::time::Duration;
    use tack_codemode::{
        CodeRuntime, DenyListPolicy, ExecutionEngine, MemoryInvoker, RuntimeInput, RuntimeLimits,
        ToolCallOutput, ToolDescriptor, ToolInvoker, ToolRegistry,
    };

    #[tokio::test(flavor = "multi_thread")]
    async fn executes_typescript_and_tools() {
        let registry = ToolRegistry {
            tools: vec![ToolDescriptor::new("example.echo", "echo")],
        };
        let mut outputs = HashMap::new();
        outputs.insert("example.echo".to_string(), json!({"message":"ok"}));
        let engine = ExecutionEngine::new(
            registry,
            QuickJsRuntime,
            Arc::new(MemoryInvoker::new(outputs)),
        );
        let result = engine
            .execute("const value: number = 2; return (await tools.example.echo({message: value})).data.message;")
            .await;
        assert!(result.ok, "{:?}", result.error);
        assert_eq!(result.result, Some(json!("ok")));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn aliases_return_data_directly() {
        let registry = ToolRegistry {
            tools: vec![ToolDescriptor::new("eoka.info", "info")],
        };
        let mut outputs = HashMap::new();
        outputs.insert("eoka.info".to_string(), json!({"url":"about:blank"}));
        let engine = ExecutionEngine::new(
            registry,
            QuickJsRuntime,
            Arc::new(MemoryInvoker::new(outputs)),
        )
        .with_alias("eoka", "eoka");
        let result = engine.execute("return (await eoka.info()).url;").await;
        assert!(result.ok, "{:?}", result.error);
        assert_eq!(result.result, Some(json!("about:blank")));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn tools_search_uses_builtin_path() {
        let registry = ToolRegistry {
            tools: vec![ToolDescriptor::new("example.fetch", "fetch")],
        };
        let engine =
            ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(MemoryInvoker::default()));
        let result = engine
            .execute("return (await tools.search({query:'fetch',limit:1})).data[0].path;")
            .await;
        assert!(result.ok, "{:?}", result.error);
        assert_eq!(result.result, Some(json!("example.fetch")));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn policy_filters_builtin_discovery_in_runtime() {
        let registry = ToolRegistry {
            tools: vec![
                ToolDescriptor::new("example.visible", "visible"),
                ToolDescriptor::new("example.secret", "secret"),
            ],
        };
        let engine =
            ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(MemoryInvoker::default()))
                .with_policy(Arc::new(DenyListPolicy::new(vec![
                    "example.secret".to_string(),
                ])));
        let result = engine
            .execute(
                "const search = await tools.search({query:'example',limit:10});
                 const desc = await tools.describe.tool({path:'example.secret'});
                 return {paths: search.data.map(x => x.path), deniedDescribe: desc.ok === false};",
            )
            .await;
        assert!(result.ok, "{:?}", result.error);
        assert_eq!(
            result.result,
            Some(json!({"paths":["example.visible"],"deniedDescribe":true}))
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn awaits_real_async_tool_promises() {
        let registry = ToolRegistry {
            tools: vec![ToolDescriptor::new("example.slow", "slow")],
        };
        let engine = ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(DelayedInvoker));
        let result = engine
            .execute("const a = await tools.example.slow({value: 2}); const b = await tools.example.slow({value: 3}); return a.data.value + b.data.value;")
            .await;
        assert!(result.ok, "{:?}", result.error);
        assert_eq!(result.result, Some(json!(5)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn captures_emit_and_console_log() {
        let engine = ExecutionEngine::new(
            ToolRegistry::default(),
            QuickJsRuntime,
            Arc::new(MemoryInvoker::default()),
        );
        let result = engine
            .execute("emit({step: 1}); console.log('hello'); return 7;")
            .await;
        assert!(result.ok, "{:?}", result.error);
        assert_eq!(result.result, Some(json!(7)));
        assert_eq!(result.emitted, vec![json!({"step": 1})]);
        assert_eq!(result.logs, vec!["hello".to_string()]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn preserves_captured_output_on_runtime_failure() {
        let engine = ExecutionEngine::new(
            ToolRegistry::default(),
            QuickJsRuntime,
            Arc::new(MemoryInvoker::default()),
        );
        let result = engine
            .execute("emit({step: 1}); console.log('before'); throw new Error('boom');")
            .await;
        assert!(!result.ok);
        assert_eq!(result.emitted, vec![json!({"step": 1})]);
        assert_eq!(result.logs, vec!["before".to_string()]);
        let error = result.error.unwrap();
        assert_eq!(error.emitted, vec![json!({"step": 1})]);
        assert_eq!(error.logs, vec!["before".to_string()]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn preserves_captured_output_on_timeout() {
        let limits = RuntimeLimits {
            timeout_ms: 50,
            ..RuntimeLimits::default()
        };
        let engine = ExecutionEngine::new(
            ToolRegistry::default(),
            QuickJsRuntime,
            Arc::new(MemoryInvoker::default()),
        )
        .with_limits(limits);
        let result = engine
            .execute("console.log('before'); while (true) {}")
            .await;
        assert!(!result.ok);
        assert_eq!(result.logs, vec!["before".to_string()]);
        assert_eq!(result.error.unwrap().logs, vec!["before".to_string()]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn console_log_formats_multiple_arguments() {
        let engine = ExecutionEngine::new(
            ToolRegistry::default(),
            QuickJsRuntime,
            Arc::new(MemoryInvoker::default()),
        );
        let result = engine
            .execute("console.log('value', {x: 1}, undefined); return 1;")
            .await;
        assert!(result.ok, "{:?}", result.error);
        assert_eq!(result.logs, vec!["value {\"x\":1} undefined".to_string()]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn enforces_capture_limit_during_logs() {
        let limits = RuntimeLimits {
            max_output_bytes: 32,
            ..RuntimeLimits::default()
        };
        let engine = ExecutionEngine::new(
            ToolRegistry::default(),
            QuickJsRuntime,
            Arc::new(MemoryInvoker::default()),
        )
        .with_limits(limits);
        let result = engine
            .execute("for (let i = 0; i < 10; i++) console.log('xxxxxxxxxx'); return 1;")
            .await;
        assert!(!result.ok);
        assert!(result.error.unwrap().message.contains("output exceeded"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn denies_unregistered_tool_paths_before_host_call() {
        let registry = ToolRegistry {
            tools: vec![ToolDescriptor::new("example.allowed", "allowed")],
        };
        let engine = ExecutionEngine::new(
            registry,
            QuickJsRuntime,
            Arc::new(MemoryInvoker::new(HashMap::from([(
                "example.hidden".to_string(),
                json!({"leaked": true}),
            )]))),
        );
        let result = engine
            .execute("return await tools.example.hidden({});")
            .await;
        assert!(result.ok, "{:?}", result.error);
        assert_eq!(result.result.unwrap()["error"]["code"], "unknown_tool");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn direct_runtime_use_enforces_registry_and_builtins() {
        let input = RuntimeInput {
            source: "const search = await tools.search({query:'allowed',limit:1}); const hidden = await tools.example.hidden({}); return {search: search.data[0].path, hidden: hidden.error.code};".to_string(),
            registry: ToolRegistry {
                tools: vec![ToolDescriptor::new("example.allowed", "allowed")],
            },
            aliases: HashMap::new(),
            limits: RuntimeLimits::default(),
        };
        let output = QuickJsRuntime
            .execute(
                input,
                Arc::new(MemoryInvoker::new(HashMap::from([(
                    "example.hidden".to_string(),
                    json!({"leaked": true}),
                )]))),
            )
            .await
            .unwrap();
        assert_eq!(
            output.result,
            json!({"search":"example.allowed","hidden":"unknown_tool"})
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn direct_runtime_use_rejects_reserved_registry_paths() {
        let input = RuntimeInput {
            source: "return 1;".to_string(),
            registry: ToolRegistry {
                tools: vec![ToolDescriptor::new("tools.search", "search")],
            },
            aliases: HashMap::new(),
            limits: RuntimeLimits::default(),
        };
        let error = QuickJsRuntime
            .execute(input, Arc::new(MemoryInvoker::default()))
            .await
            .unwrap_err();
        assert_eq!(error.phase, FailurePhase::Runtime);
        assert!(error.message.contains("reserved"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn alias_failures_throw_runtime_errors() {
        let registry = ToolRegistry {
            tools: vec![ToolDescriptor::new("eoka.fail", "fail")],
        };
        let engine = ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(FailingInvoker))
            .with_alias("eoka", "eoka");
        let result = engine.execute("await eoka.fail(); return 1;").await;
        assert!(!result.ok);
        assert_eq!(result.error.unwrap().phase, FailurePhase::Runtime);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ambient_network_and_process_are_absent() {
        let engine = ExecutionEngine::new(
            ToolRegistry::default(),
            QuickJsRuntime,
            Arc::new(MemoryInvoker::default()),
        );
        let result = engine
            .execute(
                "return {fetch: typeof fetch, process: typeof process, require: typeof require};",
            )
            .await;
        assert!(result.ok, "{:?}", result.error);
        assert_eq!(
            result.result,
            Some(json!({"fetch":"undefined","process":"undefined","require":"undefined"}))
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn enforces_output_limit() {
        let limits = RuntimeLimits {
            max_output_bytes: 32,
            ..RuntimeLimits::default()
        };
        let engine = ExecutionEngine::new(
            ToolRegistry::default(),
            QuickJsRuntime,
            Arc::new(MemoryInvoker::default()),
        )
        .with_limits(limits);
        let result = engine.execute("return 'x'.repeat(100);").await;
        assert!(!result.ok);
        assert!(result.error.unwrap().message.contains("output exceeded"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn enforces_tool_call_limit() {
        let registry = ToolRegistry {
            tools: vec![ToolDescriptor::new("example.echo", "echo")],
        };
        let mut outputs = HashMap::new();
        outputs.insert("example.echo".to_string(), json!({"ok": true}));
        let limits = RuntimeLimits {
            max_tool_calls: 1,
            ..RuntimeLimits::default()
        };
        let engine = ExecutionEngine::new(
            registry,
            QuickJsRuntime,
            Arc::new(MemoryInvoker::new(outputs)),
        )
        .with_limits(limits);
        let result = engine
            .execute("await tools.example.echo({}); return await tools.example.echo({});")
            .await;
        assert!(result.ok, "{:?}", result.error);
        assert_eq!(result.result.unwrap()["ok"], false);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn enforces_tool_payload_limits() {
        let registry = ToolRegistry {
            tools: vec![ToolDescriptor::new("example.echo", "echo")],
        };
        let mut outputs = HashMap::new();
        outputs.insert(
            "example.echo".to_string(),
            json!({"value": "x".repeat(100)}),
        );
        let limits = RuntimeLimits {
            max_tool_request_bytes: 40,
            max_tool_response_bytes: 40,
            ..RuntimeLimits::default()
        };
        let engine = ExecutionEngine::new(
            registry,
            QuickJsRuntime,
            Arc::new(MemoryInvoker::new(outputs)),
        )
        .with_limits(limits.clone());
        let request_result = engine
            .execute("return await tools.example.echo({value: 'x'.repeat(100)});")
            .await;
        assert!(request_result.ok, "{:?}", request_result.error);
        let request = request_result.result.unwrap();
        assert_eq!(request["ok"], false);
        assert_eq!(request["error"]["message"], "tool request too large");

        let engine = ExecutionEngine::new(
            ToolRegistry {
                tools: vec![ToolDescriptor::new("example.echo", "echo")],
            },
            QuickJsRuntime,
            Arc::new(MemoryInvoker::new(HashMap::from([(
                "example.echo".to_string(),
                json!({"value": "x".repeat(100)}),
            )]))),
        )
        .with_limits(RuntimeLimits {
            max_tool_request_bytes: 1_000,
            max_tool_response_bytes: 40,
            ..RuntimeLimits::default()
        });
        let response_result = engine.execute("return await tools.example.echo({});").await;
        assert!(response_result.ok, "{:?}", response_result.error);
        assert_eq!(response_result.result.unwrap()["ok"], false);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn preserves_explicit_null_tool_input() {
        let registry = ToolRegistry {
            tools: vec![ToolDescriptor::new("example.echo", "echo")],
        };
        let engine = ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(EchoInputInvoker));
        let result = engine
            .execute("return (await tools.example.echo(null)).data;")
            .await;
        assert!(result.ok, "{:?}", result.error);
        assert_eq!(result.result, Some(Value::Null));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn invokes_paths_that_collide_with_proxy_then() {
        let registry = ToolRegistry {
            tools: vec![ToolDescriptor::new("example.then", "then")],
        };
        let engine = ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(EchoInputInvoker));
        let result = engine
            .execute("return (await tools.invoke('example.then', {value: 9})).data.value;")
            .await;
        assert!(result.ok, "{:?}", result.error);
        assert_eq!(result.result, Some(json!(9)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn invoke_rejects_missing_tool_paths_locally() {
        let engine = ExecutionEngine::new(
            ToolRegistry::default(),
            QuickJsRuntime,
            Arc::new(MemoryInvoker::default()),
        );
        let result = engine.execute("return await tools.invoke();").await;
        assert!(result.ok, "{:?}", result.error);
        assert_eq!(
            result.result,
            Some(json!({"ok":false,"error":{"message":"tool path is required"}}))
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn hides_mutable_host_bindings_from_user_code() {
        let registry = ToolRegistry {
            tools: vec![ToolDescriptor::new("example.echo", "echo")],
        };
        let engine = ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(EchoInputInvoker));
        let result = engine
            .execute(
                "const before = typeof globalThis.__tackCallJson; try { globalThis.__tackCallJson = () => '{}'; } catch (_error) {} return {before, after: (await tools.example.echo({value: 4})).data.value};",
            )
            .await;
        assert!(result.ok, "{:?}", result.error);
        assert_eq!(result.result, Some(json!({"before":"undefined","after":4})));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn captures_json_intrinsics_before_user_code_runs() {
        let registry = ToolRegistry {
            tools: vec![ToolDescriptor::new("example.echo", "echo")],
        };
        let engine = ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(EchoInputInvoker));
        let result = engine
            .execute(
                "JSON.stringify = () => '{\"value\":\"corrupted\"}'; JSON.parse = () => ({ok: false}); return (await tools.example.echo({value: 6})).data.value;",
            )
            .await;
        assert!(result.ok, "{:?}", result.error);
        assert_eq!(result.result, Some(json!(6)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn captures_prototype_intrinsics_before_user_code_runs() {
        let registry = ToolRegistry {
            tools: vec![ToolDescriptor::new("eoka.echo", "echo")],
        };
        let engine = ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(EchoInputInvoker))
            .with_alias("eoka", "eoka");
        let result = engine
            .execute(
                "Function.prototype.call = () => false; Array.prototype.map = () => ['bad']; Array.prototype.push = () => {}; Array.prototype.join = () => 'bad'; console.log('ok', {value: 1}); return (await eoka.echo({value: 7})).value;",
            )
            .await;
        assert!(result.ok, "{:?}", result.error);
        assert_eq!(result.result, Some(json!(7)));
        assert_eq!(result.logs, vec!["ok {\"value\":1}".to_string()]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn times_out_pending_promises() {
        let engine = ExecutionEngine::new(
            ToolRegistry::default(),
            QuickJsRuntime,
            Arc::new(MemoryInvoker::default()),
        )
        .with_limits(RuntimeLimits {
            timeout_ms: 25,
            ..RuntimeLimits::default()
        });
        let result = engine.execute("return await new Promise(() => {});").await;
        assert!(!result.ok);
        assert_eq!(result.error.unwrap().phase, FailurePhase::Timeout);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn direct_runtime_rejects_invalid_runtime_limits_before_timeout() {
        let input = RuntimeInput {
            source: "return 1;".to_string(),
            registry: ToolRegistry::default(),
            aliases: HashMap::new(),
            limits: RuntimeLimits {
                timeout_ms: 0,
                ..RuntimeLimits::default()
            },
        };
        let error = QuickJsRuntime
            .execute(input, Arc::new(MemoryInvoker::default()))
            .await
            .unwrap_err();
        assert_eq!(error.phase, FailurePhase::Runtime);
        assert!(error.message.contains("timeoutMs"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn tool_request_serialization_errors_return_envelopes() {
        let registry = ToolRegistry {
            tools: vec![ToolDescriptor::new("example.echo", "echo")],
        };
        let engine = ExecutionEngine::new(
            registry,
            QuickJsRuntime,
            Arc::new(MemoryInvoker::new(HashMap::from([(
                "example.echo".to_string(),
                json!({"unexpected":"host call"}),
            )]))),
        );
        let result = engine
            .execute(
                "const input = {}; input.self = input; return await tools.example.echo(input);",
            )
            .await;
        assert!(result.ok, "{:?}", result.error);
        let value = result.result.unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(
            value["error"]["message"],
            "tool request is not JSON serializable"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn result_serialization_matches_json_stringify_semantics() {
        let engine = ExecutionEngine::new(
            ToolRegistry::default(),
            QuickJsRuntime,
            Arc::new(MemoryInvoker::default()),
        );

        let undefined = engine.execute("return undefined;").await;
        assert!(undefined.ok, "{:?}", undefined.error);
        assert_eq!(undefined.result, Some(Value::Null));

        let bigint = engine.execute("return 1n;").await;
        assert!(!bigint.ok);
        assert_eq!(bigint.error.unwrap().phase, FailurePhase::Runtime);

        let cyclic = engine
            .execute("const value = {}; value.self = value; return value;")
            .await;
        assert!(!cyclic.ok);
        assert_eq!(cyclic.error.unwrap().phase, FailurePhase::Runtime);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn enforces_tool_call_timeout() {
        let registry = ToolRegistry {
            tools: vec![ToolDescriptor::new("example.hang", "hang")],
        };
        let engine = ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(HangingInvoker))
            .with_limits(RuntimeLimits {
                tool_call_timeout_ms: 10,
                ..RuntimeLimits::default()
            });
        let result = engine.execute("return await tools.example.hang({});").await;
        assert!(result.ok, "{:?}", result.error);
        assert_eq!(
            result.result.unwrap()["error"]["message"],
            "tool call timed out"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn interrupts_cpu_bound_code_at_timeout() {
        let limits = RuntimeLimits {
            timeout_ms: 25,
            ..RuntimeLimits::default()
        };
        let engine = ExecutionEngine::new(
            ToolRegistry::default(),
            QuickJsRuntime,
            Arc::new(MemoryInvoker::default()),
        )
        .with_limits(limits);
        let result = engine.execute("while (true) {}").await;
        assert!(!result.ok);
        assert_eq!(result.error.unwrap().phase, FailurePhase::Timeout);
    }

    #[test]
    fn transforms_typescript() {
        let code = transform_typescript("const value: number = 2; return value;").unwrap();
        assert!(code.contains("const value = 2"));
        assert!(code.contains("function"));
    }

    struct DelayedInvoker;

    #[async_trait]
    impl ToolInvoker for DelayedInvoker {
        async fn call(&self, call: ToolCall) -> ToolCallOutput {
            tokio::time::sleep(Duration::from_millis(10)).await;
            ToolCallOutput::data(json!({"value": call.input["value"].as_i64().unwrap()}))
        }
    }

    struct FailingInvoker;

    #[async_trait]
    impl ToolInvoker for FailingInvoker {
        async fn call(&self, _call: ToolCall) -> ToolCallOutput {
            ToolCallOutput::failure("boom")
        }
    }

    struct EchoInputInvoker;

    #[async_trait]
    impl ToolInvoker for EchoInputInvoker {
        async fn call(&self, call: ToolCall) -> ToolCallOutput {
            ToolCallOutput::data(call.input)
        }
    }

    struct HangingInvoker;

    #[async_trait]
    impl ToolInvoker for HangingInvoker {
        async fn call(&self, _call: ToolCall) -> ToolCallOutput {
            tokio::time::sleep(Duration::from_secs(1)).await;
            ToolCallOutput::data(json!({"done": true}))
        }
    }
}
