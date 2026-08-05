use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use thiserror::Error;

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
