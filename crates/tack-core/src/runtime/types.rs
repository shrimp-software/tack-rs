use super::{RuntimeFailure, RuntimeLimits};
use crate::tools::{ToolInvoker, ToolRegistry};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

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
