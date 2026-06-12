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
