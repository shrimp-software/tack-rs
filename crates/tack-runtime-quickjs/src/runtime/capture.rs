use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tack_codemode::{FailurePhase, RuntimeFailure, RuntimeOutput};

use serde_json::Value;

/// Tracks captured logs and emitted values as an early output guard.
///
/// The exact serialized `RuntimeOutput` is checked again before returning.
#[derive(Clone)]
pub(crate) struct CapturedOutput {
    emitted: Arc<Mutex<Vec<Value>>>,
    logs: Arc<Mutex<Vec<String>>>,
    bytes: Arc<Mutex<usize>>,
    exceeded: Arc<AtomicBool>,
    max_bytes: usize,
}

impl CapturedOutput {
    pub(crate) fn new(max_bytes: usize) -> Self {
        Self {
            emitted: Arc::new(Mutex::new(Vec::new())),
            logs: Arc::new(Mutex::new(Vec::new())),
            bytes: Arc::new(Mutex::new(0)),
            exceeded: Arc::new(AtomicBool::new(false)),
            max_bytes,
        }
    }

    pub(crate) fn emit_json(&self, json: &str) {
        if !self.reserve(json.len()) {
            return;
        }
        if let Ok(value) = serde_json::from_str::<Value>(json) {
            self.emitted.lock().expect("emitted lock").push(value);
        }
    }

    pub(crate) fn log_line(&self, line: String) {
        if self.reserve(line.len()) {
            self.logs.lock().expect("logs lock").push(line);
        }
    }

    pub(crate) fn into_output(self, result: Value) -> Result<RuntimeOutput, RuntimeFailure> {
        if self.exceeded.load(Ordering::Relaxed) {
            return Err(self.failure(
                FailurePhase::Runtime,
                format!("execution output exceeded {} bytes", self.max_bytes),
            ));
        }
        Ok(RuntimeOutput {
            result,
            emitted: self.emitted.lock().expect("emitted lock").clone(),
            logs: self.logs.lock().expect("logs lock").clone(),
        })
    }

    fn snapshot(&self) -> (Vec<Value>, Vec<String>) {
        (
            self.emitted.lock().expect("emitted lock").clone(),
            self.logs.lock().expect("logs lock").clone(),
        )
    }

    pub(crate) fn failure(
        &self,
        phase: FailurePhase,
        message: impl Into<String>,
    ) -> RuntimeFailure {
        let (emitted, logs) = self.snapshot();
        RuntimeFailure::new(phase, message).with_output(emitted, logs)
    }

    pub(crate) fn caught_failure(
        &self,
        error: rquickjs::CaughtError<'_>,
        timed_out: &AtomicBool,
    ) -> RuntimeFailure {
        if timed_out.load(Ordering::Relaxed) {
            self.failure(FailurePhase::Timeout, "execution timed out")
        } else {
            self.failure(FailurePhase::Runtime, error.to_string())
        }
    }

    fn reserve(&self, next_bytes: usize) -> bool {
        let mut used = self.bytes.lock().expect("capture bytes lock");
        let Some(next_total) = used.checked_add(next_bytes) else {
            self.exceeded.store(true, Ordering::Relaxed);
            return false;
        };
        if next_total > self.max_bytes {
            self.exceeded.store(true, Ordering::Relaxed);
            return false;
        }
        *used = next_total;
        true
    }
}
