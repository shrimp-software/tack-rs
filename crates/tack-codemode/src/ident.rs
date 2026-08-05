use crate::{FailurePhase, RuntimeFailure};
use std::collections::HashMap;

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

pub(crate) fn is_js_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
}
