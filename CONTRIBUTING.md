# Contributing

## Scope, and what to expect

This engine is developed for [CiteMe](https://citeme.app) and released under
MIT because it's useful outside it. That shapes what gets merged:

- **Bug reports are always welcome**, especially reachable panics, wrong CSL
  output with a named style, and round-trip failures on import/export. A
  failing input is worth more than a description.
- **Feature requests may be declined** without much discussion if they don't
  serve a use case the maintainer has. That isn't a judgment on the idea — a
  single maintainer can only support what they run in production. Forking is
  fine and MIT exists for exactly this.
- **No SLA.** Issues may sit for a while. Security reports get priority; see
  [SECURITY.md](SECURITY.md).

Deliberately out of scope: reimplementing CSL rendering (that's
[Hayagriva](https://github.com/typst/hayagriva) — send rendering bugs
upstream), and full CSL specification conformance (see *Conformance* in the
README).

## Before opening a PR

Open an issue first for anything beyond a bug fix — it's cheaper than writing
code that gets declined.

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
./build.sh && (cd js && npm test)
```

CI runs exactly these. `-D warnings` is enforced, so a clippy lint fails the
build.

## Tests

New behavior needs a test, and the test should fail before the fix.

- Parsers/exporters — a fixture in `tests/fixtures/` plus a case in the
  relevant `crates/core/tests/*.rs`.
- Anything touching untrusted input — extend
  `crates/core/tests/no_panic_props.rs` (proptest). This crate builds with
  `panic = "abort"`; a panic can't be caught at the Wasm boundary, so
  "unreachable" is the only acceptable guarantee. Run it with a high
  `PROPTEST_CASES` (e.g. 20000, `--release`) before a release: the default
  256 cases miss rare inputs.
- A panic you fixed — add its input to `js/test/no-abort.test.mjs`, which
  runs the release binary, where a panic really is an abort.
- Export changes — `crates/core/tests/export_validity.rs` asserts that what we
  emit is accepted by a parser for that format. Keep it that way.
- New or changed public Wasm methods — pin them in
  `js/test/api-surface.test.mjs`.

## Changing the returned JSON shape

Every JSON payload the engine returns is versioned by `resultShapeVersion()`.
A breaking shape change means bumping it, and consumers assert it once at
boot. Don't change a shape without bumping — silent drift turns into schema
errors scattered across call sites in downstream apps.

## Vendored Hayagriva

`vendor/hayagriva/` is Hayagriva 0.9.1 with one fix, wired in through
`[patch.crates-io]`; `vendor/README.md` says what changed and when to delete
it. Don't edit it beyond upstreamable fixes, and record each one there.

## Style corpus

`tests/fixtures/styles/corpus/` mirrors the CSL styles CiteMe serves in
production, and `corpus_smoke.rs` runs all of them in CI. It's synced with
`scripts/sync-corpus.sh`, not edited by hand. These files are CC BY-SA 3.0,
not MIT — see [NOTICE.md](NOTICE.md).
