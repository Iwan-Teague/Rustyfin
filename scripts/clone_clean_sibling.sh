#!/usr/bin/env bash
set -euo pipefail

# Clone a clean sibling copy of the current repo into the parent directory.
# Example from /Users/me/Desktop/Rustyfin -> /Users/me/Desktop/rustyfin_1

usage() {
  cat <<'EOF'
Usage:
  ./scripts/clone_clean_sibling.sh [base_name]

Examples:
  ./scripts/clone_clean_sibling.sh
  ./scripts/clone_clean_sibling.sh rustyfin

Notes:
  - Clones from the current repo's "origin" remote.
  - Creates a sibling folder with an incrementing suffix: <base_name>_1, _2, ...
  - Uses a shallow clone by default (--depth 1). Set CLONE_DEPTH=0 for full history.
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if [[ "$#" -gt 1 ]]; then
  echo "Error: expected at most one argument (base_name)." >&2
  usage
  exit 1
fi

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -z "$repo_root" ]]; then
  echo "Error: this script must be run from inside a git repository." >&2
  exit 1
fi

origin_url="$(git -C "$repo_root" remote get-url origin 2>/dev/null || true)"
if [[ -z "$origin_url" ]]; then
  echo "Error: no 'origin' remote found for repository: $repo_root" >&2
  exit 1
fi

default_base="$(basename "$repo_root" | tr '[:upper:]' '[:lower:]')"
base_name="${1:-$default_base}"
base_name="${base_name// /_}"

if [[ -z "$base_name" ]]; then
  echo "Error: base_name resolved to empty value." >&2
  exit 1
fi

parent_dir="$(dirname "$repo_root")"
counter=1
while [[ -e "${parent_dir}/${base_name}_${counter}" ]]; do
  counter=$((counter + 1))
done
target_dir="${parent_dir}/${base_name}_${counter}"

clone_depth="${CLONE_DEPTH:-1}"
clone_args=()
if [[ "$clone_depth" != "0" ]]; then
  clone_args+=(--depth "$clone_depth")
fi

echo "Repository root : $repo_root"
echo "Remote (origin) : $origin_url"
echo "Clone target    : $target_dir"
echo "Clone depth     : ${clone_depth}"

git clone "${clone_args[@]}" "$origin_url" "$target_dir"

echo
echo "Done. Clean clone created at:"
echo "  $target_dir"
