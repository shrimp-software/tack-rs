use crate::ident::is_js_identifier;
use crate::tools::{
    ToolCall, ToolCallOutput, ToolDescriptor, ToolInvoker, ToolPolicy, ToolRegistry,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub struct DenyListPolicy {
    denied_paths: Vec<String>,
}

impl DenyListPolicy {
    pub fn new(denied_paths: Vec<String>) -> Self {
        Self { denied_paths }
    }
}

impl ToolPolicy for DenyListPolicy {
    fn is_allowed(&self, path: &str) -> bool {
        !self.denied_paths.iter().any(|denied| {
            denied == path
                || denied
                    .strip_suffix(".*")
                    .is_some_and(|prefix| path == prefix || path.starts_with(&format!("{prefix}.")))
        })
    }
}

pub struct PolicyAwareInvoker {
    policy: Arc<dyn ToolPolicy>,
    inner: Arc<dyn ToolInvoker>,
}

impl PolicyAwareInvoker {
    pub fn new(policy: Arc<dyn ToolPolicy>, inner: Arc<dyn ToolInvoker>) -> Self {
        Self { policy, inner }
    }
}

#[async_trait]
impl ToolInvoker for PolicyAwareInvoker {
    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        if !self.policy.is_allowed(&call.path) {
            let message = format!("Operation denied by policy: {}", call.path);
            let mut output = ToolCallOutput::failure_with_code(
                message.clone(),
                Some("policy_denied".to_string()),
            );
            output.text = Some(message);
            return output;
        }
        self.inner.call(call).await
    }
}

pub struct RegistryAwareInvoker {
    registry: ToolRegistry,
    inner: Arc<dyn ToolInvoker>,
}

impl RegistryAwareInvoker {
    pub fn new(registry: ToolRegistry, inner: Arc<dyn ToolInvoker>) -> Self {
        Self { registry, inner }
    }
}

#[async_trait]
impl ToolInvoker for RegistryAwareInvoker {
    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        if !self.registry.contains_path(&call.path) {
            let message = format!("Unknown tool: {}", call.path);
            let mut output = ToolCallOutput::failure_with_code(
                message.clone(),
                Some("unknown_tool".to_string()),
            );
            output.text = Some(message);
            return output;
        }
        self.inner.call(call).await
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub path: String,
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub score: usize,
}

pub fn search_tools(registry: &ToolRegistry, query: &str, limit: usize) -> Vec<SearchHit> {
    let terms = query
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();

    let mut hits = registry
        .tools
        .iter()
        .filter_map(|tool| {
            let text = [
                tool.path.as_str(),
                tool.name.as_str(),
                tool.title.as_deref().unwrap_or_default(),
                tool.description.as_deref().unwrap_or_default(),
            ]
            .join(" ")
            .to_ascii_lowercase();
            let score = if terms.is_empty() {
                1
            } else {
                terms
                    .iter()
                    .map(|term| {
                        if tool.path.eq_ignore_ascii_case(term)
                            || tool.name.eq_ignore_ascii_case(term)
                        {
                            10
                        } else if text.contains(term) {
                            3
                        } else {
                            0
                        }
                    })
                    .sum()
            };
            (score > 0).then(|| SearchHit {
                path: tool.path.clone(),
                name: tool.name.clone(),
                title: tool.title.clone(),
                description: tool.description.clone(),
                score,
            })
        })
        .collect::<Vec<_>>();

    hits.sort_by_key(|hit| (Reverse(hit.score), hit.path.clone()));
    hits.truncate(limit);
    hits
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolDescription {
    pub descriptor: ToolDescriptor,
    pub type_signature: String,
}

pub fn describe_tool(registry: &ToolRegistry, path: &str) -> Option<ToolDescription> {
    registry.by_path(path).map(|descriptor| ToolDescription {
        descriptor: descriptor.clone(),
        type_signature: format!(
            "(input: {}): Promise<ToolCallOutput<{}>>",
            schema_to_type(&descriptor.input_schema),
            schema_to_type(&descriptor.output_schema)
        ),
    })
}

pub(crate) fn schema_to_type(schema: &Value) -> String {
    if let Some(value) = schema.get("const") {
        return literal_type(value);
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        return union_type(values.iter().map(literal_type));
    }
    if let Some(values) = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .and_then(Value::as_array)
    {
        return union_type(values.iter().map(schema_to_type));
    }
    match schema.get("type") {
        Some(Value::String(kind)) => schema_kind_to_type(kind, schema),
        Some(Value::Array(kinds)) => union_type(
            kinds
                .iter()
                .filter_map(Value::as_str)
                .map(|kind| schema_kind_to_type(kind, schema)),
        ),
        _ if schema.is_null() => "unknown".to_string(),
        _ => "unknown".to_string(),
    }
}

fn schema_kind_to_type(kind: &str, schema: &Value) -> String {
    match kind {
        "string" => "string".to_string(),
        "number" | "integer" => "number".to_string(),
        "boolean" => "boolean".to_string(),
        "array" => array_schema_to_type(schema),
        "object" => object_schema_to_type(schema),
        "null" => "null".to_string(),
        _ => "unknown".to_string(),
    }
}

fn array_schema_to_type(schema: &Value) -> String {
    if let Some(items) = schema.get("prefixItems").and_then(Value::as_array) {
        let fields = items
            .iter()
            .map(schema_to_type)
            .collect::<Vec<_>>()
            .join(", ");
        return format!("[{fields}]");
    }
    let item = schema
        .get("items")
        .map(schema_to_type)
        .unwrap_or_else(|| "unknown".to_string());
    format!("{item}[]")
}

fn literal_type(value: &Value) -> String {
    match value {
        Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null => {
            serde_json::to_string(value).expect("json schema literal")
        }
        _ => "unknown".to_string(),
    }
}

fn union_type(types: impl IntoIterator<Item = String>) -> String {
    let mut unique = Vec::new();
    for ty in types {
        if !unique.contains(&ty) {
            unique.push(ty);
        }
    }
    if unique.is_empty() {
        "unknown".to_string()
    } else {
        unique.join(" | ")
    }
}

fn object_schema_to_type(schema: &Value) -> String {
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let additional = schema.get("additionalProperties");
    if properties.is_empty() {
        return match additional {
            Some(Value::Object(_)) => {
                format!("Record<string, {}>", schema_to_type(additional.unwrap()))
            }
            Some(Value::Bool(false)) => "{}".to_string(),
            _ => "Record<string, unknown>".to_string(),
        };
    }
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    let mut fields = properties
        .iter()
        .map(|(key, value)| {
            let optional = if required.contains(key) { "" } else { "?" };
            format!(
                "{}{optional}: {}",
                ts_property_name(key),
                schema_to_type(value)
            )
        })
        .collect::<Vec<_>>();
    if additional == Some(&Value::Bool(true)) {
        fields.push("[key: string]: unknown".to_string());
    }
    let fields = fields.join("; ");
    format!("{{ {fields} }}")
}

fn ts_property_name(key: &str) -> String {
    if is_js_identifier(key) {
        key.to_string()
    } else {
        serde_json::to_string(key).expect("typescript property json string")
    }
}

pub struct BuiltinAwareInvoker {
    registry: ToolRegistry,
    inner: Arc<dyn ToolInvoker>,
}

impl BuiltinAwareInvoker {
    pub fn new(registry: ToolRegistry, inner: Arc<dyn ToolInvoker>) -> Self {
        Self { registry, inner }
    }
}

#[async_trait]
impl ToolInvoker for BuiltinAwareInvoker {
    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        match call.path.as_str() {
            "tools.search" => {
                let query = call
                    .input
                    .get("query")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let limit = call
                    .input
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(10) as usize;
                ToolCallOutput::data(json!(search_tools(&self.registry, query, limit)))
            }
            "tools.describe.tool" => {
                let path = call
                    .input
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                match describe_tool(&self.registry, path) {
                    Some(description) => ToolCallOutput::data(json!(description)),
                    None => ToolCallOutput::failure(format!("Unknown tool: {path}")),
                }
            }
            "tools.invoke" => {
                let Some(path) = call.input.get("path").and_then(Value::as_str) else {
                    return ToolCallOutput::failure("tool path is required");
                };
                if path.is_empty() || path == "tools.invoke" {
                    return ToolCallOutput::failure("tool path is required");
                }
                let input = call
                    .input
                    .get("input")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                self.call(ToolCall {
                    path: path.to_string(),
                    input,
                })
                .await
            }
            _ => self.inner.call(call).await,
        }
    }
}

#[derive(Default)]
pub struct MemoryInvoker {
    outputs: tokio::sync::Mutex<HashMap<String, Value>>,
}

impl MemoryInvoker {
    pub fn new(outputs: HashMap<String, Value>) -> Self {
        Self {
            outputs: tokio::sync::Mutex::new(outputs),
        }
    }
}

#[async_trait]
impl ToolInvoker for MemoryInvoker {
    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        self.outputs
            .lock()
            .await
            .get(&call.path)
            .cloned()
            .map(ToolCallOutput::data)
            .unwrap_or_else(|| ToolCallOutput::failure(format!("Unknown tool: {}", call.path)))
    }
}
