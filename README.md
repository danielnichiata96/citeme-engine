# citeme-engine

Rust/Wasm citation formatting engine for CiteMe.

## Quick Start

```bash
# Build
./build.sh

# Test
cargo test --workspace

# Use from JS
import { createEngine } from '@citeme/citation-engine-wasm';
const engine = await createEngine();
engine.loadStyle('apa', apaXml);
engine.loadLocale('en-US', localeXml);
const result = JSON.parse(engine.formatOne(cslJsonStr, 'apa', 'en-US', false));
```

## Architecture

- `crates/core/` — Pure Rust logic (CitationEngine, parsers, ABNT)
- `crates/wasm/` — Thin wasm-bindgen wrapper
- `js/` — npm package (`@citeme/citation-engine-wasm`)

Powered by [Hayagriva](https://github.com/typst/hayagriva) v0.9.1 with the `csl-json` feature for direct CSL-JSON formatting.
