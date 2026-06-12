# Changelog

All notable changes to `citeme-engine` are documented here.

This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
and the [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format.

## [0.3.4] — 2026-06-12

### Fixed
- `init`/`createEngine` no longer trigger wasm-bindgen's
  `using deprecated parameters for the initialization function` console
  warning when called with a raw input (bytes, URL, string) instead of a
  `{ module_or_path }` object. The package wrapper now normalizes the
  argument itself — previously every serverless cold start logged the
  warning.

### Added
- **Versioned result-shape contract.** New `resultShapeVersion()` export
  (module-level, also re-exported from the package root) returns the version
  of every JSON shape the engine emits (`FormatResult`, `ParseResult`, …) —
  currently `1`. Consumers that validate engine output against their own
  schemas can assert it once at init, turning shape drift into a loud boot
  failure instead of per-call validation errors. The exact serialized shapes
  are pinned by unit tests in `crates/core/src/types.rs`; editing those tests
  requires bumping `RESULT_SHAPE_VERSION`.
- **Production-corpus smoke test.** `tests/fixtures/styles/corpus/` snapshots
  all 59 CSL styles the CiteMe app serves; CI loads every one and formats a
  battery of representative items (multibyte text, patent `submitted` dates,
  literal authors, missing fields) in all 7 supported locales. A style edge
  case now fails engine CI instead of consumer production.
- **Anti-panic property tests.** Proptest suites assert that no arbitrary
  input — including adversarial XML-ish strings heavy on multibyte chars —
  can panic the CSL normalizer, any parser, or any exporter. This is the
  class-level defense for the bug fixed in 0.3.3 (panics cross the Wasm
  boundary as instance-poisoning aborts and cannot be caught on
  `wasm32-unknown-unknown`).
- **GitHub Actions CI** running the full Rust workspace suite plus a real
  Wasm build with the JS package tests.
- `locales-pt-PT.xml` test fixture (completes the CiteMe locale set).

### Notes
- **iso690-fr renders without aborting, but output parity with citation-js
  is NOT claimed.** Hayagriva currently drops the standalone
  `<text term="and"/>` between the patent dates and renders months
  numerically, producing e.g. `…le 3 2 2021publié…` (missing separator,
  numeric month) — upstream rendering gaps, not the 0.3.3 normalizer fix.
  Consumers switching this style from a fallback formatter to the Wasm
  engine should gate on an output comparison first; the no-abort guarantee
  holds regardless.
- README gained an "Error semantics across the Wasm boundary" section:
  expected failures are `Err`/`JsError` and never poison the instance; Rust
  panics abort and *cannot* be converted to `Err` on this target, so the
  engine's guarantee is making them unreachable (enforced by the corpus and
  property suites). Consumer-side reset guards are expected to be dead code
  from this release on.

## [0.3.3] — 2026-06-12

### Fixed
- **CSL normalizer no longer panics on styles with multibyte characters in
  attribute values** (e.g. `iso690-author-date-fr`'s
  `<date … prefix="Brevet déposé le ">`). The `delimiter` attribute scanner
  advanced byte-by-byte and could slice mid-character, aborting across the
  Wasm boundary on `loadStyle`. Scanning is now char-boundary-safe; the real
  style is a test fixture and loads/formats cleanly.

### Added
- **Exports map now supports path resolution from any module system.**
  `js/package.json` adds `"./package.json"` and
  `"./pkg/citeme_engine_wasm_bg.wasm"` subpaths plus a `"default"` condition
  on `"."`, so `require.resolve('citeme-engine-wasm')`,
  `require.resolve('citeme-engine-wasm/pkg/citeme_engine_wasm_bg.wasm')` and
  the `import.meta.resolve` equivalents all work — no more
  `ERR_PACKAGE_PATH_NOT_EXPORTED` / filesystem walk-ups in consumers.
  Additive; existing `import` resolution is unchanged.

### Notes
- Evaluated matching citation-js "brace-cleaning" in the BibTeX/RIS
  exporters; declined. On import both engines already strip case-protection
  braces identically, so nothing leaks into RIS. On BibTeX export,
  citation-js *adds* case-protection braces around capitalized words and
  rewrites non-ASCII text as LaTeX escapes — opinionated conventions (and
  typographically questionable, e.g. `{\' i}`) that would silently change
  output for existing consumers. The divergence is documented in the README
  ("Export compatibility notes") instead.

## [0.3.2] — 2026-05-03

### Added
- Wasm API methods `formatOneWithOutput` and `formatBatchWithOutput` for
  explicit `"html"` vs `"plain"` rendering without changing the existing
  `formatOne` / `formatBatch` signatures.

### Fixed
- BibLaTeX import now preserves BibLaTeX-only entry types and fields used by
  CiteMe exports, including `@dataset`, `@software`, `journaltitle`, `location`,
  `eventtitle`, `volume`, `number`, `url`, `version`, and `urldate`.
- MEDLINE/NBIB parser now accepts normalized tag spacing such as `TI - value`
  as well as fixed-width `TI  - value`.

## [0.3.0] — 2026-04-20

### Added
- **Native BibLaTeX exporter** (`exportBiblatex`) with canonical fields:
  `journaltitle` (not `journal`), `location` (not `address`), unified ISO
  `date = YYYY-MM-DD`, `@online` for webpage/post-weblog, `eprintclass`
  (not `primaryClass`). Emits `pmid = {…}` + biblatex-idiomatic
  `eprint = {…}, eprinttype = {pubmed}` dual-form for style compatibility.
- BibTeX parser now preserves `abstract`, `note`, `keyword` (CSL singular
  string), `collection-title` (from `series`), `chapter-number`, `PMID`,
  `PMCID`, and `custom.eprint = { id, type, class }`. Covers both canonical
  hayagriva fields and fields hayagriva drops (keywords, direct pmid/pmcid,
  eprintclass) via a dual-parse through the `biblatex` crate.
- BibTeX exporter now emits `keywords` (plural, from CSL singular `keyword`),
  `note`, `series`, `chapter`, `pmid`, `pmcid`, `eprint`/`eprinttype`/
  `eprintclass`, and `month` as a 3-letter macro (`month = mar`, no braces,
  never numeric).
- Array exporters (`exportBibtex`, `exportBiblatex`) automatically
  disambiguate duplicate cite keys with `a`/`b`/`c`/… suffixes (rolls past
  `z` to `aa`, `ab`, …).

### Changed
- `biblatex = "0.11"` is now a direct dependency (was transitive via
  `hayagriva`); used for the dual-parse path.

### Notes
- The BibTeX dual-parse path tolerates duplicate cite keys and individual
  malformed entries via per-chunk fallback, so metadata enrichment still
  fires on the non-duplicate / non-broken entries.
- `eprint = {…}` without `eprinttype` is ignored rather than emitted as an
  ambiguous `custom.eprint`. Both fields must be present to reach the CSL
  output (and to survive roundtrip through the exporters).
- Out-of-range month/day in the CSL `issued.date-parts` are silently
  dropped to the next-lower precision in the BibLaTeX exporter (prevents
  malformed EDTF that Biber would reject).
