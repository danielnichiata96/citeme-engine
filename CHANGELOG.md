# Changelog

All notable changes to `citeme-engine` are documented here.

This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
and the [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format.

## [0.4.0] — 2026-10-05

A review of the whole engine. Each item below is pinned by a test that failed
before the fix. Several inputs aborted the Wasm instance, which is the one
failure a consumer can't catch. Almost everything else produced wrong data
without raising an error. The result-JSON shapes are unchanged
(`resultShapeVersion()` is still `1`), and no method was added or removed.
The binary grew from 544 to 574 KB gzip, almost all of it the BibTeX
pre-scan. The changes a consumer can see are listed under *Changed*.

### Fixed — inputs that aborted the Wasm instance
- **A CSL-JSON date range panicked inside Hayagriva** ("ranges are not
  supported"). This happened for `"issued": {"date-parts": [[2019], [2020]]}`
  and for `"raw": "2019/2020"`, whenever the style rendered or sorted on that
  date, which APA does for every reference. It is now an `unsupported CSL-JSON
  input` error (see *Changed*). A range whose end repeats its start is read as
  one date.
- **An empty CSL-JSON date panicked in citationberg.** `[[]]` and `[[""]]` hit
  an `Option::unwrap`. They now mean no date.
- **Hayagriva panicked when a value opened with punctuation after a style
  prefix ending in a space,** for example `"pp.&#160;"` followed by `".é"`.
  It measured the prefix before trimming that space, then sliced the text at
  the stale length, which landed mid-character. The same bug read a short
  value as empty and dropped it: `".e"` printed nothing. This is fixed in a
  vendored Hayagriva 0.9.1 (`vendor/`, documented in `vendor/README.md`).
  hayagriva 0.10.1 still has the bug. The realistic trigger is surrounding
  whitespace (`" Études"`), and every value is now trimmed as well.
- **ABNT post-processing panicked** when an in-text citation had a `)` before
  its first `(`. This affected any style with `isAbnt` on.
- **BibTeX import could abort or hang the engine.**
  - A pasted entry reached an `unwrap` in biblatex's date parser:
    `month = {may 300}`, `year = {- 2020}`, `date = {²²}`, `month = {0}`.
  - Some entries recursed until the stack overflowed: a `crossref`, `xdata`
    or `@string` that reaches itself, or about 5,000 nested `\emph{`.
  - Others went quadratic or exponential: 40,000 inner braces took 7 s, a
    2,000-entry crossref chain took 8 s, and `a = b # b` doubles per level.
    The pre-scan itself had to be kept linear too: 20,000 copies of one key
    or one entry with 160,000 fields each took about 20 s.

  A linear pre-scan (`parsers/bibtex_guard.rs`) now drops the offending field
  or `@string` before biblatex reads the input. It keeps the entry and says
  so in `errors` (see *Changed*).

### Fixed — formatting
- **Every arXiv paper CiteMe formatted failed.** CiteMe's item carries the
  arXiv id in a `custom` extension object. Hayagriva's CSL-JSON reader
  rejected the whole item over it, and over any `null`, boolean, float or
  keyword list. `formatOne`/`formatBatch` threw and the app fell back to
  citation-js, for the whole batch if one paper came from arXiv. Values
  Hayagriva doesn't read are now dropped before rendering.
- **The same item cited twice in a batch got two numbers.** In IEEE,
  Vancouver and ABNT numérico it was `[1]` and then `[3]`, against a single
  bibliography entry. In APA the repeat lost its year suffix:
  `(J. Smith, 2020)` beside `(Smith, 2020a)`. Hayagriva tells entries apart by
  address, so identical items are now one entry. That also covers items with
  no `id`, which were disambiguated against themselves (`2024a`/`2024b`).
  Synthetic ids never take an id an item already has.
- **The date normalizer added spacing artifacts to six production styles:**
  - `( March 2018)` in McGill;
  - `Science.  2000;287` in every PLOS journal reference, and `[cited  2018]`;
  - `Published Online First:  2018` in BMJ;
  - `March  15,  2018` in ACS;
  - `consulté le  mai 2024` in Bordeaux.

  Rewriting `delimiter` as per-part prefixes is only equivalent for dates
  that start with the year, so it now applies only to those. A corpus test
  renders every production style with and without normalization.
- **ABNT:**
  - `et al.` stays lower case: `(SILVA et al., 2024)` instead of
    `(SILVA ET AL., 2024)`.
  - Entries led by a title (no author, editor, translator or organizer) are no
    longer uppercased as if the title were a family name. Before, the output
    was `PESQUISA NACIONAL POR AMOSTRA DE DOMICÍLIOS, síntese de indicadores`
    and `(MANUAL DE REDAÇÃO, 2010)`.
  - Markup in an in-text citation is no longer uppercased or split at the `;`
    in its CSS.
- **Out-of-range months and days printed as garbage:** month 0 came out as
  `(2019, 256 3)` and day 40 as `May 40`. They are now dropped, as the
  exporters already did.
- **A `raw` date is read only year-first, with a four-digit year,** as the
  exporters read it. `"raw": "03-05-2019"` rendered year 3.
- **A whitespace-only value now counts as absent.** `"volume": " "` used to
  print the style's empty labels (`2020., .`).

### Fixed — import
- **CSL-JSON import rejected valid items** that had a keyword list, a `null`,
  a literal date or an extension object. That included CiteMe's own CSL-JSON
  export of an arXiv paper and this engine's BibTeX import of one. Items are
  now validated the way the formatter reads them, and only malformed ones are
  errors: a name that is a string, or a date part that isn't a number. Accepted
  items keep the value types the old check guaranteed: no `null`s or booleans,
  keyword lists in the CSL string form, no null names, and numbers as integers
  or text. A leading byte-order mark is skipped.
- **Duplicate keys, broken entries and one-line pastes no longer cost other
  entries.** A duplicate key or one broken entry used to force a per-chunk
  recovery that:
  - skipped `@string` definitions, so every macro user failed;
  - split only on "\n@", so CrossRef one-liners were lost whole and
    `max_entries` was exceeded;
  - could give the second entry of a duplicated key its twin's type and
    journal.

  The file is now parsed once and each entry is converted on its own. An
  entry hayagriva can't convert costs only that entry.
- **BibTeX fields import as written.**
  - A numeric month (`month = {5}`, the form biblatex documents) was dropped,
    and so was a `day` with it.
  - A date range imported its end; it now imports its start.
  - Page lists lost their commas ("100-115200").
  - `and others` became an author named "others".
  - `\emph{…}`, `\textsubscript{…}` and `{\it …}` stayed in titles.
  - `\o{}` became a combining stroke ("J̸rgensen").
  - `\url{…}` in `howpublished` never became the URL.
  - `year = {in press}` rejected the entry. A year written as words now
    imports as CSL `status`, which APA prints as "(in press)".
  - `urldate = {15/03/2024}` was read as year 15. Day- or month-first dates
    are now read only when the order is unambiguous.
  - An undefined `@string` used inside another `@string` is kept as text,
    as it already was in a field. Before, every such definition cost a
    whole-file re-parse, and past eight the import read entries one by one:
    some were dropped, and all lost their `crossref` inheritance.
- **BibTeX import shifted every month and day back by one.** hayagriva's
  `Date` is zero-based and was copied into one-based CSL date-parts:
  `month = may` imported as April, and January as month 0, so APA rendered
  "(2019, 256)". The bug was there since the parser was written. biblatex
  `type` keys (`phdthesis`, `mathesis`, …) now import as readable genres
  instead of printing "[Phdthesis]".
- **MEDLINE: wrapped lines.** PubMed wraps at about 80 columns with a
  six-space indent.
  - A wrapped line opening with "HIV-1", "IL-6" or "SARS-CoV-2" was read as a
    tag and cut the title short.
  - A wrapped journal title replaced the journal with its last fragment, so
    PNAS imported as "America".
  - A wrapped line whose indent a paste stripped now continues its field.
- **MEDLINE: pre-2002 records and record boundaries.**
  - Pre-2002 records carry only `AU` ("Smith JA") and imported with no
    authors. `AU` is now read when `FAU` is absent, and `AU` keeps a "Jr"
    suffix.
  - `CN` (corporate author) is kept in order.
  - A second `PMID` starts a new record instead of merging two records whose
    blank separator was lost.
  - `DP` keeps the day and the first month of a range.
  - Continuation is relative to the tag indent, so indented pastes parse. A
    record indented deeper than the one before it is still read; a regression
    in this pass, caught before release, had dropped it.
  - `PMC` imports as `PMCID`, and a leading byte-order mark is skipped.
- **RIS: `Y2` is the access date** (as Zotero and EndNote write it, for every
  type), never the publication date. It used to overwrite `DA`, so a 2019
  article imported as 2024.
- **RIS: dates.** A day/month-first `DA` (`05/12/2019`) no longer imports as
  year 5. Only a four-digit year is a year, and `PY` wins when `DA` is
  ambiguous. ISO dates and full `PY` dates are read.
- **RIS: pages and journal titles.** A lone `SP` (article numbers, whole
  ranges) keeps its page. Full journal titles (`T2`/`JF`/`BT`) beat
  abbreviations (`JO`/`JA`/`J2`, now `container-title-short`) whatever their
  order.
- **RIS: record structure.**
  - A missing `ER` no longer discards the record.
  - A leading byte-order mark no longer drops the first record.
  - Wrapped lines continue their title, abstract or journal instead of
    disappearing; an untagged line under `KW` is another keyword.
  - The truncation path no longer leaks the internal `_sp`/`_ep` keys.
- **RIS: names and editors.** `ED` (and `A2` on chapters and proceedings)
  become editors, with linear de-duplication (20k pairs took about 6 s).
  "Last, First, Jr." keeps its suffix.
- **RIS: other fields.**
  - `BT` is the title of a `BOOK`.
  - `SN` values that are ISSNs are filed as `ISSN`, not `ISBN`, including
    ones with a qualifier or no hyphen.
  - `ET` imports as `edition` and `TY  - PAT` as `patent`.
- **Format detection:** an "@WHO (" or "@CDCgov (" mention in a RIS or
  MEDLINE abstract made the whole file BibTeX, so nothing imported. The first
  line that opens a record now decides the format, and detection sees past a
  byte-order mark.

### Fixed — export
- **BibTeX/BibLaTeX export escapes every field.** Volume, number, pages,
  ISBN/ISSN, PMID/PMCID, chapter and eprint were written raw, so a `}` in any
  of them lost the whole entry on re-import, and `2_suppl` broke LaTeX.
  Verbatim fields (`doi`, `url`, `eprint`) percent-encode braces.
- **Cite keys never collide.** A repeated id took `a`/`b` suffixes blindly:
  `["smith2024", "smith2024a", "smith2024"]` exported two `smith2024a`, which
  BibTeX rejects. Suffixes now skip every key in the file, and keys are
  compared case-insensitively, as BibTeX compares them. A numeric `id` keeps
  its identity as a key.
- **BibTeX name parts with a comma or the word "and" are braced.** "Procter
  and Gamble" re-imported as two authors.
- **All exporters read CSL "string or number" values.** Numeric
  `volume`/`issue`/`page`/`edition`/`PMID`/`ISBN`, numeric-string date-parts
  (`[["2019", "5"]]`, as Zotero and citation-js emit them) and `raw` ISO dates
  were dropped. Out-of-range months and days, and days a month doesn't have,
  are no longer written. Ambiguous `raw` dates like "05/03/2019" are not
  guessed.
- **Exports carry the access date, event, version and number:**
  - BibLaTeX: `urldate`, `eventtitle`, `version`;
  - BibTeX: `urldate`;
  - RIS: `Y2`, editors as `ED`/`A2`, the number as `M1`/`C7`;
  - Hayagriva: the number as `serial-number.serial`.

  An article's CSL `number` goes to BibTeX/BibLaTeX `eid`, since `number` is
  the issue there, and RIS uses `C7` only for article types. The importers
  read all of these back into CSL `number`. A report's or a patent's BibTeX
  `number` also stays in `issue`, for readers that only know `issue`.
- **Theses and reports export their institution** as BibTeX
  `school`/`institution` and BibLaTeX `institution`, plus the BibLaTeX thesis
  `type` (the genre as written). Standard styles ignore `publisher` there.
- **Hayagriva YAML always loads, and loads the right names.**
  - Persons are written in dictionary form. The "Given Family" strings were
    read back as different people.
  - Years are four digits (`0800-03`, `-0350`).
  - Impossible days and URLs hayagriva can't parse are left out.
  - U+FFFE/U+FFFF are escaped.

  Before, one such value in one item failed the whole file. Key
  de-duplication is now linear: 16,000 items went from 15 s to 19 ms.
- **RIS:** patents are `PAT`, and a keyword-list item containing a comma
  stays one keyword. **BibLaTeX:** BC years round-trip (`date = {-0350}`).

### Changed
- **Formatting valid CSL-JSON the renderer can't represent is an error.** A
  date range, or a date given only as `literal` text, is refused as
  `unsupported CSL-JSON input: <variable>: …` (Rust:
  `EngineError::UnsupportedCslJson`). Before, a range aborted the instance and
  a literal date was an opaque deserialization error. In a batch, the error
  names the item by position and fails the call, as other invalid items
  already did. Format the start date alone if that's what you want.
- **Malformed CSL-JSON that rendered silently is now an `invalid CSL-JSON
  input` error.** This covers a name list that is a string (the author was
  silently dropped) and a `date-parts` with more than a start and an end (the
  date was silently dropped).
- **BibTeX `errors` can describe an entry that was imported.** When the
  pre-scan drops a field or an `@string`, it reports it with a message ending
  "imported without it", "kept as text" or "its uses read as empty". These
  inputs used to abort the engine or fail the entry. An undefined `@string`
  used to fail its entry; the entry now imports with the macro's name as the
  text. `scannedEntries` counts every entry found, readable or not.
- **Output changes CiteMe will see** are all fixes listed above: arXiv papers
  format through the engine instead of falling back; ABNT `et al.` is lower
  case and title-led entries keep their case; six styles lose spacing
  artifacts; repeated items keep one number.
- **BibTeX "von" parts import as `non-dropping-particle`**, matching BibTeX
  semantics, citation-js and CiteMe's own name parser. APA now renders
  "van der Berg, J. (2000)" / "(van der Berg, 2000)" instead of
  "Berg, J. van der. (2000)" / "(Berg, 2000)", and the style's
  `demote-non-dropping-particle` decides how it displays. A particle-only name
  ("von Neumann") is kept as a person instead of a literal that loses its
  "von".

### Dependencies
- **Hayagriva stays at 0.9.1**, vendored with the fix above. 0.10.1 was
  evaluated by rendering the 59 production styles before and after, and was
  not taken:
  - its ICU collation data doubles the binary (540 → 1015 KB gzip);
  - it drops the given name and the year suffix in APA-family
    disambiguation: `Smith. (2020)` instead of `Smith, J. (2020a)`;
  - it truncates AGLC page ranges (`100–115` → `100`);
  - it turns on subsequent-author substitution, which can't be right for
    results returned one item at a time.

  Its real fixes are noted in `vendor/README.md` for the next attempt.
- **wasm-bindgen 0.2.114 → 0.2.129.** The CLI must match the crate exactly.
  `build.sh` now checks this before building, and CI reads the version from
  `Cargo.lock` instead of pinning it by hand. The generated init code is
  unchanged.
- Compatible updates across `Cargo.lock` (serde 1.0.229, serde_json 1.0.151,
  thiserror 2.0.21, proptest 1.11, …). criterion 0.5 → 0.8 (benches only).
- **CI:** `actions/checkout` and `actions/setup-node` v4 → v7, and Node 22 →
  24, which is CiteMe's runtime. The workspace is fmt- and clippy-clean on
  Rust 1.99, which is what CI's floating `stable` now installs.

### Notes
- New guards:
  - `crates/core/tests/format_input.rs`;
  - `normalize_corpus.rs` (the raw-versus-normalized corpus render);
  - CSL-JSON, ABNT and tagged-format property tests;
  - `js/test/no-abort.test.mjs`, which runs the release binary, where a panic
    really is an abort.

  Run the property tests with a high `PROPTEST_CASES` before a release; the
  default 256 cases missed the punctuation panic.
- The structured fuzzing behind this release pushed 2.4M random items through
  all 59 production styles with no panic. That harness is not part of the
  repository.

## [0.3.8] — 2026-07-25

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
