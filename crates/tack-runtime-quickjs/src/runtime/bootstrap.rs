use std::collections::HashMap;

use super::HOST_BINDINGS;

const BOOTSTRAP_JS: &str = include_str!("../bootstrap.js");
const BOOTSTRAP_OPEN: &str = "(() => {\n";
const TOOL_REQUEST_LIMIT_DECL: &str = "const __tackMaxToolRequestBytes = ";
const BOOTSTRAP_CLOSE: &str = "})();\n";
const USER_CODE_WRAPPER_OPEN: &str =
    "(async function(){const __tackResultStringify = JSON.stringify; const __tackValue = await ";
const USER_CODE_WRAPPER_CLOSE: &str = "\n; return __tackResultStringify({value:__tackValue});})()";

pub(crate) fn bootstrap_script(
    aliases: &HashMap<String, String>,
    max_tool_request_bytes: usize,
) -> String {
    let mut script = String::new();
    script.push_str(BOOTSTRAP_OPEN);
    script.push_str(TOOL_REQUEST_LIMIT_DECL);
    script.push_str(&max_tool_request_bytes.to_string());
    script.push_str(";\n");
    script.push_str(BOOTSTRAP_JS);
    for (namespace, prefix) in aliases {
        script.push_str(&alias_definition(namespace, prefix));
    }
    script.push_str(&hide_host_bindings());
    script.push_str(BOOTSTRAP_CLOSE);
    script
}

pub(crate) fn wrap_user_code(transformed: &str) -> String {
    let mut script = String::new();
    script.push_str(USER_CODE_WRAPPER_OPEN);
    script.push_str(transformed);
    script.push_str(USER_CODE_WRAPPER_CLOSE);
    script
}

fn alias_definition(namespace: &str, prefix: &str) -> String {
    format!(
        "__tackDefine(globalThis, {}, {{ value: __tackNamespace({}, true), writable: false, configurable: false }});\n",
        serde_json::to_string(namespace).expect("namespace json"),
        serde_json::to_string(prefix).expect("prefix json")
    )
}

fn hide_host_bindings() -> String {
    HOST_BINDINGS
        .iter()
        .map(|name| {
            format!(
                "__tackDefine(globalThis, {}, {{ value: undefined, writable: false, configurable: false }});\n",
                serde_json::to_string(name).expect("host binding json")
            )
        })
        .collect()
}
