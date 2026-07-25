# Third-party notices

The source code in this repository is MIT licensed (see [LICENSE](LICENSE)).
The files below are **not** — they are third-party assets redistributed here
under their own terms.

## CSL styles and locales (test fixtures)

- `tests/fixtures/styles/corpus/*.csl` — 60 citation styles
- `tests/fixtures/locales/locales-*.xml` — 7 locale files

These come from the [Citation Style Language](https://citationstyles.org)
project ([styles](https://github.com/citation-style-language/styles),
[locales](https://github.com/citation-style-language/locales)) and are
licensed under
[Creative Commons Attribution-ShareAlike 3.0 Unported](https://creativecommons.org/licenses/by-sa/3.0/).
Each file carries its own `<rights>` element naming its authors.

They are vendored solely as CI fixtures: `crates/core/tests/corpus_smoke.rs`
loads every one of them and asserts that formatting never panics. They are
not compiled into the Wasm binary and are not part of the published npm
package.

If you redistribute these files, CC BY-SA 3.0 applies to them — attribution
plus share-alike — independently of the MIT license on the code.

## Rust dependencies

The engine builds on [Hayagriva](https://github.com/typst/hayagriva) and
[biblatex](https://github.com/typst/biblatex) (both MIT OR Apache-2.0), which
do the CSL rendering and BibTeX tokenizing respectively. Run
`cargo tree --workspace` for the full dependency graph; every transitive
dependency is permissively licensed.
