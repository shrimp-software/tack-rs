use super::*;
use crate::tools::type_signature::schema_to_type;

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
