#!/usr/bin/env bash
# Mechanical doc-link check (openspec 11-docs-refresh).
#
# Extracts every backticked code-file reference (*.rs, *.ts, *.tsx, *.js, *.jsx)
# from docs/*.md and AGENTS.md and checks that a file with that basename exists
# somewhere in the repo (frontend/src-tauri/src, frontend/src, frontend, scripts,
# or the repo root - i.e. anywhere outside the excluded build/vendor dirs).
#
# Path existence only: it does not verify a reference's directory, nor the
# symbols or claims around it. Backticked strings containing spaces or glob
# characters (e.g. `*.rs`, `{windows,macos}.rs` templates) are not treated as
# file references.
#
# Exit 0 when every reference resolves, 1 otherwise.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DOCS=(docs/*.md AGENTS.md)

# Basenames of every file in the repo, minus excluded directories.
INDEX="$(mktemp)"
trap 'rm -f "$INDEX"' EXIT
find . \
  \( -name node_modules -o -name target -o -name .git -o -name graphify-out \
     -o -name .next -o -name dist -o -name build \) -prune -o \
  -type f -print \
  | sed 's#.*/##' | sort -u > "$INDEX"

missing=0
for doc in "${DOCS[@]}"; do
  [ -f "$doc" ] || continue
  # `-o` prints each backticked token; strip the backticks, then the directory.
  while IFS= read -r ref; do
    [ -n "$ref" ] || continue
    base="${ref##*/}"
    if ! grep -qxF -- "$base" "$INDEX"; then
      echo "$doc: unresolved reference \`$ref\`"
      missing=$((missing + 1))
    fi
  done < <(grep -oE '`[^` *?{}]+\.(rs|ts|tsx|js|jsx)`' "$doc" | tr -d '`' | sort -u)
done

if [ "$missing" -gt 0 ]; then
  echo "check-doc-links: $missing unresolved reference(s)"
  exit 1
fi
echo "check-doc-links: all references resolve"
