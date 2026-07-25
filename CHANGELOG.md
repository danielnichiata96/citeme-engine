# Changelog

All notable changes to `citeme-engine` are documented here.

This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
and the [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format.

## [Unreleased]

Follow-up to the 0.3.7 export audit: that release escaped hostile *field*
values but left the author path, RIS, and the auto-detect entry point
unguarded.

### Fixed
- **Author names are escaped on BibTeX/BibLaTeX export.** `format_authors`
  interpolated `family`, `given`, `suffix` and `literal` raw, so a `}` in a
  family name closed the field and let the rest of the value open a second
  entry — a corrupt file, produced without raising anything.
- **RIS export can no longer be made to emit extra records.** RIS is
  line-oriented and nothing was filtered, so a newline in any value ended the
  field, and `\nER  - \nTY  - BOOK` ended the record and opened another. The
  crafted file re-imported as two entries with **zero errors**, which no
  consumer-side fallback can detect. All values now flow through a single
  `push_field` that flattens CR/LF/tab.
- **`non-dropping-particle` is preserved by every exporter.** Only
  `dropping-particle` was read, so "Maria da Silva" exported as "Silva,
  Maria" in BibTeX, BibLaTeX, RIS and Hayagriva YAML — a visible citation
  error for pt/es/nl/de names.
- **Hayagriva keys are unique and never empty.** Items without an `id` all
  took the literal key `entry` and `id: ""` emitted an empty key; duplicate
  YAML keys mean the last item silently wins, so exporting three id-less
  items loaded back as one.
- **`parseAuto` enforces `max_input_bytes` before detecting the format.**
  Detection proves JSON-ness by deserializing the entire document into a
  `Value`, so an oversized paste was fully allocated and parsed before any
  per-format parser could refuse it. Detection and dispatch moved into
  `parsers::parse_auto` so the guard runs first and is covered by Rust tests.
- **RIS tags without a trailing space parse.** The parser required `"  - "`
  while `detect_format` accepted `"TY  -JOUR"`, so a file detected as RIS
  yielded zero entries *and* zero errors. A detected-but-unreadable input now
  always reports an error rather than looking like an empty file.

### Documented
- Formatted HTML output is **not** sanitized — it can't be, since CSL styles
  and CSL-JSON legitimately carry `<i>`/`<sub>`/`<span>`. Consumers sanitize
  or use the `plain` output format. Same contract as citeproc-js and
  citation-js; stated in the README and scoped out in `SECURITY.md`.
- The npm package now ships the README (`build.sh` stages it into `js/`),
  which previously made the package page render no documentation at all.

## [0.3.7] — 2026-07-22

### Fixed
- **BibTeX/BibLaTeX export no longer emits unparseable files.**
  `escape_bibtex` now covers `{`, `}` and `\` (an unbalanced brace in a
  title corrupted the entry and everything after it), and cite keys are
  sanitized — commas, braces, parens, whitespace and other delimiter
  chars in a CSL `id` become `_`; ids with no alphanumeric content fall
  back to the derived author-year key. `export_validity` integration
  tests now round-trip every exporter through the engine's own parser.
- **Hayagriva export no longer emits YAML its own reader rejects.**
  String quoting is whitelist-based: scalars YAML would type-coerce
  (`true`, `null`, numbers) or that start/contain indicator chars
  (`- `, `@`, `*`, `&`, `!`, `: `…) are double-quoted with full escape
  coverage (backslash, quotes, control chars, newlines). `language`
  values that are not plausible BCP-47 tags are omitted entirely —
  hayagriva's `LanguageIdentifier` rejects junk even when quoted.
- **`detectFormat` accepts any BibTeX entry type.** Detection now looks
  for a generic `@word{` / `@word(` opener instead of a closed list of
  17 types — `@dataset`, `@software` and custom biblatex types no
  longer fall through to `unknown`. Bare `@` in prose still never
  matches.

### Changed
- Parse error arrays are capped at `MAX_PARSE_ERRORS` (100) — hostile
  input full of broken entries no longer balloons the result payload
  (`max_entries` bounded successful output but not errors).
- CI now enforces `cargo fmt --check` and `cargo clippy -D warnings`;
  the workspace is warning-clean.
- Added the `LICENSE` file (MIT — `package.json` already declared it).

### Notes
- `RESULT_SHAPE_VERSION` is unchanged (no returned shape changed).
- Corpus re-synced with CiteMe (`harvard-cite-them-right.csl` had
  drifted upstream).

## [0.3.6] — 2026-07-09

### Fixed
- **Distinct batch items sharing an id no longer collide.** `formatBatch`
  keyed its bibliography lookup by item id, so two *different* items that
  arrived with the same user-supplied `id` both rendered the first item's
  reference. Later occurrences of a duplicated id are now rewritten to a
  synthetic id before formatting. Identical duplicates keep their shared
  id — the same entry cited twice is one bibliography entry, and splitting
  it would trigger spurious year-suffix disambiguation (`2024a`/`2024b`);
  a regression test pins both behaviors.

### Changed
- `formatBatch` deserializes items from the parsed JSON tree directly
  instead of cloning each item's full `serde_json::Value` — one less
  full-tree clone per item on the hot path. No behavior change.

### Notes
- `RESULT_SHAPE_VERSION` is unchanged (no returned shape changed).
- `citeme-engine-core`'s crate version now tracks the release version
  (it had lagged at 0.3.4 through the 0.3.5 release); it is a path
  dependency, so nothing observable changes for consumers.

## [0.3.5] — 2026-06-12

### Changed
- **Formatting with an unloaded locale is now a loud error instead of
  silently wrong output.** `formatOne`/`formatBatch` (and the prose/output
  variants) return `EngineError::LocaleNotLoaded` → `JsError` when
  `locale_code` names a locale that was never passed to `loadLocale`.
  Previously hayagriva rendered with **zero locale terms** — numeric months,
  missing `and`/`n.d.` terms — producing exactly the
  `Brevet déposé le 3 2 2021publié le 10 5 2023` output that failed CiteMe's
  iso690-fr parity gate on 2026-06-12. Root-cause analysis showed the
  engine's rendering was correct all along *when the locale file is
  actually loaded* (`…le 3 février 2021 et publié le 10 mai 2023`); the
  broken output only reproduces with no matching locale available, so the
  silent degradation, not the renderer, was the defect. The empty string
  (`""`) remains the explicit escape hatch ("use the style's
  default-locale against whatever is loaded").
  **Migration:** load every locale you reference before formatting. CiteMe
  specifically: the parity-gate harness must `loadLocale` the 6 locales the
  adapter preloads, and `np405` (locale `pt-PT`) needs `pt-PT` added to
  `PRELOAD_LOCALES` — today it silently falls back to English terms.

### Added
- Parity regression tests pinning locale-term resolution: the patent
  fixture through `iso690-author-date-fr` × `fr-FR` must contain
  `3 février 2021` and ` et publié le ` (NBSP-normalized — the style joins
  date-parts with `&#160;`), and `plos` × `pt-BR` must render
  `10 de maio de 2023` / `3 de fevereiro de 2024`, proving long-form month
  terms are not iso690-local.

### Notes
- `RESULT_SHAPE_VERSION` is unchanged (errors are exceptions, not result
  JSON; no returned shape changed).
- Supersedes the 0.3.4 note that blamed hayagriva for dropped `and` terms
  and numeric months — hayagriva 0.9 renders both correctly given a loaded
  locale; no upstream patch needed.

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
