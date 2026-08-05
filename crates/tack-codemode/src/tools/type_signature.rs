use crate::ident::is_js_identifier;
use serde_json::{Map, Value};
use std::collections::HashSet;

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
    let empty = Map::new();
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let additional = schema.get("additionalProperties");
    if properties.is_empty() {
        return match additional {
            Some(schema @ Value::Object(_)) => {
                format!("Record<string, {}>", schema_to_type(schema))
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
