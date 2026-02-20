#!/usr/bin/env bash
set -euo pipefail

# Usage:
#   ./make_shallow_zip.sh            # archives HEAD to <repo>-<sha>.zip
#   ./make_shallow_zip.sh <ref>      # archives <ref> (e.g. main, HEAD, v1.2.3)
#   ./make_shallow_zip.sh <ref> <out.zip>
# Env:
#   SHALLOW_ZIP_EXTRA_EXCLUDES="path1,path2"  # optional comma-separated extra excludes

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

if [[ "$out" = /* ]]; then
  out_path="$out"
else
  out_path="${root}/${out}"
fi

mkdir -p "$(dirname "$out_path")"

stage_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$stage_dir"
}
trap cleanup EXIT

# --prefix puts everything under a single folder when unzipping
git archive --format=tar --prefix="${repo_name}/" "$ref" | tar -xf - -C "$stage_dir"

# Keep export lightweight by removing local/test/bootstrap artifacts if present.
exclude_paths=(
  ".tmp"
  ".npm-cache"
  ".playwright-browsers"
  "node_modules"
  "ui/node_modules"
  "ui/.next"
  "tests/node_modules"
  "tests/_runs"
  "scripts/rustfin.db"
  "scripts/rustfin.db-shm"
  "scripts/rustfin.db-wal"
)

if [[ -n "${SHALLOW_ZIP_EXTRA_EXCLUDES:-}" ]]; then
  IFS=',' read -r -a extra_excludes <<< "${SHALLOW_ZIP_EXTRA_EXCLUDES}"
  exclude_paths+=("${extra_excludes[@]}")
fi

for rel in "${exclude_paths[@]}"; do
  [[ -z "$rel" ]] && continue
  rm -rf "${stage_dir}/${repo_name}/${rel}"
done

(
  cd "$stage_dir"
  zip -rq "$out_path" "$repo_name"
)

echo "Created: $out_path"
