import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import { createEngine } from '../index.js';

// Inputs that aborted the Wasm instance before 0.4.0. In the shipped build a
// Rust panic surfaces as `RuntimeError: unreachable` and poisons the
// instance, so each of these must now return a result or throw an ordinary
// Error — and the same instance must keep working afterwards. The Rust test
// suite runs with unwinding; only this runs the release binary.

const fixture = (path) => readFile(new URL(`../../tests/fixtures/${path}`, import.meta.url), 'utf8');

async function engineWithFixtures() {
  const engine = await createEngine({
    module_or_path: await readFile(new URL('../pkg/citeme_engine_wasm_bg.wasm', import.meta.url)),
  });
  engine.loadStyle('apa', await fixture('styles/apa.csl'));
  engine.loadStyle('iso690-fr', await fixture('styles/iso690-author-date-fr.csl'));
  engine.loadLocale('en-US', await fixture('locales/locales-en-US.xml'));
  engine.loadLocale('fr-FR', await fixture('locales/locales-fr-FR.xml'));
  return engine;
}

const book = (fields) => JSON.stringify({ type: 'book', title: 'T', author: [{ family: 'Doe' }], ...fields });

const healthy = (engine) => {
  const result = JSON.parse(engine.formatOne(book({ issued: { 'date-parts': [[2024]] } }), 'apa', 'en-US', false));
  assert.match(result.reference, /Doe/);
};

const cases = [
  ['a date range', () => [book({ issued: { 'date-parts': [[2019], [2020]] } }), 'apa', 'en-US']],
  ['a raw date range', () => [book({ issued: { raw: '2019/2020' } }), 'apa', 'en-US']],
  ['an empty date', () => [book({ issued: { 'date-parts': [[]] } }), 'apa', 'en-US']],
  [
    'a value opening with punctuation after an NBSP prefix',
    () => [
      JSON.stringify({
        type: 'article-journal', title: 'Titre', author: [{ family: 'Dupont' }],
        'container-title': 'Revue', volume: '.é', page: '.é', issued: { 'date-parts': [[2020]] },
      }),
      'iso690-fr',
      'fr-FR',
    ],
  ],
];

for (const [name, args] of cases) {
  test(`${name} does not abort the instance`, async () => {
    const engine = await engineWithFixtures();
    try {
      engine.formatOne(...args(), false);
    } catch (error) {
      assert.ok(error instanceof Error, String(error));
      assert.doesNotMatch(error.message, /unreachable/, 'a panic reached JS');
    }
    healthy(engine);
  });
}

const bibtexCases = [
  ['a day past 255 in month', '@article{k, title = {T}, year = {2020}, month = {may 300}}'],
  ['a signed year with a space', '@article{k, title = {T}, year = {- 2020}}'],
  ['non-ASCII digits in a date', '@article{k, title = {T}, date = {²²}}'],
  ['a crossref to itself', '@book{a, title = {T}, crossref = {a}}'],
  ['a self-referencing @string', '@string{a = a}\n@article{x, title = a, year = {2020}}'],
  ['deeply nested LaTeX commands', `@article{k, title = {${'\\emph{'.repeat(5000)}x${'}'.repeat(5000)}}, year = {2020}}`],
];

for (const [name, input] of bibtexCases) {
  test(`BibTeX import: ${name} does not abort the instance`, async () => {
    const engine = await engineWithFixtures();
    const result = JSON.parse(engine.parseBibtex(input));
    assert.equal(result.entries.length, 1, JSON.stringify(result.errors));
    healthy(engine);
  });
}

test('a date range is a clean, named error', async () => {
  const engine = await engineWithFixtures();
  assert.throws(
    () => engine.formatOne(book({ issued: { 'date-parts': [[2019], [2020]] } }), 'apa', 'en-US', false),
    /unsupported CSL-JSON input: issued: date ranges/,
  );
});

test("CiteMe's arXiv item formats (custom.eprint used to fail every call)", async () => {
  const engine = await engineWithFixtures();
  const item = JSON.stringify({
    type: 'article-journal', id: 'p1', title: 'Attention Is All You Need',
    author: [{ family: 'Vaswani', given: 'Ashish' }], issued: { 'date-parts': [[2017]] },
    'container-title': 'arXiv', custom: { eprint: { id: '1706.03762', type: 'arxiv' } },
  });
  const [result] = JSON.parse(engine.formatBatch(`[${item}]`, 'apa', 'en-US', false));
  assert.match(result.reference, /Vaswani/);
});
