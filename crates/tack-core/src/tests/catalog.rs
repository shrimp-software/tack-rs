use super::*;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

struct RecordingLayer {
    label: &'static str,
    events: Arc<Mutex<Vec<String>>>,
}

impl ToolLayer for RecordingLayer {
    fn layer(&self, inner: Arc<dyn ToolInvoker>) -> Arc<dyn ToolInvoker> {
        Arc::new(RecordingInvoker {
            label: self.label,
            events: self.events.clone(),
            inner,
        })
    }
}

struct RecordingInvoker {
    label: &'static str,
    events: Arc<Mutex<Vec<String>>>,
    inner: Arc<dyn ToolInvoker>,
}

#[async_trait]
impl ToolInvoker for RecordingInvoker {
    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        self.events
            .lock()
            .unwrap()
            .push(format!("{}:before", self.label));
        let output = self.inner.call(call).await;
        self.events
            .lock()
            .unwrap()
            .push(format!("{}:after", self.label));
        output
    }
}

struct DenyLayer;

impl ToolLayer for DenyLayer {
    fn layer(&self, _inner: Arc<dyn ToolInvoker>) -> Arc<dyn ToolInvoker> {
        Arc::new(DenyInvoker)
    }
}

struct DenyInvoker;

#[async_trait]
impl ToolInvoker for DenyInvoker {
    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        ToolCallOutput::failure_with_code(
            format!("denied by layer: {}", call.path),
            Some("layer_denied".to_string()),
        )
    }
}

#[tokio::test]
async fn catalog_registers_and_invokes_function_tools() {
    let mut catalog = ToolCatalog::new();
    catalog
        .register_fn(
            ToolDescriptor::new("example.echo", "echo").with_description("Echo input"),
            |input| async move { ToolCallOutput::data(json!({ "input": input })) },
        )
        .unwrap();

    let registry = catalog.registry();
    assert_eq!(registry.len(), 1);
    assert_eq!(registry.as_slice()[0].path(), "example.echo");

    let output = catalog
        .call(ToolCall {
            path: "example.echo".to_string(),
            input: json!({ "value": 7 }),
        })
        .await;
    assert!(output.ok);
    assert_eq!(output.data, Some(json!({ "input": { "value": 7 } })));
}

#[tokio::test]
async fn catalog_layers_wrap_dispatch_in_registration_order() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut catalog = ToolCatalog::new()
        .with_layer(RecordingLayer {
            label: "outer",
            events: events.clone(),
        })
        .with_layer(RecordingLayer {
            label: "inner",
            events: events.clone(),
        });
    catalog
        .register_fn(ToolDescriptor::new("example.ok", "ok"), |_| async {
            ToolCallOutput::data(json!(true))
        })
        .unwrap();

    let output = catalog
        .call(ToolCall {
            path: "example.ok".to_string(),
            input: json!({}),
        })
        .await;

    assert!(output.ok);
    assert_eq!(
        *events.lock().unwrap(),
        vec!["outer:before", "inner:before", "inner:after", "outer:after"]
    );
}

#[tokio::test]
async fn catalog_layers_can_short_circuit_dispatch() {
    let mut catalog = ToolCatalog::new().with_layer(DenyLayer);
    catalog
        .register_fn(ToolDescriptor::new("example.ok", "ok"), |_| async {
            ToolCallOutput::data(json!(true))
        })
        .unwrap();

    let output = catalog
        .call(ToolCall {
            path: "example.ok".to_string(),
            input: json!({}),
        })
        .await;

    assert!(!output.ok);
    assert_eq!(output.error.unwrap().code.as_deref(), Some("layer_denied"));
}

#[tokio::test]
async fn call_function_tools_receive_resolved_tool_call() {
    let mut catalog = ToolCatalog::new();
    catalog
        .register_call_fn(
            ToolDescriptor::new("example.echo", "echo"),
            |call| async move {
                ToolCallOutput::data(json!({
                    "path": call.path,
                    "input": call.input
                }))
            },
        )
        .unwrap();

    let output = catalog
        .call(ToolCall {
            path: "example.echo".to_string(),
            input: json!({ "value": 7 }),
        })
        .await;

    assert_eq!(
        output.data,
        Some(json!({
            "path": "example.echo",
            "input": { "value": 7 }
        }))
    );
}

#[tokio::test]
async fn catalog_returns_unknown_tool_for_missing_paths() {
    let catalog = ToolCatalog::new();
    let output = catalog
        .call(ToolCall {
            path: "example.missing".to_string(),
            input: json!({}),
        })
        .await;

    assert!(!output.ok);
    assert_eq!(output.error.unwrap().code.as_deref(), Some("unknown_tool"));
}

#[test]
fn catalog_rejects_invalid_and_duplicate_paths_without_mutating() {
    let mut catalog = ToolCatalog::new();
    catalog
        .register_fn(ToolDescriptor::new("example.one", "one"), |_| async {
            ToolCallOutput::data(json!(true))
        })
        .unwrap();

    let duplicate = catalog
        .register_fn(ToolDescriptor::new("example.one", "one"), |_| async {
            ToolCallOutput::data(json!(true))
        })
        .unwrap_err();
    assert!(matches!(
        duplicate,
        CatalogError::Registry(RegistryValidationError::DuplicateToolPath(path))
            if path == "example.one"
    ));

    let invalid = catalog
        .register_fn(ToolDescriptor::new("example.bad-path", "bad"), |_| async {
            ToolCallOutput::data(json!(true))
        })
        .unwrap_err();
    assert!(matches!(
        invalid,
        CatalogError::Registry(RegistryValidationError::InvalidToolPath { .. })
    ));

    assert_eq!(catalog.len(), 1);
    assert!(catalog.registry().contains_path("example.one"));
}

#[tokio::test]
async fn catalog_mount_prefixes_relative_tool_paths() {
    let mut child = ToolCatalog::new();
    child
        .register_fn(ToolDescriptor::new("open", "open"), |input| async move {
            ToolCallOutput::data(input)
        })
        .unwrap();
    child
        .register_fn(ToolDescriptor::new("tab.new", "new"), |input| async move {
            ToolCallOutput::data(input)
        })
        .unwrap();

    let mut catalog = ToolCatalog::new();
    catalog.mount("eoka", child).unwrap();

    let registry = catalog.registry();
    assert!(registry.contains_path("eoka.open"));
    assert!(registry.contains_path("eoka.tab.new"));

    let output = catalog
        .call(ToolCall {
            path: "eoka.open".to_string(),
            input: json!({ "url": "https://example.com" }),
        })
        .await;
    assert!(output.ok);
    assert_eq!(output.data, Some(json!({ "url": "https://example.com" })));
}

#[tokio::test]
async fn catalog_mount_accepts_borrowed_tool_sets() {
    let mut child = ToolCatalog::new();
    child
        .register_fn(ToolDescriptor::new("open", "open"), |_| async {
            ToolCallOutput::data(json!(true))
        })
        .unwrap();

    let mut catalog = ToolCatalog::new();
    catalog.mount("one", &child).unwrap();
    catalog.mount("two", &child).unwrap();

    assert!(catalog.registry().contains_path("one.open"));
    assert!(catalog.registry().contains_path("two.open"));
}

#[tokio::test]
async fn mounted_call_tools_receive_mounted_path() {
    let mut child = ToolCatalog::new();
    child
        .register_call_fn(ToolDescriptor::new("open", "open"), |call| async move {
            ToolCallOutput::data(json!({ "path": call.path }))
        })
        .unwrap();

    let mut catalog = ToolCatalog::new();
    catalog.mount("eoka", child).unwrap();

    let output = catalog
        .call(ToolCall {
            path: "eoka.open".to_string(),
            input: json!({}),
        })
        .await;
    assert_eq!(output.data, Some(json!({ "path": "eoka.open" })));
}

#[tokio::test]
async fn execution_engine_can_be_built_from_catalog() {
    let mut catalog = ToolCatalog::new();
    catalog
        .register_fn(ToolDescriptor::new("example.echo", "echo"), |_| async {
            ToolCallOutput::data(json!(true))
        })
        .unwrap();

    let engine = ExecutionEngine::from_catalog(catalog, TestRuntime::ok(json!(5)));
    let result = engine.execute("return 5").await;
    assert!(result.ok);
    assert_eq!(result.result, Some(json!(5)));
}

#[cfg(feature = "schemars")]
#[derive(Deserialize, schemars::JsonSchema)]
struct TypedInput {
    value: i64,
}

#[cfg(feature = "schemars")]
#[derive(Serialize, schemars::JsonSchema)]
struct TypedOutput {
    doubled: i64,
}

#[cfg(feature = "schemars")]
#[tokio::test]
async fn typed_function_tools_generate_schemas_and_validate_input() {
    let mut catalog = ToolCatalog::new();
    catalog
        .register_typed_fn(
            ToolDescriptor::new("example.double", "double"),
            |input: TypedInput| async move {
                Ok(TypedOutput {
                    doubled: input.value * 2,
                })
            },
        )
        .unwrap();

    let descriptor = catalog
        .registry()
        .by_path("example.double")
        .cloned()
        .unwrap();
    assert_eq!(descriptor.input_schema()["type"], "object");
    assert_eq!(descriptor.output_schema()["type"], "object");

    let output = catalog
        .call(ToolCall {
            path: "example.double".to_string(),
            input: json!({ "value": 9 }),
        })
        .await;
    assert_eq!(output.data, Some(json!({ "doubled": 18 })));

    let output = catalog
        .call(ToolCall {
            path: "example.double".to_string(),
            input: json!({ "value": "bad" }),
        })
        .await;
    assert!(!output.ok);
    assert_eq!(output.error.unwrap().code.as_deref(), Some("invalid_input"));
}

#[cfg(feature = "schemars")]
struct DoubleTool;

#[cfg(feature = "schemars")]
#[async_trait]
impl TypedTool for DoubleTool {
    type Input = TypedInput;
    type Output = TypedOutput;

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::new("example.typed", "typed")
    }

    async fn call_typed(&self, input: Self::Input) -> ToolResult<Self::Output> {
        Ok(TypedOutput {
            doubled: input.value * 2,
        })
    }
}

#[cfg(feature = "schemars")]
#[tokio::test]
async fn typed_tool_trait_adapts_into_catalog_tools() {
    let mut catalog = ToolCatalog::new();
    catalog.register_typed(DoubleTool).unwrap();

    let descriptor = catalog
        .registry()
        .by_path("example.typed")
        .cloned()
        .unwrap();
    assert_eq!(descriptor.input_schema()["type"], "object");
    assert_eq!(descriptor.output_schema()["type"], "object");

    let output = catalog
        .call(ToolCall {
            path: "example.typed".to_string(),
            input: json!({ "value": 11 }),
        })
        .await;
    assert_eq!(output.data, Some(json!({ "doubled": 22 })));
}
