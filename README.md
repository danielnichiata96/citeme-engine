# citeme-engine

Bibliography interchange and CSL formatting for Rust and the browser.

**Reads** BibTeX, RIS, MEDLINE/NBIB and CSL-JSON — with format auto-detection.
**Writes** BibTeX, BibLaTeX, RIS and Hayagriva YAML.
**Renders** citations and bibliographies in any CSL style, in 7 locales.

Ships as a ~540 KB (gzip) Wasm module with no runtime dependencies, so all of
it runs client-side. Built for and used in production by
[CiteMe](https://citeme.app).

CSL rendering is [Hayagriva](https://github.com/typst/hayagriva)'s — this
crate doesn't reimplement it.

## What this adds on top of Hayagriva

Hayagriva is an excellent CSL renderer with a deliberately small I/O surface:
it reads BibLaTeX and its own YAML, and writes that YAML. Everything below is
what you need around it to actually exchange bibliographies with the rest of
the world.

| | Hayagriva 0.9 | here |
|---|---|---|
| Read BibTeX/BibLaTeX | ✅ | ✅ error-tolerant, → CSL-JSON |
| Read RIS | — | ✅ |
| Read MEDLINE/NBIB (PubMed) | — | ✅ |
| Read CSL-JSON | ✅ | ✅ validated, single-or-array |
| Detect format from raw text | — | ✅ |
| **Write BibTeX** | — | ✅ |
| **Write BibLaTeX** | — | ✅ |
| **Write RIS** | — | ✅ |
| Write Hayagriva YAML | from `Library` | ✅ from raw CSL-JSON |
| Render CSL | ✅ | ✅ (Hayagriva) |
| Runs in the browser | — | ✅ |

Two more things worth knowing about:

- **CSL XML normalization** (`crates/core/src/normalize.rs`) works around a
  Hayagriva ≤0.9 date-rendering bug: the `delimiter` of a `<date>` element is
  emitted between every `<date-part>` even when a neighbor is empty, so a
  year-only date renders as `"2018 "` and a parent suffix turns it into
  `"2018 ;"`. Every style and locale is rewritten to use per-part `prefix`
  instead. The transform is idempotent.
- **ABNT post-processing** (`crates/core/src/abnt.rs`) applies the family-name
  uppercasing that Brazil's ABNT 2023 requires and CSL can't express, while
  protecting institutional acronyms (IBGE, CAPES, USP, …) from being mangled.

### Error tolerance

Parsing returns partial results instead of failing the batch — importing
user-pasted content, 47 good entries out of 50 beats an error:

```jsonc
{
  "entries": [ /* CSL-JSON */ ],
  "errors":  [ { "preview": "@article{broken,", "error": "…" } ],
  "format": "bibtex",
  "truncated": false,
  "scannedEntries": 50
}
```

## Install

```bash
npm install citeme-engine-wasm
```

Styles and locales are not bundled — you supply the XML, so you control which
of the [10k+ CSL styles](https://github.com/citation-style-language/styles)
you ship and where they're cached.

```js
import { createEngine, resultShapeVersion } from 'citeme-engine-wasm';

const engine = await createEngine();
engine.loadStyle('apa', apaXml);       // CSL style XML
engine.loadLocale('en-US', localeXml); // CSL locale XML

const { reference, inText } = JSON.parse(
  engine.formatOne(cslJsonStr, 'apa', 'en-US', false)
);

// Import anything, get CSL-JSON back
const parsed = JSON.parse(engine.parseAuto(pastedText));

// Export it again
const bib = engine.exportBibtex(JSON.stringify(parsed.entries));
```

Assert `resultShapeVersion() === 1` once after init and every returned JSON
shape is pinned; see *Error semantics* below.

> **Formatted output is HTML, and it is not sanitized.** Like every CSL
> processor, the engine emits markup — `<i>`, `<sub>`, `<span>` — because CSL
> styles and CSL-JSON legitimately carry it (`The <i>Drosophila</i> genome`).
> It does not distinguish that from markup an attacker put in a title, so a
> hostile `title` reaches your DOM verbatim. **Sanitize before rendering as
> HTML**, or pass `formatOneWithOutput(…, "plain")` and get text back. Same
> contract as citeproc-js and citation-js.

Also available as a plain Rust crate (`crates/core`) with no Wasm involved.

## Conformance

**Not a full CSL processor, and not trying to be.** What's guaranteed is that
the 60 styles in `tests/fixtures/styles/corpus/` — the set CiteMe serves in
production, spanning APA, ABNT, Chicago, Harvard, IEEE, MLA, Vancouver and
others — load and format without panicking across all supported locales,
enforced in CI on every commit. Arbitrary CSL beyond that corpus generally
works, because Hayagriva does the rendering, but it isn't tested here and this
project does not run the official
[CSL test suite](https://github.com/citation-style-language/test-suite).

If you need audited spec conformance, [citeproc-js](https://github.com/Juris-M/citeproc-js)
is the reference implementation. If you need bibliography I/O that runs in a
browser tab in under a millisecond, that's this.

Locales tested: `en-US`, `en-GB`, `pt-BR`, `pt-PT`, `es-ES`, `fr-FR`, `de-DE`.

## Architecture

- `crates/core/` — pure Rust: engine, parsers, exporters, normalization, ABNT
- `crates/wasm/` — thin `wasm-bindgen` wrapper
- `js/` — npm package (`citeme-engine-wasm`)

## Performance

Measured in CiteMe's pipeline against [citation-js](https://citation.js.org),
the JS toolchain it replaced:

| Operation | citation-js | here |
|---|---|---|
| `formatOne`, APA | 1.4 ms | 0.5 ms |
| `formatOne`, ABNT | 0.8 ms | 0.08 ms |
| `formatBatch`, 50 items | 51 ms | 40 ms |
| End-to-end, 5 items, cold | 444 ms | 52 ms |

Numbers are from one machine and one workload — treat them as an order of
magnitude, not a benchmark result. `cargo bench` (Criterion) measures the Rust
side directly if you want to reproduce the formatting figures locally.

## Error semantics across the Wasm boundary

Two distinct failure modes, with different blast radii:

- **Expected failures return `Err`.** Every fallible API (`loadStyle`,
  `formatOne`, `parse*`, `export*`, …) returns `Result<_, JsError>` — invalid
  XML, malformed JSON, unknown style names, unloaded locales (since 0.3.5 —
  formatting with a locale you never loaded would otherwise silently drop
  every locale term), unsupported output formats all surface as ordinary JS
  exceptions with a message. The engine instance stays healthy; just catch
  and continue.
- **A Rust panic aborts the instance — and cannot be caught at the
  boundary.** This crate builds with `panic = "abort"`, and
  `wasm32-unknown-unknown` has no unwinding anyway, so `catch_unwind`-style
  conversion to `Err` is not possible on this target. A panic reaches JS as a
  `RuntimeError: unreachable` and poisons the `WasmCitationEngine`; the only
  recovery is re-instantiating the module.

Because the second mode can't be intercepted, the engine's guarantee is to
make it **unreachable** rather than catchable, enforced in CI:

- `crates/core/tests/no_panic_props.rs` — property tests asserting no
  arbitrary input (including multibyte/XML-ish adversarial strings) panics
  the normalizer, parsers, or exporters.
- `crates/core/tests/corpus_smoke.rs` — every CSL style CiteMe serves in
  production (`tests/fixtures/styles/corpus/`) must load and format a
  battery of items in all supported locales.

Consumers may keep a last-resort "reset engine on `RuntimeError`" guard as
defense in depth, but as of 0.3.4 it is expected to be dead code.

To assert the JSON contract once at boot instead of per call, compare
`resultShapeVersion()` against the version your schemas were written for
(currently `1`).

## Export compatibility notes (vs citation-js)

The BibTeX/RIS exporters intentionally do **not** reproduce citation-js
output byte-for-byte. Both outputs are valid; pick whichever toolchain you
trust, but don't diff them against each other.

On **import**, both engines strip BibTeX case-protection braces the same way
(`The {DNA} of {C}ities` → `The DNA of Cities`), so braces never leak into
CSL-JSON or RIS output.

On **BibTeX export** the conventions differ:

| Aspect | citeme-engine | citation-js |
|---|---|---|
| Case protection | none — emits the title as stored | wraps every capitalized word in braces (`A {Study} of {DNA}`) |
| Non-ASCII text | preserved as UTF-8 (valid for modern BibTeX/biber) | converted to LaTeX escapes (`Garc{\' i}a`) |
| `month` | 3-letter macro, unbraced (`month = mar`) | numeric, braced (`month = {3}`) |
| Page ranges | as stored (`100-115`) | en-dash normalized (`100--115`) |
| Cite keys | `<Author><Year>` (+ `a`/`b`/… dedup) | `<Author><Year><FirstTitleWord>` |

On **RIS export** both engines emit plain values (no brace handling exists in
either); remaining differences are tag-level (`JO` vs `T2` for journal,
citation-js adds an `ID` tag).

## Contributing

Bug reports welcome — reachable panics and round-trip failures especially.
Feature requests may be declined; this is a single-maintainer project built
around one product's needs. See [CONTRIBUTING.md](CONTRIBUTING.md) and
[SECURITY.md](SECURITY.md).

## License

MIT — see [LICENSE](LICENSE).

The CSL styles and locales vendored as test fixtures are **not** MIT: they
belong to the [Citation Style Language](https://citationstyles.org) project
and are CC BY-SA 3.0. See [NOTICE.md](NOTICE.md).
