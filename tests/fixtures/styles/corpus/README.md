# Curated style corpus

Snapshot of every CSL style the CiteMe app serves in production
(`citeme/public/csl/**/*.csl`, flattened — basenames are unique).
Exercised by `crates/core/tests/corpus_smoke.rs`: every style must load and
format a battery of items in every supported locale without panicking, so a
style edge case fails the engine's CI instead of CiteMe's production.

To check for drift (run before every release) or refresh:

    scripts/sync-corpus.sh [--write] [path-to-citeme-checkout]

Check mode exits 1 on missing/stale/orphaned styles; `--write` syncs and
removes orphans, then re-run `cargo test -p citeme-engine-core --test
corpus_smoke`.

Last synced: 2026-10-05 (in sync, 59 styles).
