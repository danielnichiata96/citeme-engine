// js/index.js
import init, { WasmCitationEngine } from './pkg/citeme_engine_wasm.js';

let initialized = false;

export async function createEngine() {
  if (!initialized) {
    await init();
    initialized = true;
  }
  return new WasmCitationEngine();
}

export { WasmCitationEngine };
