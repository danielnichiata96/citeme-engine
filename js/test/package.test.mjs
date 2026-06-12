import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import initDefault, { init, createEngine, resultShapeVersion, WasmCitationEngine } from '../index.js';

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

test('parseBibtex preserves BibLaTeX-only entry types and fields', async () => {
  const engine = await createEngine({ module_or_path: await wasmBytesPromise });
  const bib = `@dataset{data2023,
  author = {Lab, Some},
  title = {Dataset X},
  publisher = {Zenodo},
  date = {2023},
  doi = {10.5281/zenodo.1234567}
}

@software{tool2024,
  author = {Dev Team},
  title = {A Tool},
  version = {1.2.0},
  date = {2024-03-01},
  url = {https://github.com/x/y}
}`;

  const result = JSON.parse(engine.parseBibtex(bib));

  assert.equal(result.errors.length, 0);
  assert.equal(result.entries[0].type, 'dataset');
  assert.equal(result.entries[0].publisher, 'Zenodo');
  assert.equal(result.entries[1].type, 'software');
  assert.equal(result.entries[1].URL, 'https://github.com/x/y');
  assert.equal(result.entries[1].version, '1.2.0');
});

test('parseMedline accepts collapsed tag spacing', async () => {
  const engine = await createEngine({ module_or_path: await wasmBytesPromise });
  const nbib = `PMID- 41764257
TI - A normalized MEDLINE title.
FAU - Zaman, Khalid
DP - 2026 Feb
JT - Scientific reports
LID - 10.1038/s41598-026-40798-8 [doi]
`;

  const result = JSON.parse(engine.parseMedline(nbib));

  assert.equal(result.errors.length, 0);
  assert.equal(result.entries.length, 1);
  assert.equal(result.entries[0].title, 'A normalized MEDLINE title.');
  assert.equal(result.entries[0]['container-title'], 'Scientific reports');
});

test('resultShapeVersion pins the result-JSON contract at init time', async () => {
  const engine = await createEngine({ module_or_path: await wasmBytesPromise });

  // Consumers assert this once at boot; bumping it is a contract change.
  assert.equal(resultShapeVersion(), 1);

  // Pin the shape the version covers, exactly as the boundary serializes it.
  engine.loadStyle('apa', await stylePromise);
  engine.loadLocale('en-US', await localePromise);
  const formatted = JSON.parse(engine.formatOne(
    JSON.stringify({ type: 'book', title: 'T', author: [{ family: 'X' }], issued: { 'date-parts': [[2024]] } }),
    'apa', 'en-US', false,
  ));
  assert.deepEqual(Object.keys(formatted).sort(), ['inText', 'reference']);

  const parsed = JSON.parse(engine.parseBibtex('@book{k, title={T}, author={X}, year={2024}}'));
  assert.deepEqual(
    Object.keys(parsed).sort(),
    ['entries', 'errors', 'format', 'scannedEntries', 'truncated'],
  );
});

test('loadStyle survives multibyte chars in CSL attributes (iso690-fr regression)', async () => {
  // 0.3.2 panicked (Wasm abort) while normalizing iso690-author-date-fr's
  // `<date … prefix="Brevet déposé le ">` — multibyte char in a <date> tag.
  const engine = await createEngine({ module_or_path: await wasmBytesPromise });
  const styleXml = await readFile(
    new URL('../../tests/fixtures/styles/iso690-author-date-fr.csl', import.meta.url),
    'utf8',
  );
  const localeXml = await readFile(
    new URL('../../tests/fixtures/locales/locales-fr-FR.xml', import.meta.url),
    'utf8',
  );

  engine.loadStyle('iso690-author-date-fr', styleXml);
  engine.loadLocale('fr-FR', localeXml);

  const patent = JSON.stringify({
    type: 'patent',
    title: 'Dispositif de chiffrement',
    author: [{ family: 'Dupont', given: 'Marie' }],
    submitted: { 'date-parts': [[2019, 3, 14]] },
    issued: { 'date-parts': [[2021, 7, 2]] },
    number: 'FR3094000',
  });

  const result = JSON.parse(engine.formatOne(patent, 'iso690-author-date-fr', 'fr-FR', false));
  assert.ok(result.reference.length > 0, 'reference should not be empty');
});

test('formatOneWithOutput supports plain output', async () => {
  const engine = await createEngine({ module_or_path: await wasmBytesPromise });
  engine.loadStyle('apa', await stylePromise);
  engine.loadLocale('en-US', await localePromise);
  const book = JSON.stringify({
    type: 'book',
    title: 'The Elements of Statistical Learning',
    author: [{ family: 'Hastie', given: 'Trevor' }],
    issued: { 'date-parts': [[2009]] },
    publisher: 'Springer',
  });

  const html = JSON.parse(engine.formatOneWithOutput(book, 'apa', 'en-US', false, 'html', false));
  const plain = JSON.parse(engine.formatOneWithOutput(book, 'apa', 'en-US', false, 'plain', false));

  assert.match(html.reference, /<span|<i|<em/);
  assert.doesNotMatch(plain.reference, /<[^>]+>/);
  assert.equal(plain.inText, '(Hastie, 2009)');
});
