use crate::tools::{
    RegistryValidationError, ToolCall, ToolCallOutput, ToolDescriptor, ToolInvoker, ToolRegistry,
};
#[cfg(feature = "schemars")]
use crate::tools::{ToolCallError, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;

type BoxToolFuture = Pin<Box<dyn Future<Output = ToolCallOutput> + Send>>;

pub trait ToolLayer: Send + Sync {
    fn layer(&self, inner: Arc<dyn ToolInvoker>) -> Arc<dyn ToolInvoker>;
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn descriptor(&self) -> &ToolDescriptor;

    async fn call(&self, call: ToolCall) -> ToolCallOutput;
}

#[cfg(feature = "schemars")]
#[async_trait]
pub trait TypedTool: Send + Sync + 'static {
    type Input: serde::de::DeserializeOwned + schemars::JsonSchema + Send + 'static;
    type Output: serde::Serialize + schemars::JsonSchema + Send + 'static;

    fn descriptor(&self) -> ToolDescriptor;

    async fn call_typed(&self, input: Self::Input) -> ToolResult<Self::Output>;
}

pub trait ToolSet {
    fn tools(&self) -> Result<Vec<Arc<dyn Tool>>, ToolSetError>;
}

impl ToolSet for ToolCatalog {
    fn tools(&self) -> Result<Vec<Arc<dyn Tool>>, ToolSetError> {
        Ok(self.tools.values().cloned().collect())
    }
}

impl<T> ToolSet for Vec<T>
where
    T: Tool + Clone + 'static,
{
    fn tools(&self) -> Result<Vec<Arc<dyn Tool>>, ToolSetError> {
        Ok(self
            .iter()
            .cloned()
            .map(|tool| Arc::new(tool) as Arc<dyn Tool>)
            .collect())
    }
}

impl ToolSet for Vec<Arc<dyn Tool>> {
    fn tools(&self) -> Result<Vec<Arc<dyn Tool>>, ToolSetError> {
        Ok(self.clone())
    }
}

impl<T> ToolSet for &T
where
    T: ToolSet + ?Sized,
{
    fn tools(&self) -> Result<Vec<Arc<dyn Tool>>, ToolSetError> {
        (*self).tools()
    }
}

#[derive(Clone, Default)]
pub struct ToolCatalog {
    tools: BTreeMap<String, Arc<dyn Tool>>,
    layers: Vec<Arc<dyn ToolLayer>>,
}

impl ToolCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<T>(&mut self, tool: T) -> Result<(), CatalogError>
    where
        T: Tool + 'static,
    {
        self.register_arc(Arc::new(tool))
    }

    pub fn register_arc(&mut self, tool: Arc<dyn Tool>) -> Result<(), CatalogError> {
        let path = tool.descriptor().path().to_string();
        validate_added_paths(self.tools.keys().map(String::as_str), [path.as_str()])?;
        self.tools.insert(path, tool);
        Ok(())
    }

    pub fn with_layer<L>(mut self, layer: L) -> Self
    where
        L: ToolLayer + 'static,
    {
        self.layers.push(Arc::new(layer));
        self
    }

    pub fn layer<L>(&mut self, layer: L)
    where
        L: ToolLayer + 'static,
    {
        self.layers.push(Arc::new(layer));
    }

    pub fn invoker(&self) -> Arc<dyn ToolInvoker> {
        let mut invoker: Arc<dyn ToolInvoker> = Arc::new(CatalogDispatch {
            tools: self.tools.clone(),
        });
        for layer in self.layers.iter().rev() {
            invoker = layer.layer(invoker);
        }
        invoker
    }

    pub fn register_fn<F, Fut>(
        &mut self,
        descriptor: ToolDescriptor,
        handler: F,
    ) -> Result<(), CatalogError>
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ToolCallOutput> + Send + 'static,
    {
        self.register(FunctionTool::new(descriptor, handler))
    }

    pub fn register_call_fn<F, Fut>(
        &mut self,
        descriptor: ToolDescriptor,
        handler: F,
    ) -> Result<(), CatalogError>
    where
        F: Fn(ToolCall) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ToolCallOutput> + Send + 'static,
    {
        self.register(FunctionTool::new_call(descriptor, handler))
    }

    #[cfg(feature = "schemars")]
    pub fn register_typed_fn<I, O, F, Fut>(
        &mut self,
        descriptor: ToolDescriptor,
        handler: F,
    ) -> Result<(), CatalogError>
    where
        I: serde::de::DeserializeOwned + schemars::JsonSchema + Send + 'static,
        O: serde::Serialize + schemars::JsonSchema + Send + 'static,
        F: Fn(I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ToolResult<O>> + Send + 'static,
    {
        self.register(TypedFunctionTool::new(descriptor, handler))
    }

    #[cfg(feature = "schemars")]
    pub fn register_typed<T>(&mut self, tool: T) -> Result<(), CatalogError>
    where
        T: TypedTool,
    {
        self.register(TypedToolAdapter::new(tool))
    }

    pub fn mount<S>(&mut self, namespace: impl Into<String>, set: S) -> Result<(), CatalogError>
    where
        S: ToolSet,
    {
        let namespace = namespace.into();
        let mounted = set
            .tools()?
            .into_iter()
            .map(|tool| MountedTool::new(namespace.clone(), tool))
            .collect::<Result<Vec<_>, _>>()?;
        validate_added_paths(
            self.tools.keys().map(String::as_str),
            mounted.iter().map(|tool| tool.descriptor().path()),
        )?;
        for tool in mounted {
            self.tools
                .insert(tool.descriptor().path().to_string(), Arc::new(tool));
        }
        Ok(())
    }

    pub fn registry(&self) -> ToolRegistry {
        ToolRegistry::from_tools(
            self.tools
                .values()
                .map(|tool| tool.descriptor().clone())
                .collect(),
        )
        .expect("catalog validates tool descriptors at registration")
    }

    pub fn iter(&self) -> impl Iterator<Item = &ToolDescriptor> {
        self.tools.values().map(|tool| tool.descriptor())
    }

    pub fn by_path(&self, path: &str) -> Option<&dyn Tool> {
        self.tools.get(path).map(Arc::as_ref)
    }

    pub fn contains_path(&self, path: &str) -> bool {
        self.tools.contains_key(path)
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

#[async_trait]
impl ToolInvoker for ToolCatalog {
    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        self.invoker().call(call).await
    }
}

struct CatalogDispatch {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

#[async_trait]
impl ToolInvoker for CatalogDispatch {
    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        match self.tools.get(&call.path) {
            Some(tool) => tool.call(call).await,
            None => unknown_tool(&call.path),
        }
    }
}

pub struct FunctionTool {
    descriptor: ToolDescriptor,
    handler: Arc<dyn Fn(ToolCall) -> BoxToolFuture + Send + Sync>,
}

impl FunctionTool {
    pub fn new<F, Fut>(descriptor: ToolDescriptor, handler: F) -> Self
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ToolCallOutput> + Send + 'static,
    {
        Self::new_call(descriptor, move |call| handler(call.input))
    }

    pub fn new_call<F, Fut>(descriptor: ToolDescriptor, handler: F) -> Self
    where
        F: Fn(ToolCall) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ToolCallOutput> + Send + 'static,
    {
        Self {
            descriptor,
            handler: Arc::new(move |call| Box::pin(handler(call))),
        }
    }
}

#[async_trait]
impl Tool for FunctionTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        (self.handler)(call).await
    }
}

#[cfg(feature = "schemars")]
pub struct TypedFunctionTool {
    inner: FunctionTool,
}

#[cfg(feature = "schemars")]
impl TypedFunctionTool {
    pub fn new<I, O, F, Fut>(descriptor: ToolDescriptor, handler: F) -> Self
    where
        I: serde::de::DeserializeOwned + schemars::JsonSchema + Send + 'static,
        O: serde::Serialize + schemars::JsonSchema + Send + 'static,
        F: Fn(I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ToolResult<O>> + Send + 'static,
    {
        let descriptor = descriptor
            .with_input_schema(schema_value::<I>())
            .with_output_schema(schema_value::<O>());
        Self {
            inner: FunctionTool::new_call(descriptor, move |call| {
                let future = deserialize_typed_input(call.input).map(&handler);
                async move {
                    let future = match future {
                        Ok(future) => future,
                        Err(error) => return ToolCallOutput::from_error(error),
                    };
                    typed_result_to_output(future.await)
                }
            }),
        }
    }
}

#[cfg(feature = "schemars")]
#[async_trait]
impl Tool for TypedFunctionTool {
    fn descriptor(&self) -> &ToolDescriptor {
        self.inner.descriptor()
    }

    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        self.inner.call(call).await
    }
}

#[cfg(feature = "schemars")]
pub struct TypedToolAdapter<T> {
    descriptor: ToolDescriptor,
    inner: T,
}

#[cfg(feature = "schemars")]
impl<T> TypedToolAdapter<T>
where
    T: TypedTool,
{
    pub fn new(inner: T) -> Self {
        let descriptor = inner
            .descriptor()
            .with_input_schema(schema_value::<T::Input>())
            .with_output_schema(schema_value::<T::Output>());
        Self { descriptor, inner }
    }
}

#[cfg(feature = "schemars")]
#[async_trait]
impl<T> Tool for TypedToolAdapter<T>
where
    T: TypedTool,
{
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        let input = match deserialize_typed_input(call.input) {
            Ok(input) => input,
            Err(error) => return ToolCallOutput::from_error(error),
        };
        typed_result_to_output(self.inner.call_typed(input).await)
    }
}

struct MountedTool {
    descriptor: ToolDescriptor,
    inner: Arc<dyn Tool>,
}

impl MountedTool {
    fn new(namespace: String, inner: Arc<dyn Tool>) -> Result<Self, CatalogError> {
        let descriptor = inner.descriptor();
        let mounted_path = join_tool_path(&namespace, descriptor.path())?;
        Ok(Self {
            descriptor: descriptor.clone().with_path(mounted_path),
            inner,
        })
    }
}

#[async_trait]
impl Tool for MountedTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn call(&self, call: ToolCall) -> ToolCallOutput {
        self.inner.call(call).await
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum CatalogError {
    #[error(transparent)]
    Registry(#[from] RegistryValidationError),
    #[error(transparent)]
    ToolSet(#[from] ToolSetError),
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ToolSetError {
    #[error("{0}")]
    Message(String),
}

impl ToolSetError {
    pub fn new(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

fn validate_added_paths<'a, E, A>(existing: E, added: A) -> Result<(), CatalogError>
where
    E: IntoIterator<Item = &'a str>,
    A: IntoIterator<Item = &'a str>,
{
    let existing = existing
        .into_iter()
        .map(|path| ToolDescriptor::new(path, path))
        .collect::<Vec<_>>();
    let added = added
        .into_iter()
        .map(|path| ToolDescriptor::new(path, path))
        .collect::<Vec<_>>();
    let mut registry = ToolRegistry::from_tools(existing)?;
    registry.extend(added)?;
    Ok(())
}

fn join_tool_path(namespace: &str, path: &str) -> Result<String, CatalogError> {
    let path = path.trim_start_matches('.');
    let full_path = if namespace.is_empty() {
        path.to_string()
    } else if path.is_empty() {
        namespace.to_string()
    } else {
        format!("{namespace}.{path}")
    };
    ToolRegistry::from_tools(vec![ToolDescriptor::new(&full_path, &full_path)])?;
    Ok(full_path)
}

fn unknown_tool(path: &str) -> ToolCallOutput {
    let message = format!("Unknown tool: {path}");
    let mut output =
        ToolCallOutput::failure_with_code(message.clone(), Some("unknown_tool".into()));
    output.text = Some(message);
    output
}

#[cfg(feature = "schemars")]
fn schema_value<T>() -> Value
where
    T: schemars::JsonSchema,
{
    serde_json::to_value(schemars::schema_for!(T)).expect("schemars schemas serialize to JSON")
}

#[cfg(feature = "schemars")]
fn deserialize_typed_input<I>(input: Value) -> Result<I, ToolCallError>
where
    I: serde::de::DeserializeOwned,
{
    serde_json::from_value(input)
        .map_err(|error| ToolCallError::with_code(error.to_string(), "invalid_input"))
}

#[cfg(feature = "schemars")]
fn typed_result_to_output<O>(result: ToolResult<O>) -> ToolCallOutput
where
    O: serde::Serialize,
{
    match result {
        Ok(output) => match serde_json::to_value(output) {
            Ok(output) => ToolCallOutput::data(output),
            Err(error) => ToolCallOutput::from_error(ToolCallError::with_code(
                error.to_string(),
                "invalid_output",
            )),
        },
        Err(error) => ToolCallOutput::from_error(error),
    }
}
