use super::*;

#[test]
fn transforms_typescript() {
    let code = transform_typescript("const value: number = 2; return value;").unwrap();
    assert!(code.contains("const value = 2"));
    assert!(code.contains("function"));
}
