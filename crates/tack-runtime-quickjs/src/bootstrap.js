const __tackStringify = JSON.stringify;
const __tackParse = JSON.parse;
const __tackString = String;
const __tackDefine = Object.defineProperty;
const __tackFunctionCall = Function.prototype.call;
const __tackHasOwn = __tackFunctionCall.bind(Object.prototype.hasOwnProperty);
const __tackHostCallJson = globalThis.__tackCallJson;
const __tackHostEmitJson = globalThis.__tackEmitJson;
const __tackHostLogLine = globalThis.__tackLogLine;
function __tackEmit(value) {
  const json = __tackStringify(value);
  if (typeof json === 'string') __tackHostEmitJson(json);
}
function __tackFormatLogArg(value) {
  if (typeof value === 'string') return value;
  if (typeof value === 'undefined') return 'undefined';
  if (typeof value === 'function') return value.toString();
  try {
    const json = __tackStringify(value);
    return typeof json === 'undefined' ? __tackString(value) : json;
  } catch (_error) {
    return __tackString(value);
  }
}
function __tackToolFailure(message) {
  return { ok: false, error: { message } };
}
function __tackSerializeToolInput(input) {
  let inputJson;
  try {
    inputJson = __tackStringify(input);
  } catch (_error) {
    return { ok: false, output: __tackToolFailure('tool request is not JSON serializable') };
  }
  if (typeof inputJson !== 'string') {
    return { ok: false, output: __tackToolFailure('tool request is not JSON serializable') };
  }
  if (inputJson.length > __tackMaxToolRequestBytes) {
    return { ok: false, output: __tackToolFailure('tool request too large') };
  }
  return { ok: true, inputJson };
}
function __tackCallable(path, direct) {
  return async (input = {}) => {
    const serialized = __tackSerializeToolInput(input);
    const output = serialized.ok
      ? __tackParse(await __tackHostCallJson(path, serialized.inputJson))
      : serialized.output;
    if (!direct) return output;
    if (!output.ok) {
      const message = output.error && output.error.message ? output.error.message : `Tool failed: ${path}`;
      throw new Error(message);
    }
    if (__tackHasOwn(output, "data")) return output.data;
    if (__tackHasOwn(output, "text")) return output.text;
    return output.raw ?? null;
  };
}
function __tackInvokeArguments(args) {
  if (args.length === 1 && args[0] !== null && typeof args[0] === 'object' && __tackHasOwn(args[0], 'path')) {
    if (typeof args[0].path !== 'string' || args[0].path.length === 0) {
      return { error: __tackToolFailure('tool path is required') };
    }
    return {
      path: args[0].path,
      input: __tackHasOwn(args[0], 'input') ? args[0].input : {}
    };
  }
  if (typeof args[0] !== 'string' || args[0].length === 0) {
    return { error: __tackToolFailure('tool path is required') };
  }
  return {
    path: args[0],
    input: args.length > 1 ? args[1] : {}
  };
}
function __tackNamespace(prefix, direct) {
  return new Proxy(function(){}, {
    get(_target, prop) {
      if (prop === 'then') return undefined;
      if (prefix === '' && prop === 'invoke') {
        return async (...args) => {
          const call = __tackInvokeArguments(args);
          if (call.error) return call.error;
          return __tackCallable(call.path, false)(call.input);
        };
      }
      const name = String(prop);
      const path = prefix ? `${prefix}.${name}` : (name === 'search' || name === 'describe' ? `tools.${name}` : name);
      return __tackNamespace(path, direct);
    },
    apply(_target, _thisArg, args) {
      return __tackCallable(prefix, direct)(args.length === 0 ? {} : args[0]);
    }
  });
}
__tackDefine(globalThis, 'emit', { value: __tackEmit, writable: false, configurable: false });
__tackDefine(globalThis, 'console', {
  value: { log: (...args) => {
    let line = '';
    for (let i = 0; i < args.length; i += 1) {
      if (i > 0) line += ' ';
      line += __tackFormatLogArg(args[i]);
    }
    __tackHostLogLine(line);
  } },
  writable: false,
  configurable: false
});
__tackDefine(globalThis, 'tools', { value: __tackNamespace('', false), writable: false, configurable: false });
