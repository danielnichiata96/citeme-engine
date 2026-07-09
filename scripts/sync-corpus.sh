#!/usr/bin/env bash
# Sync/check the production style corpus against the CiteMe app's curated CSL set.
#
# The corpus (tests/fixtures/styles/corpus/) is what corpus_smoke.rs runs every
# style through in CI — it only protects production while it mirrors what the
# app actually serves from public/csl/. This script keeps the two in lockstep.
#
# Usage:
#   scripts/sync-corpus.sh [--write] [path-to-citeme-checkout]
#
#   default: check mode — reports drift (missing / stale / orphaned corpus
#            files) and exits 1 if any. Run before every release.
#   --write: copy changed/missing styles into the corpus and delete orphans.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CORPUS_DIR="$REPO_ROOT/tests/fixtures/styles/corpus"

WRITE=0
CITEME_DIR=""
for arg in "$@"; do
  case "$arg" in
    --write) WRITE=1 ;;
    *) CITEME_DIR="$arg" ;;
  esac
done
CITEME_DIR="${CITEME_DIR:-$REPO_ROOT/../citeme}"
CSL_SRC="$CITEME_DIR/public/csl"

if [[ ! -d "$CSL_SRC" ]]; then
  echo "error: CiteMe CSL dir not found: $CSL_SRC" >&2
  echo "       pass the citeme checkout path as an argument" >&2
  exit 2
fi

drift=0

# Basename collisions in the source would make the flat corpus ambiguous.
dupes=$(find "$CSL_SRC" -name '*.csl' -exec basename {} \; | sort | uniq -d)
if [[ -n "$dupes" ]]; then
  echo "error: duplicate CSL basenames in $CSL_SRC — flat corpus can't represent them:" >&2
  echo "$dupes" >&2
  exit 2
fi

declare -a expected=()
while IFS= read -r src; do
  base="$(basename "$src")"
  expected+=("$base")
  dst="$CORPUS_DIR/$base"
  if [[ ! -f "$dst" ]]; then
    echo "missing in corpus: $base"
    drift=1
    [[ $WRITE -eq 1 ]] && cp "$src" "$dst" && echo "  → copied"
  elif ! cmp -s "$src" "$dst"; then
    echo "stale in corpus:   $base"
    drift=1
    [[ $WRITE -eq 1 ]] && cp "$src" "$dst" && echo "  → updated"
  fi
done < <(find "$CSL_SRC" -name '*.csl' | sort)

for dst in "$CORPUS_DIR"/*.csl; do
  base="$(basename "$dst")"
  found=0
  for e in "${expected[@]}"; do [[ "$e" == "$base" ]] && found=1 && break; done
  if [[ $found -eq 0 ]]; then
    echo "orphan in corpus:  $base (app no longer serves it)"
    drift=1
    [[ $WRITE -eq 1 ]] && rm "$dst" && echo "  → removed"
  fi
done

if [[ $drift -eq 0 ]]; then
  echo "corpus in sync with $CSL_SRC (${#expected[@]} styles)"
elif [[ $WRITE -eq 1 ]]; then
  echo "corpus synced — review the diff and re-run 'cargo test -p citeme-engine-core --test corpus_smoke'"
else
  echo "corpus drift detected — run scripts/sync-corpus.sh --write" >&2
  exit 1
fi
