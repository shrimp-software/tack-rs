# TypeScript API

Each snippet is evaluated as a TypeScript function body in a fresh QuickJS
context. The runtime erases TypeScript types before evaluation, replacing them
with whitespace so line and column numbers stay intact. Syntax that would need
generated code (enums, namespaces with values, parameter properties,
decorators, `import x = require(...)`) and `import`/`export` of values fail
in the `parse` phase.

## Globals

Snippets receive these globals:

- `tools.search({ query, limit })`
- `tools.describe.tool({ path })`
- lazy tool calls such as `tools.inventory.lookup(input)`
- explicit tool calls such as `tools.invoke("inventory.lookup", input)`
- configured aliases such as `inventory.lookup(input)`
- `emit(value)`
- `console.log(...values)`

The QuickJS runtime does not provide `fetch`, `process`, `require`,
filesystem access, or a module loader.

## Tool Results

Calls through `tools.*` return a `ToolCallOutput` envelope:

```ts
const result = await tools.inventory.lookup({ sku: "abc-123" });

if (result.ok) {
  emit({ quantity: result.data.quantity });
} else {
  console.log(result.error.message);
}
```

The envelope has this shape:

```ts
type ToolCallOutput<T = unknown> = {
  ok: boolean;
  data?: T;
  text?: string;
  raw?: unknown;
  error?: {
    message: string;
    code?: string;
    details?: unknown;
  };
};
```

Use `tools.invoke(path, input)` when the path is dynamic or when a path segment
would collide with JavaScript proxy behavior, such as `then`.

## Discovery

Use discovery when the snippet should choose from the available tool set at
runtime:

```ts
const hits = await tools.search({ query: "inventory", limit: 5 });
const lookup = await tools.describe.tool({ path: "inventory.lookup" });

console.log(lookup.data.typeSignature);
return hits.data.map((tool) => tool.path);
```

`tools.search` returns scored matches using registered path, name, title, and
description. `tools.describe.tool` returns the descriptor and a generated type
signature based on the registered JSON schemas.

## Aliases

Hosts can configure aliases with `ExecutionEngine::with_alias(namespace,
prefix)`. Aliases unwrap successful tool results and throw on failed tool
calls:

```rust
let engine = engine.with_alias("inventory", "inventory");
```

```ts
const item = await inventory.lookup({ sku: "abc-123" });
return item.quantity;
```

Aliases are convenient for concise scripts. Prefer `tools.*` calls when the
snippet needs to inspect failed tool envelopes directly.

## Policies and Limits

Hosts can attach a `ToolPolicy` and `RuntimeLimits` to the engine. Policy-filtered
tools are removed from the execution registry, so snippets cannot discover or
call them through `tools.*`.

```rust
use std::sync::Arc;
use tack_core::{DenyListPolicy, RuntimeLimits};

let limits = RuntimeLimits {
    max_tool_calls: 3,
    timeout_ms: 5_000,
    ..RuntimeLimits::default()
};

let engine = engine
    .with_policy(Arc::new(DenyListPolicy::new(vec![
        "inventory.delete".to_string(),
    ])))
    .with_limits(limits);
```

## Serialization

Returned values, emitted values, logged object values, and tool inputs use
`JSON.stringify` semantics.

- top-level `undefined` returns `null`
- `BigInt` and cyclic return values fail execution with a runtime error
- cyclic or non-serializable tool inputs return a failed tool envelope without
  calling the host
