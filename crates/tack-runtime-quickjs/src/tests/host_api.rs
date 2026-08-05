use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn ambient_network_and_process_are_absent() {
    let engine = ExecutionEngine::new(
        ToolRegistry::default(),
        QuickJsRuntime,
        Arc::new(MemoryInvoker::default()),
    );
    let result = engine
        .execute("return {fetch: typeof fetch, process: typeof process, require: typeof require};")
        .await;
    assert!(result.ok, "{:?}", result.error);
    assert_eq!(
        result.result,
        Some(json!({"fetch":"undefined","process":"undefined","require":"undefined"}))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn denies_unregistered_tool_paths_before_host_call() {
    let registry = registry(vec![ToolDescriptor::new("example.allowed", "allowed")]);
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
async fn preserves_explicit_null_tool_input() {
    let registry = registry(vec![ToolDescriptor::new("example.echo", "echo")]);
    let engine = ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(EchoInputInvoker));
    let result = engine
        .execute("return (await tools.example.echo(null)).data;")
        .await;
    assert!(result.ok, "{:?}", result.error);
    assert_eq!(result.result, Some(Value::Null));
}

#[tokio::test(flavor = "multi_thread")]
async fn invokes_paths_that_collide_with_proxy_then() {
    let registry = registry(vec![ToolDescriptor::new("example.then", "then")]);
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
    let registry = registry(vec![ToolDescriptor::new("example.echo", "echo")]);
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
    let registry = registry(vec![ToolDescriptor::new("example.echo", "echo")]);
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
    let registry = registry(vec![ToolDescriptor::new("eoka.echo", "echo")]);
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
