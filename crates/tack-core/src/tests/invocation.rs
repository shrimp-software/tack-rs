use super::*;

#[tokio::test]
async fn policy_denies_exact_and_wildcard_paths() {
    let policy = Arc::new(DenyListPolicy::new(vec![
        "example.secret".to_string(),
        "example.admin.*".to_string(),
    ]));
    let invoker = PolicyAwareInvoker::new(policy, Arc::new(TestInvoker::default()));
    let exact = invoker
        .call(ToolCall {
            path: "example.secret".to_string(),
            input: json!({}),
        })
        .await;
    assert!(!exact.ok);
    assert_eq!(exact.error.unwrap().code.as_deref(), Some("policy_denied"));

    let wildcard = invoker
        .call(ToolCall {
            path: "example.admin.delete".to_string(),
            input: json!({}),
        })
        .await;
    assert!(!wildcard.ok);
}

#[tokio::test]
async fn registry_denies_undeclared_tool_paths() {
    let invoker = RegistryAwareInvoker::new(
        registry(vec![ToolDescriptor::new("example.allowed", "allowed")]),
        Arc::new(TestInvoker::new(HashMap::from([(
            "example.hidden".to_string(),
            json!({"leaked": true}),
        )]))),
    );
    let output = invoker
        .call(ToolCall {
            path: "example.hidden".to_string(),
            input: json!({}),
        })
        .await;
    assert!(!output.ok);
    assert_eq!(output.error.unwrap().code.as_deref(), Some("unknown_tool"));
}

#[test]
fn tool_call_output_omits_absent_fields() {
    let value = serde_json::to_value(ToolCallOutput::data(json!({"ok":true}))).unwrap();
    let object = value.as_object().unwrap();
    assert_eq!(object.get("ok"), Some(&Value::Bool(true)));
    assert!(!object.contains_key("error"));
}
