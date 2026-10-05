# Vendored dependencies

## `hayagriva/` — hayagriva 0.9.1, one fix

The crates.io release of [hayagriva](https://github.com/typst/hayagriva)
0.9.1 (MIT OR Apache-2.0, licenses alongside), wired in through
`[patch.crates-io]` in the root `Cargo.toml`. Unchanged except:

- `Cargo.toml`: the `[[bin]]` and `[[test]]` targets are removed — their
  sources (`src/main.rs` is kept but unused, `tests/`) aren't needed to build
  the library, and the test fixtures aren't vendored.
- `src/csl/mod.rs`, `WritingContext::has_content_since`: the text after an
  affix was sliced at the affix's byte length. `push_str` may have trimmed the
  affix's trailing space in between ("punctuation eats spaces"), so the index
  could fall inside a multibyte character — a panic, which aborts the Wasm
  instance. Reached by a style prefix ending in a space (`"pp.&#160;"`) and a
  value opening with punctuation then a multibyte character (`".é"`); the
  slice now starts at the next character boundary.

`crates/core/tests/format_input.rs` (`a_value_opening_with_punctuation_is_not_a_panic`)
and the formatting property in `no_panic_props.rs` fail without the fix.

hayagriva 0.10.1 still has the bug. Delete this directory and the
`[patch.crates-io]` entry once an upstream release fixes it — and re-run
`cargo test --workspace` with a high `PROPTEST_CASES` to confirm.

### Why not hayagriva 0.10

Evaluated for this release and not taken (details in the CHANGELOG): it adds
ICU collation data that doubles the Wasm binary (540 → 1015 KB gzip), and
over the production style corpus it regresses APA-family disambiguation
(`Smith. (2020)` — the given name dropped — instead of `Smith, J. (2020a)`),
truncates AGLC page ranges (`100–115` → `100`), and turns on
subsequent-author substitution, which can't be right for results returned
one item at a time.
