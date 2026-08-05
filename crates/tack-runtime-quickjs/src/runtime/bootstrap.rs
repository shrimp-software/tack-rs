use std::collections::HashMap;

const BOOTSTRAP_JS: &str = include_str!("../bootstrap.js");

pub(crate) fn bootstrap_script(
    aliases: &HashMap<String, String>,
    max_tool_request_bytes: usize,
) -> String {
    let mut script =
        format!("(() => {{\nconst __tackMaxToolRequestBytes = {max_tool_request_bytes};\n");
    script.push_str(BOOTSTRAP_JS);
    for (namespace, prefix) in aliases {
        script.push_str(&format!(
            "__tackDefine(globalThis, {}, {{ value: __tackNamespace({}, true), writable: false, configurable: false }});\n",
            serde_json::to_string(namespace).expect("namespace json"),
            serde_json::to_string(prefix).expect("prefix json")
        ));
    }
    script.push_str(
        r#"
for (const name of ['__tackCallJson', '__tackEmitJson', '__tackLogLine']) {
  __tackDefine(globalThis, name, { value: undefined, writable: false, configurable: false });
}
})();
"#,
    );
    script
}

pub(crate) fn wrap_user_code(transformed: &str) -> String {
    format!(
        "(async function(){{const __tackResultStringify = JSON.stringify; const __tackValue = await {transformed}\n; return __tackResultStringify({{value:__tackValue}});}})()"
    )
}
