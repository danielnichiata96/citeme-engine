# Changelog

All notable changes to `citeme-engine` are documented here.

This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
and the [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format.

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
