// js/index.js
import initWasm, { WasmCitationEngine } from './pkg/citeme_engine_wasm.js';

let initPromise = null;

function load(moduleOrPath) {
  initPromise = initWasm(moduleOrPath).catch((error) => {
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
export { WasmCitationEngine };
