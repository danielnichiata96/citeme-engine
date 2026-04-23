// js/index.d.ts

import type { InitInput, InitOutput, WasmCitationEngine } from './pkg/citeme_engine_wasm.js';
export { WasmCitationEngine } from './pkg/citeme_engine_wasm.js';
export type { InitInput, InitOutput } from './pkg/citeme_engine_wasm.js';

export interface FormatResult {
  reference: string;
  inText: string;
}

export interface ParseResult {
  entries: unknown[];
  errors: Array<{ preview: string; error: string }>;
  format: string;
  truncated: boolean;
  scannedEntries: number;
}

export type InitOptions = {
  module_or_path: InitInput | Promise<InitInput>;
};

export type InitArgument = InitOptions | InitInput | Promise<InitInput>;

export declare function createEngine(moduleOrPath?: InitArgument): Promise<WasmCitationEngine>;

/**
 * Initialize the Wasm module. Call before constructing WasmCitationEngine.
 * Pass `{ module_or_path }` to specify an explicit URL for the .wasm binary
 * (required in bundled environments where import.meta.url is unreliable).
 */
export declare function init(moduleOrPath?: InitArgument): Promise<InitOutput>;

export default init;
