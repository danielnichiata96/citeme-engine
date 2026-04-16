// js/index.js
import init, { WasmCitationEngine } from './pkg/citeme_engine_wasm.js';

let initPromise = null;

export async function createEngine() {
  if (!initPromise) {
    initPromise = init();
  }
  await initPromise;
  return new WasmCitationEngine();
}

export { init, WasmCitationEngine };
