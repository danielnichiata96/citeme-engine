import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import { createEngine, resultShapeVersion } from '../index.js';

// The consumer-facing API surface. CiteMe's adapters call these by name
// (engine-wasm.ts, wasm-browser.ts, import-parser.ts); a wasm-bindgen
// js_name rename ships as a silent per-call failure over there, so pin
// the whole surface here and fail the release instead. Removing or
// renaming a method requires a deliberate edit to this list — treat that
// as a breaking change for the CiteMe adapters.
const ENGINE_METHODS = [
  'loadStyle',
  'loadLocale',
  'hasStyle',
  'hasLocale',
  'formatOne',
  'formatBatch',
  'formatOneProse',
  'formatBatchProse',
  'formatOneWithOutput',
  'formatBatchWithOutput',
  'detectFormat',
  'parseAuto',
  'parseBibtex',
  'parseRis',
  'parseCslJson',
  'parseMedline',
  'exportBibtex',
  'exportRis',
  'exportBiblatex',
  'exportHayagriva',
  'version',
];

const wasmBytesPromise = readFile(new URL('../pkg/citeme_engine_wasm_bg.wasm', import.meta.url));

test('engine exposes the full consumer API surface', async () => {
  const engine = await createEngine({ module_or_path: await wasmBytesPromise });

  for (const method of ENGINE_METHODS) {
    assert.equal(
      typeof engine[method],
      'function',
      `engine.${method} missing or not a function — CiteMe adapters call it by this name`,
    );
  }
});

test('result shape version matches what CiteMe adapters were written against', async () => {
  await createEngine({ module_or_path: await wasmBytesPromise });

  // CiteMe's engine-wasm.ts asserts SUPPORTED_RESULT_SHAPE_VERSION === 1
  // at init. Bumping this constant is a coordinated change: update
  // wasm-schemas.ts + engine-wasm.ts in the app in the same release.
  assert.equal(resultShapeVersion(), 1);
});
