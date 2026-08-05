//! Host-independent contracts for tool-backed TypeScript execution.
//!
//! This crate defines the stable data model shared by hosts and runtimes: tool
//! descriptors, invocation envelopes, runtime limits, execution results,
//! built-in discovery helpers, and policy hooks. Hosts provide a
//! [`ToolInvoker`], runtimes implement [`CodeRuntime`], and
//! [`ExecutionEngine`] ties them together.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolDescriptor {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub input_schema: Value,
    #[serde(default)]
    pub output_schema: Value,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl ToolDescriptor {
    pub fn new(path: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            tool_id: None,
            name: name.into(),
            title: None,
            description: None,
            input_schema: json!({"type":"object","properties":{},"additionalProperties":true}),
            output_schema: Value::Null,
            tags: Vec::new(),
        }
    }
}

const BUILTIN_TOOL_PATHS: &[&str] = &["tools.search", "tools.describe.tool", "tools.invoke"];

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolRegistry {
    pub tools: Vec<ToolDescriptor>,
}

impl ToolRegistry {
    pub fn validate(&self) -> Result<(), RegistryValidationError> {
        let mut paths = self
            .tools
            .iter()
            .map(|tool| tool.path.as_str())
            .collect::<Vec<_>>();
        paths.sort_unstable();
        if let Some(duplicate) = paths.windows(2).find(|paths| paths[0] == paths[1]) {
            return Err(RegistryValidationError::DuplicateToolPath(
                duplicate[0].to_string(),
            ));
        }
        for tool in &self.tools {
            validate_tool_path(&tool.path)?;
        }
        Ok(())
    }

    pub fn by_path(&self, path: &str) -> Option<&ToolDescriptor> {
        self.tools.iter().find(|tool| tool.path == path)
    }

    pub fn contains_path(&self, path: &str) -> bool {
        self.by_path(path).is_some()
    }

    pub fn filtered_by_policy(&self, policy: &dyn ToolPolicy) -> Self {
        Self {
            tools: self
                .tools
                .iter()
                .filter(|tool| policy.is_allowed(&tool.path))
                .cloned()
                .collect(),
        }
    }

    pub fn from_manifest_value(value: &Value) -> Result<Self, ManifestImportError> {
        let manifest: TackManifestImport = serde_json::from_value(value.clone())?;
        let mut tools = Vec::with_capacity(manifest.tools.len());
        for (fallback_id, tool) in manifest.tools {
            let id = tool.id.unwrap_or(fallback_id);
            let namespace = tool.namespace_name.unwrap_or_else(|| {
                id.split('.')
                    .next()
                    .filter(|part| !part.is_empty())
                    .unwrap_or("tool")
                    .to_string()
            });
            let name = tool.sdk_name.unwrap_or_else(|| {
                id.rsplit('.')
                    .next()
                    .filter(|part| !part.is_empty())
                    .unwrap_or(id.as_str())
                    .to_string()
            });
            tools.push(ToolDescriptor {
                path: format!("{namespace}.{name}"),
                tool_id: Some(id),
                name,
                title: None,
                description: tool.description,
                input_schema: tool
                    .input_schema
                    .unwrap_or_else(|| json!({"type":"object","additionalProperties":true})),
                output_schema: tool.output_schema.unwrap_or(Value::Null),
                tags: vec![namespace],
            });
        }
        tools.sort_by(|left, right| left.path.cmp(&right.path));
        let registry = Self { tools };
        registry.validate()?;
        Ok(registry)
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum RegistryValidationError {
    #[error("tool path must not be empty")]
    EmptyToolPath,
    #[error("tool path `{path}` has invalid segment `{segment}`")]
    InvalidToolPath { path: String, segment: String },
    #[error("tool path `{0}` is reserved")]
    ReservedToolPath(String),
    #[error("tool registry contains duplicate path `{0}`")]
    DuplicateToolPath(String),
}

fn validate_tool_path(path: &str) -> Result<(), RegistryValidationError> {
    if path.is_empty() {
        return Err(RegistryValidationError::EmptyToolPath);
    }
    if path.starts_with("__tack")
        || BUILTIN_TOOL_PATHS.contains(&path)
        || path.starts_with("tools.")
    {
        return Err(RegistryValidationError::ReservedToolPath(path.to_string()));
    }
    for segment in path.split('.') {
        if !is_js_identifier(segment) {
            return Err(RegistryValidationError::InvalidToolPath {
                path: path.to_string(),
                segment: segment.to_string(),
            });
        }
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ManifestImportError {
    #[error("invalid Tack manifest: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid Tack registry: {0}")]
    Registry(#[from] RegistryValidationError),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TackManifestImport {
    #[serde(default)]
    tools: HashMap<String, TackToolImport>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TackToolImport {
    id: Option<String>,
    namespace_name: Option<String>,
    sdk_name: Option<String>,
    description: Option<String>,
    input_schema: Option<Value>,
    output_schema: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub path: String,
    #[serde(default)]
    pub input: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallOutput {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ToolCallError>,
}

impl ToolCallOutput {
    pub fn data(data: Value) -> Self {
        Self {
            ok: true,
            data: Some(data),
            text: None,
            raw: None,
            error: None,
        }
    }

    pub fn failure(message: impl Into<String>) -> Self {
        Self::failure_with_code(message, None)
    }

    pub fn failure_with_code(message: impl Into<String>, code: Option<String>) -> Self {
        Self {
            ok: false,
            data: None,
            text: None,
            raw: None,
            error: Some(ToolCallError {
                message: message.into(),
                code,
                details: None,
            }),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallError {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[async_trait]
pub trait ToolInvoker: Send + Sync {
    async fn call(&self, call: ToolCall) -> ToolCallOutput;
}

pub trait ToolPolicy: Send + Sync {
    fn is_allowed(&self, path: &str) -> bool;
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLimits {
    pub timeout_ms: u64,
    pub memory_bytes: usize,
    pub stack_bytes: usize,
    pub max_output_bytes: usize,
    pub max_tool_calls: usize,
    pub tool_call_timeout_ms: u64,
    pub max_tool_request_bytes: usize,
    pub max_tool_response_bytes: usize,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            timeout_ms: 30_000,
            memory_bytes: 128 * 1024 * 1024,
            stack_bytes: 1_000_000,
            max_output_bytes: 1_000_000,
            max_tool_calls: 100,
            tool_call_timeout_ms: 30_000,
            max_tool_request_bytes: 1_000_000,
            max_tool_response_bytes: 1_000_000,
        }
    }
}

impl RuntimeLimits {
    pub fn validate(&self) -> Result<(), RuntimeLimitsError> {
        ensure_limit_nonzero("timeoutMs", self.timeout_ms)?;
        ensure_limit_nonzero("memoryBytes", self.memory_bytes)?;
        ensure_limit_nonzero("stackBytes", self.stack_bytes)?;
        ensure_limit_nonzero("maxOutputBytes", self.max_output_bytes)?;
        ensure_limit_nonzero("maxToolCalls", self.max_tool_calls)?;
        ensure_limit_nonzero("toolCallTimeoutMs", self.tool_call_timeout_ms)?;
        ensure_limit_nonzero("maxToolRequestBytes", self.max_tool_request_bytes)?;
        ensure_limit_nonzero("maxToolResponseBytes", self.max_tool_response_bytes)?;
        Ok(())
    }

    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }

    pub fn tool_call_timeout(&self) -> Duration {
        Duration::from_millis(self.tool_call_timeout_ms)
    }
}

/// Error returned when runtime limits would disable a required guard.
///
/// All limits must be greater than zero. A zero memory or stack limit is
/// rejected instead of relying on backend-specific "unlimited" semantics.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum RuntimeLimitsError {
    #[error("runtime limit `{field}` must be greater than zero")]
    NonZero { field: &'static str },
}

fn ensure_limit_nonzero<T>(field: &'static str, value: T) -> Result<(), RuntimeLimitsError>
where
    T: PartialEq + Default,
{
    if value == T::default() {
        Err(RuntimeLimitsError::NonZero { field })
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInput {
    pub source: String,
    pub registry: ToolRegistry,
    pub aliases: HashMap<String, String>,
    pub limits: RuntimeLimits,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeOutput {
    #[serde(default)]
    pub result: Value,
    #[serde(default)]
    pub emitted: Vec<Value>,
    #[serde(default)]
    pub logs: Vec<String>,
}

#[async_trait(?Send)]
pub trait CodeRuntime: Send + Sync {
    async fn execute(
        &self,
        input: RuntimeInput,
        invoker: Arc<dyn ToolInvoker>,
    ) -> Result<RuntimeOutput, RuntimeFailure>;
}

#[derive(Clone, Debug, Error, Serialize, Deserialize, PartialEq, Eq)]
#[error("{phase}: {message}")]
#[serde(rename_all = "camelCase")]
pub struct RuntimeFailure {
    pub phase: FailurePhase,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emitted: Vec<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub logs: Vec<String>,
}

impl RuntimeFailure {
    pub fn new(phase: FailurePhase, message: impl Into<String>) -> Self {
        Self {
            phase,
            message: message.into(),
            emitted: Vec::new(),
            logs: Vec::new(),
        }
    }

    pub fn with_output(mut self, emitted: Vec<Value>, logs: Vec<String>) -> Self {
        self.emitted = emitted;
        self.logs = logs;
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FailurePhase {
    Parse,
    Runtime,
    Timeout,
}

impl fmt::Display for FailurePhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse => f.write_str("parse"),
            Self::Runtime => f.write_str("runtime"),
            Self::Timeout => f.write_str("timeout"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default)]
    pub emitted: Vec<Value>,
    #[serde(default)]
    pub logs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RuntimeFailure>,
}

#[derive(Clone)]
pub struct ExecutionEngine<R> {
    registry: ToolRegistry,
    runtime: R,
    invoker: Arc<dyn ToolInvoker>,
    policy: Option<Arc<dyn ToolPolicy>>,
    limits: RuntimeLimits,
    aliases: HashMap<String, String>,
}

impl<R> ExecutionEngine<R>
where
    R: CodeRuntime,
{
    pub fn new(registry: ToolRegistry, runtime: R, invoker: Arc<dyn ToolInvoker>) -> Self {
        Self {
            registry,
            runtime,
            invoker,
            policy: None,
            limits: RuntimeLimits::default(),
            aliases: HashMap::new(),
        }
    }

    pub fn with_limits(mut self, limits: RuntimeLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn with_alias(mut self, namespace: impl Into<String>, prefix: impl Into<String>) -> Self {
        self.aliases.insert(namespace.into(), prefix.into());
        self
    }

    pub fn with_policy(mut self, policy: Arc<dyn ToolPolicy>) -> Self {
        self.policy = Some(policy);
        self
    }

    pub async fn execute(&self, source: impl Into<String>) -> ExecutionResult {
        let registry = self.execution_registry();
        if let Err(error) = validate_aliases(&self.aliases) {
            return ExecutionResult {
                ok: false,
                result: None,
                emitted: Vec::new(),
                logs: Vec::new(),
                error: Some(error),
            };
        }
        if let Err(error) = self.limits.validate() {
            return ExecutionResult {
                ok: false,
                result: None,
                emitted: Vec::new(),
                logs: Vec::new(),
                error: Some(RuntimeFailure::new(
                    FailurePhase::Runtime,
                    error.to_string(),
                )),
            };
        }
        if let Err(error) = registry.validate() {
            return ExecutionResult {
                ok: false,
                result: None,
                emitted: Vec::new(),
                logs: Vec::new(),
                error: Some(RuntimeFailure::new(
                    FailurePhase::Runtime,
                    error.to_string(),
                )),
            };
        }

        let input = RuntimeInput {
            source: source.into(),
            registry,
            aliases: self.aliases.clone(),
            limits: self.limits.clone(),
        };

        match self.runtime.execute(input, self.host_invoker()).await {
            Ok(output) => ExecutionResult {
                ok: true,
                result: Some(output.result),
                emitted: output.emitted,
                logs: output.logs,
                error: None,
            },
            Err(error) => ExecutionResult {
                ok: false,
                result: None,
                emitted: error.emitted.clone(),
                logs: error.logs.clone(),
                error: Some(error),
            },
        }
    }

    fn execution_registry(&self) -> ToolRegistry {
        self.policy
            .as_ref()
            .map(|policy| self.registry.filtered_by_policy(policy.as_ref()))
            .unwrap_or_else(|| self.registry.clone())
    }

    fn host_invoker(&self) -> Arc<dyn ToolInvoker> {
        match &self.policy {
            Some(policy) => Arc::new(PolicyAwareInvoker::new(
                policy.clone(),
                self.invoker.clone(),
            )),
            None => self.invoker.clone(),
        }
    }
}

pub fn validate_aliases(aliases: &HashMap<String, String>) -> Result<(), RuntimeFailure> {
    for (namespace, prefix) in aliases {
        if is_reserved_alias_namespace(namespace) || !is_js_identifier(namespace) {
            return Err(RuntimeFailure::new(
                FailurePhase::Runtime,
                format!("invalid alias namespace: {namespace}"),
            ));
        }
        if !prefix.split('.').all(is_js_identifier) {
            return Err(RuntimeFailure::new(
                FailurePhase::Runtime,
                format!("invalid alias prefix: {prefix}"),
            ));
        }
    }
    Ok(())
}

fn is_reserved_alias_namespace(namespace: &str) -> bool {
    matches!(
        namespace,
        "tools"
            | "emit"
            | "console"
            | "globalThis"
            | "global"
            | "window"
            | "__tackCallJson"
            | "__tackEmitJson"
    ) || namespace.starts_with("__tack")
}

fn is_js_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
}

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

fn schema_to_type(schema: &Value) -> String {
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

#[derive(Clone, Debug)]
pub struct StaticRuntime {
    output: Result<RuntimeOutput, RuntimeFailure>,
}

impl StaticRuntime {
    pub fn ok(result: Value) -> Self {
        Self {
            output: Ok(RuntimeOutput {
                result,
                emitted: Vec::new(),
                logs: Vec::new(),
            }),
        }
    }
}

#[async_trait(?Send)]
impl CodeRuntime for StaticRuntime {
    async fn execute(
        &self,
        _input: RuntimeInput,
        _invoker: Arc<dyn ToolInvoker>,
    ) -> Result<RuntimeOutput, RuntimeFailure> {
        self.output.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn engine_wraps_runtime_result() {
        let engine = ExecutionEngine::new(
            ToolRegistry::default(),
            StaticRuntime::ok(json!(5)),
            Arc::new(MemoryInvoker::default()),
        );
        let result = engine.execute("return 5").await;
        assert!(result.ok);
        assert_eq!(result.result, Some(json!(5)));
    }

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
        let invoker =
            BuiltinAwareInvoker::new(registry.clone(), Arc::new(MemoryInvoker::default()));
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
    async fn policy_denies_exact_and_wildcard_paths() {
        let policy = Arc::new(DenyListPolicy::new(vec![
            "example.secret".to_string(),
            "example.admin.*".to_string(),
        ]));
        let invoker = PolicyAwareInvoker::new(policy, Arc::new(MemoryInvoker::default()));
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
            ToolRegistry {
                tools: vec![ToolDescriptor::new("example.allowed", "allowed")],
            },
            Arc::new(MemoryInvoker::new(HashMap::from([(
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

    #[tokio::test]
    async fn invalid_aliases_fail_before_runtime_execution() {
        let engine = ExecutionEngine::new(
            ToolRegistry::default(),
            StaticRuntime::ok(json!(5)),
            Arc::new(MemoryInvoker::default()),
        )
        .with_alias("tools", "eoka");
        let result = engine.execute("return 5").await;
        assert!(!result.ok);
        assert!(result.error.unwrap().message.contains("invalid alias"));
    }

    #[tokio::test]
    async fn execution_engine_rejects_reserved_tool_paths_before_runtime_execution() {
        let engine = ExecutionEngine::new(
            ToolRegistry {
                tools: vec![ToolDescriptor::new("tools.search", "search")],
            },
            StaticRuntime::ok(json!(5)),
            Arc::new(MemoryInvoker::default()),
        );
        let result = engine.execute("return 5").await;
        assert!(!result.ok);
        assert_eq!(
            result.error.unwrap().message,
            "tool path `tools.search` is reserved"
        );
    }

    #[test]
    fn registry_validation_rejects_invalid_tool_paths() {
        let registry = ToolRegistry {
            tools: vec![ToolDescriptor::new("eoka.fetch-url", "fetch-url")],
        };
        assert!(matches!(
            registry.validate(),
            Err(RegistryValidationError::InvalidToolPath { .. })
        ));
    }

    #[test]
    fn schema_type_handles_objects() {
        let ty = schema_to_type(&json!({
            "type":"object",
            "required":["url", "content-type"],
            "properties":{"url":{"type":"string"},"timeout":{"type":"integer"},"content-type":{"type":"string"}}
        }));
        assert_eq!(
            ty,
            "{ \"content-type\": string; timeout?: number; url: string }"
        );
    }

    #[test]
    fn schema_type_handles_unions_enums_and_maps() {
        let nullable = schema_to_type(&json!({"type":["string","null"]}));
        assert_eq!(nullable, "string | null");

        let choices = schema_to_type(&json!({"enum":["low","high",null]}));
        assert_eq!(choices, "\"low\" | \"high\" | null");

        let union = schema_to_type(&json!({
            "oneOf":[{"type":"integer"},{"const":"auto"}]
        }));
        assert_eq!(union, "number | \"auto\"");

        let map = schema_to_type(&json!({
            "type":"object",
            "additionalProperties":{"type":"boolean"}
        }));
        assert_eq!(map, "Record<string, boolean>");
    }

    #[test]
    fn tool_call_output_omits_absent_fields() {
        let value = serde_json::to_value(ToolCallOutput::data(json!({"ok":true}))).unwrap();
        let object = value.as_object().unwrap();
        assert_eq!(object.get("ok"), Some(&Value::Bool(true)));
        assert!(!object.contains_key("error"));
    }

    #[test]
    fn runtime_limits_deserialize_missing_new_fields_from_defaults() {
        let limits: RuntimeLimits = serde_json::from_value(json!({
            "timeoutMs": 1,
            "memoryBytes": 2,
            "stackBytes": 3,
            "maxOutputBytes": 4,
            "maxToolCalls": 5,
            "maxToolRequestBytes": 6,
            "maxToolResponseBytes": 7
        }))
        .unwrap();
        assert_eq!(limits.timeout_ms, 1);
        assert_eq!(
            limits.tool_call_timeout_ms,
            RuntimeLimits::default().tool_call_timeout_ms
        );
    }

    #[test]
    fn runtime_limits_reject_zero_values() {
        let limits = RuntimeLimits {
            memory_bytes: 0,
            ..RuntimeLimits::default()
        };
        assert_eq!(
            limits.validate(),
            Err(RuntimeLimitsError::NonZero {
                field: "memoryBytes"
            })
        );
    }

    #[tokio::test]
    async fn execution_engine_rejects_invalid_runtime_limits() {
        let engine = ExecutionEngine::new(
            ToolRegistry::default(),
            StaticRuntime::ok(json!(5)),
            Arc::new(MemoryInvoker::default()),
        )
        .with_limits(RuntimeLimits {
            timeout_ms: 0,
            ..RuntimeLimits::default()
        });
        let result = engine.execute("return 5").await;
        assert!(!result.ok);
        assert_eq!(result.error.unwrap().phase, FailurePhase::Runtime);
    }

    #[test]
    fn registry_imports_tack_manifest_tools() {
        let registry = ToolRegistry::from_manifest_value(&json!({
            "version":"0.1",
            "generatedAt":"2026-01-01T00:00:00.000Z",
            "servers":{},
            "tools":{
                "grafana.list_datasources":{
                    "id":"grafana.list_datasources",
                    "serverId":"grafana",
                    "namespaceName":"grafana",
                    "sdkName":"list_datasources",
                    "upstreamName":"list_datasources",
                    "description":"List data sources",
                    "inputSchema":{"type":"object","additionalProperties":false}
                }
            }
        }))
        .unwrap();
        assert_eq!(registry.tools[0].path, "grafana.list_datasources");
        assert_eq!(
            registry.tools[0].tool_id.as_deref(),
            Some("grafana.list_datasources")
        );
    }

    #[test]
    fn registry_import_rejects_duplicate_sdk_paths() {
        let error = ToolRegistry::from_manifest_value(&json!({
            "tools": {
                "left": {"namespaceName":"eoka","sdkName":"eval"},
                "right": {"namespaceName":"eoka","sdkName":"eval"}
            }
        }))
        .unwrap_err();
        assert!(matches!(
            error,
            ManifestImportError::Registry(RegistryValidationError::DuplicateToolPath(path))
                if path == "eoka.eval"
        ));
    }

    #[test]
    fn registry_import_rejects_reserved_sdk_paths() {
        let error = ToolRegistry::from_manifest_value(&json!({
            "tools": {
                "left": {"namespaceName":"tools","sdkName":"search"}
            }
        }))
        .unwrap_err();
        assert!(matches!(
            error,
            ManifestImportError::Registry(RegistryValidationError::ReservedToolPath(path))
                if path == "tools.search"
        ));
    }
}
