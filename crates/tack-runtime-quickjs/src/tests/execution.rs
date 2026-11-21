use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn executes_typescript_and_tools() {
    let registry = registry(vec![ToolDescriptor::new("example.echo", "echo")]);
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
async fn runs_stripped_typescript() {
    let engine = ExecutionEngine::new(
        registry(Vec::new()),
        QuickJsRuntime,
        Arc::new(MemoryInvoker::default()),
    );
    let result = engine
        .execute(
            "interface Shape { area(): number }
             abstract class Base<T> implements Shape {
               protected readonly scale: number = 2;
               declare tag: string;
               abstract area(): number;
             }
             class Square extends Base<number> {
               constructor(private_side?: number) { super(); this.side = private_side!; }
               side!: number;
               override area(): number { return (this.side as number) ** 2 * this.scale; }
             }
             const pick = <T,>(items: T[], index?: number): T => items[index ?? 0]!;
             return pick<Shape>([new Square(3) satisfies Shape]).area();",
        )
        .await;
    assert!(result.ok, "{:?}", result.error);
    assert_eq!(result.result, Some(json!(18)));
}

#[tokio::test(flavor = "multi_thread")]
async fn early_errors_surface_from_quickjs() {
    let engine = ExecutionEngine::new(
        registry(Vec::new()),
        QuickJsRuntime,
        Arc::new(MemoryInvoker::default()),
    );
    let result = engine.execute("let a = 1; let a = 2; return a;").await;
    assert!(!result.ok);
    let error = result.error.unwrap();
    assert_eq!(error.phase, FailurePhase::Runtime, "{error:?}");
    assert!(error.message.contains("redefinition"), "{error:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn aliases_return_data_directly() {
    let registry = registry(vec![ToolDescriptor::new("eoka.info", "info")]);
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
    let registry = registry(vec![ToolDescriptor::new("example.fetch", "fetch")]);
    let engine = ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(MemoryInvoker::default()));
    let result = engine
        .execute("return (await tools.search({query:'fetch',limit:1})).data[0].path;")
        .await;
    assert!(result.ok, "{:?}", result.error);
    assert_eq!(result.result, Some(json!("example.fetch")));
}

#[tokio::test(flavor = "multi_thread")]
async fn policy_filters_builtin_discovery_in_runtime() {
    let registry = registry(vec![
        ToolDescriptor::new("example.visible", "visible"),
        ToolDescriptor::new("example.secret", "secret"),
    ]);
    let engine = ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(MemoryInvoker::default()))
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
    let registry = registry(vec![ToolDescriptor::new("example.slow", "slow")]);
    let engine = ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(DelayedInvoker));
    let result = engine
        .execute("const a = await tools.example.slow({value: 2}); const b = await tools.example.slow({value: 3}); return a.data.value + b.data.value;")
        .await;
    assert!(result.ok, "{:?}", result.error);
    assert_eq!(result.result, Some(json!(5)));
}

#[tokio::test(flavor = "multi_thread")]
async fn direct_runtime_use_enforces_registry_and_builtins() {
    let input = RuntimeInput {
        source: "const search = await tools.search({query:'allowed',limit:1}); const hidden = await tools.example.hidden({}); return {search: search.data[0].path, hidden: hidden.error.code};".to_string(),
        registry: registry(vec![ToolDescriptor::new("example.allowed", "allowed")]),
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
    let registry = serde_json::from_value(json!({
        "tools": [{"path": "tools.search", "name": "search"}]
    }))
    .unwrap();
    let input = RuntimeInput {
        source: "return 1;".to_string(),
        registry,
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
    let registry = registry(vec![ToolDescriptor::new("eoka.fail", "fail")]);
    let engine = ExecutionEngine::new(registry, QuickJsRuntime, Arc::new(FailingInvoker))
        .with_alias("eoka", "eoka");
    let result = engine.execute("await eoka.fail(); return 1;").await;
    assert!(!result.ok);
    assert_eq!(result.error.unwrap().phase, FailurePhase::Runtime);
}
