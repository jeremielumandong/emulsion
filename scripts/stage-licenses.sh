#!/usr/bin/env bash
# Copy the shared license manifest into an application package, preserving paths.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ $# != 1 || -z "$1" ]]; then
  echo "Usage: bash scripts/stage-licenses.sh <destination>" >&2
  exit 2
fi
DEST_DIR="$1"

# Validate the whole manifest first so missing notices fail packaging early.
while IFS= read -r file || [[ -n "$file" ]]; do
  [[ -z "$file" || "$file" == \#* ]] && continue
  if [[ ! -s "$ROOT_DIR/$file" ]]; then
    echo "Required license or notice is missing or empty: $file" >&2
    exit 1
  fi
done < "$ROOT_DIR/packaging/license-files.txt"

while IFS= read -r file || [[ -n "$file" ]]; do
  [[ -z "$file" || "$file" == \#* ]] && continue
  mkdir -p "$(dirname "$DEST_DIR/$file")"
  install -m 644 "$ROOT_DIR/$file" "$DEST_DIR/$file"
done < "$ROOT_DIR/packaging/license-files.txt"
