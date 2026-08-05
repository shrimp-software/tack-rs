use super::*;

#[tokio::test]
async fn builtins_search_and_describe() {
    let registry = registry(vec![
        ToolDescriptor::new("eoka.eval", "eval")
            .with_title("Evaluate JavaScript")
            .with_description("Run JavaScript in the active page")
            .with_input_schema(json!({"type":"object","required":["code"],"properties":{"code":{"type":"string"}}}))
            .with_output_schema(json!({"type":"object","properties":{"value":{"type":"string"}}}))
            .with_tag("browser"),
    ]);
    let invoker = BuiltinAwareInvoker::new(registry.clone(), Arc::new(TestInvoker::default()));
    let search = invoker
        .call(ToolCall {
            path: "tools.search".to_string(),
            input: json!({"query":"javascript","limit":5}),
        })
        .await;
    assert!(search.ok);
    assert_eq!(search.data.unwrap()[0]["path"], "eoka.eval");

    let description = describe_tool(&registry, "eoka.eval").unwrap();
    assert!(description.type_signature.contains("code: string"));
}

#[tokio::test]
async fn builtins_search_indexes_tags_and_annotations() {
    let registry = registry(vec![
        ToolDescriptor::new("browser.storage.clear", "clear")
            .with_description("Clear browser storage")
            .with_tag("storage")
            .with_destructive(true)
            .with_capability("browser.storage"),
        ToolDescriptor::new("browser.info", "info")
            .with_description("Read page info")
            .with_read_only(true)
            .with_tag("page"),
    ]);
    let invoker = BuiltinAwareInvoker::new(registry, Arc::new(TestInvoker::default()));

    let by_tag = invoker
        .call(ToolCall {
            path: "tools.search".to_string(),
            input: json!({"query":"storage","limit":5}),
        })
        .await;
    assert_eq!(by_tag.data.unwrap()[0]["path"], "browser.storage.clear");

    let by_annotation = invoker
        .call(ToolCall {
            path: "tools.search".to_string(),
            input: json!({"query":"read only","limit":5}),
        })
        .await;
    assert_eq!(by_annotation.data.unwrap()[0]["path"], "browser.info");
}

#[tokio::test]
async fn builtin_invoke_dispatches_to_target_tool() {
    let registry = registry(vec![ToolDescriptor::new("eoka.eval", "eval")]);
    let invoker = BuiltinAwareInvoker::new(
        registry,
        Arc::new(TestInvoker::new(HashMap::from([(
            "eoka.eval".to_string(),
            json!({"value": 11}),
        )]))),
    );
    let output = invoker
        .call(ToolCall {
            path: "tools.invoke".to_string(),
            input: json!({"path":"eoka.eval","input":{"code":"return 11"}}),
        })
        .await;
    assert!(output.ok);
    assert_eq!(output.data, Some(json!({"value": 11})));

    let missing_path = invoker
        .call(ToolCall {
            path: "tools.invoke".to_string(),
            input: json!({}),
        })
        .await;
    assert!(!missing_path.ok);
}

#[tokio::test]
async fn policy_filters_discovery_registry() {
    let registry = registry(vec![
        ToolDescriptor::new("example.visible", "visible"),
        ToolDescriptor::new("example.secret", "secret"),
    ]);
    let engine = ExecutionEngine::new(
        registry,
        TestRuntime::ok(json!(null)),
        Arc::new(TestInvoker::default()),
    )
    .with_policy(Arc::new(DenyListPolicy::new(vec![
        "example.secret".to_string(),
    ])));
    let invoker = BuiltinAwareInvoker::new(engine.execution_registry(), engine.host_invoker());
    let search = invoker
        .call(ToolCall {
            path: "tools.search".to_string(),
            input: json!({"query":"example","limit":10}),
        })
        .await;
    assert_eq!(search.data.unwrap().as_array().unwrap().len(), 1);

    let description = invoker
        .call(ToolCall {
            path: "tools.describe.tool".to_string(),
            input: json!({"path":"example.secret"}),
        })
        .await;
    assert!(!description.ok);
}
