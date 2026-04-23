import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import initDefault, { init, createEngine, WasmCitationEngine } from '../index.js';

const wasmBytesPromise = readFile(new URL('../pkg/citeme_engine_wasm_bg.wasm', import.meta.url));
const localePromise = readFile(
  new URL('../../tests/fixtures/locales/locales-en-US.xml', import.meta.url),
  'utf8',
);
const stylePromise = readFile(
  new URL('../../tests/fixtures/styles/apa.csl', import.meta.url),
  'utf8',
);

test('init is available as both default and named exports', () => {
  assert.equal(initDefault, init);
});

test('createEngine accepts module_or_path and exposes style/locale introspection', async () => {
  const engine = await createEngine({ module_or_path: await wasmBytesPromise });

  assert.ok(engine instanceof WasmCitationEngine);
  assert.equal(engine.hasStyle('apa'), false);
  assert.equal(engine.hasLocale('en-US'), false);

  engine.loadStyle('apa', await stylePromise);
  engine.loadLocale('en-US', await localePromise);

  assert.equal(engine.hasStyle('apa'), true);
  assert.equal(engine.hasLocale('en-US'), true);
});

test('parseBibtex normalizes combining-mark LaTeX commands', async () => {
  const engine = await createEngine({ module_or_path: await wasmBytesPromise });
  const acute = '\u0301';
  const tilde = '\u0303';
  const bib = `@article{x,\n  title = {Aspectos jur\\${acute}idicos e Jo\\${tilde}ao},\n  author = {Silva, Jo\\${tilde}ao and Garc\\${acute}ia, Mar\\${acute}ia},\n  journal = {Revista},\n  year = {2024}\n}`;

  const result = JSON.parse(engine.parseBibtex(bib));

  assert.equal(result.errors.length, 0);
  assert.equal(result.entries.length, 1);
  assert.equal(result.entries[0].title, 'Aspectos jurídicos e João');
  assert.deepEqual(result.entries[0].author, [
    { family: 'Silva', given: 'João' },
    { family: 'García', given: 'María' },
  ]);
});
