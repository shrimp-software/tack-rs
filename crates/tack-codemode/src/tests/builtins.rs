use super::*;

#[tokio::test]
async fn builtins_search_and_describe() {
    let registry = ToolRegistry {
        tools: vec![ToolDescriptor {
            path: "eoka.eval".to_string(),
            tool_id: None,
            name: "eval".to_string(),
            title: Some("Evaluate JavaScript".to_string()),
            description: Some("Run JavaScript in the active page".to_string()),
            input_schema: json!({"type":"object","required":["code"],"properties":{"code":{"type":"string"}}}),
            output_schema: json!({"type":"object","properties":{"value":{"type":"string"}}}),
            tags: vec!["browser".to_string()],
        }],
    };
    let invoker = BuiltinAwareInvoker::new(registry.clone(), Arc::new(MemoryInvoker::default()));
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
async fn builtin_invoke_dispatches_to_target_tool() {
    let registry = ToolRegistry {
        tools: vec![ToolDescriptor::new("eoka.eval", "eval")],
    };
    let invoker = BuiltinAwareInvoker::new(
        registry,
        Arc::new(MemoryInvoker::new(HashMap::from([(
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
    let registry = ToolRegistry {
        tools: vec![
            ToolDescriptor::new("example.visible", "visible"),
            ToolDescriptor::new("example.secret", "secret"),
        ],
    };
    let engine = ExecutionEngine::new(
        registry,
        StaticRuntime::ok(json!(null)),
        Arc::new(MemoryInvoker::default()),
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
