// js/index.js
import initWasm, { WasmCitationEngine, resultShapeVersion } from './pkg/citeme_engine_wasm.js';

let initPromise = null;

// The generated init only accepts a plain `{ module_or_path }` object without
// complaint; any other value is treated as a deprecated positional parameter
// and logs a console.warn on every call. Wrap raw inputs (bytes, URL, string,
// Response, Module, Promise) ourselves, using the same plain-object check the
// generated code uses.
function normalizeInitArgument(moduleOrPath) {
  if (moduleOrPath == null) return undefined;
  if (Object.getPrototypeOf(moduleOrPath) === Object.prototype) return moduleOrPath;
  return { module_or_path: moduleOrPath };
}

function load(moduleOrPath) {
  initPromise = initWasm(normalizeInitArgument(moduleOrPath)).catch((error) => {
    initPromise = null;
    throw error;
  });
  return initPromise;
}

export function init(moduleOrPath) {
  return initPromise ?? load(moduleOrPath);
}

export async function createEngine(moduleOrPath) {
  await init(moduleOrPath);
  return new WasmCitationEngine();
}

export default init;
export { WasmCitationEngine, resultShapeVersion };
