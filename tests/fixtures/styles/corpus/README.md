# Curated style corpus

Snapshot of every CSL style the CiteMe app serves in production
(`citeme/public/csl/**/*.csl`, flattened — basenames are unique).
Exercised by `crates/core/tests/corpus_smoke.rs`: every style must load and
format a battery of items in every supported locale without panicking, so a
style edge case fails the engine's CI instead of CiteMe's production.

To refresh after the app adds/changes styles:

    find <citeme-repo>/public/csl -name '*.csl' -exec cp {} tests/fixtures/styles/corpus/ \;

Last synced: 2026-06-12.
