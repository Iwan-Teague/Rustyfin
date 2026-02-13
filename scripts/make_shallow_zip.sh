#!/usr/bin/env bash
set -euo pipefail

# Usage:
#   ./make_shallow_zip.sh            # archives HEAD to <repo>-<sha>.zip
#   ./make_shallow_zip.sh <ref>      # archives <ref> (e.g. main, HEAD, v1.2.3)
#   ./make_shallow_zip.sh <ref> <out.zip>

ref="${1:-HEAD}"
out="${2:-}"

root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "Error: not inside a git repository." >&2
  exit 1
}

repo_name="$(basename "$root")"
cd "$root"

if [[ -z "$out" ]]; then
  short_sha="$(git rev-parse --short "$ref")"
  out="${repo_name}-${short_sha}.zip"
fi

# --prefix puts everything under a single folder when unzipping
git archive --format=zip --prefix="${repo_name}/" -o "$out" "$ref"

echo "Created: $out"
