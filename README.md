# citeme-engine

Rust/Wasm citation formatting engine for CiteMe.

## Quick Start

```bash
# Build
./build.sh

# Test
cargo test --workspace

# Use from JS
import { createEngine } from 'citeme-engine-wasm';
const engine = await createEngine();
engine.loadStyle('apa', apaXml);
engine.loadLocale('en-US', localeXml);
const result = JSON.parse(engine.formatOne(cslJsonStr, 'apa', 'en-US', false));
```

## Architecture

- `crates/core/` — Pure Rust logic (CitationEngine, parsers, ABNT)
- `crates/wasm/` — Thin wasm-bindgen wrapper
- `js/` — npm package (`citeme-engine-wasm`)

Powered by [Hayagriva](https://github.com/typst/hayagriva) v0.9.1 with the `csl-json` feature for direct CSL-JSON formatting.

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
