use serde_json::{Value, json};
use tack_core::{ExecutionEngine, ToolAnnotations, ToolCallOutput, ToolCatalog, ToolDescriptor};
use tack_runtime_quickjs::QuickJsRuntime;

#[derive(Clone, Copy)]
struct CommandSpec {
    path: &'static str,
    command: &'static str,
    description: &'static str,
    read_only: bool,
    destructive: bool,
}

const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        path: "open",
        command: "open",
        description: "Navigate to a URL",
        read_only: false,
        destructive: false,
    },
    CommandSpec {
        path: "info",
        command: "info",
        description: "Read page URL and title",
        read_only: true,
        destructive: false,
    },
    CommandSpec {
        path: "storage.clear",
        command: "clear_storage",
        description: "Clear browser storage",
        read_only: false,
        destructive: true,
    },
];

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut browser = ToolCatalog::new();
    for spec in COMMANDS {
        let descriptor = ToolDescriptor::new(spec.path, spec.command)
            .with_description(spec.description)
            .with_annotations(
                ToolAnnotations::new()
                    .read_only(spec.read_only)
                    .destructive(spec.destructive)
                    .capability("browser.command"),
            );
        browser
            .register_call_fn(descriptor, move |call| async move {
                ToolCallOutput::data(dispatch_command(spec, call.input))
            })
            .unwrap();
    }

    let mut catalog = ToolCatalog::new();
    catalog.mount("browser", browser).unwrap();

    let engine =
        ExecutionEngine::from_catalog(catalog, QuickJsRuntime).with_alias("browser", "browser");
    let output = engine
        .execute(
            r#"
            const page = await browser.info({});
            const storage = await tools.describe.tool({ path: "browser.storage.clear" });
            emit({ destructive: storage.data.descriptor.annotations.destructive });
            return page;
            "#,
        )
        .await;

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

fn dispatch_command(spec: &CommandSpec, input: Value) -> Value {
    match spec.command {
        "open" => json!({
            "ok": true,
            "url": input.get("url").and_then(Value::as_str).unwrap_or("about:blank")
        }),
        "info" => json!({
            "url": "https://example.com",
            "title": "Example Domain"
        }),
        "clear_storage" => json!({ "cleared": true }),
        _ => json!({ "unknown": spec.command }),
    }
}
