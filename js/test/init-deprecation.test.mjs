import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import { createEngine } from '../index.js';

// Own test file = own process = fresh (un-initialized) wasm module, so this
// test controls the very first init call.

test('positional init input does not trigger the wasm-bindgen deprecation warning', async (t) => {
  const warnings = [];
  const originalWarn = console.warn;
  console.warn = (...args) => { warnings.push(args.join(' ')); };
  t.after(() => { console.warn = originalWarn; });

  // Raw bytes, positionally — the call shape consumers actually use. The
  // generated init only accepts a plain `{ module_or_path }` object without
  // warning; index.js must wrap raw inputs so consumers don't get a
  // console.warn on every serverless cold start.
  const bytes = await readFile(new URL('../pkg/citeme_engine_wasm_bg.wasm', import.meta.url));
  const engine = await createEngine(bytes);

  assert.equal(engine.hasStyle('apa'), false, 'engine should be functional');
  assert.deepEqual(
    warnings.filter((w) => w.includes('deprecated parameters')),
    [],
    `init must not warn: ${warnings.join(' | ')}`,
  );
});
