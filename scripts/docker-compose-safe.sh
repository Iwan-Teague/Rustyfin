#!/usr/bin/env bash
set -euo pipefail

# Run docker compose with a known-writable TMPDIR.
# This avoids macOS temp directory permission issues in some environments.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

SAFE_TMP_DIR="${RUSTFIN_TMPDIR:-$REPO_ROOT/.tmp}"
mkdir -p "$SAFE_TMP_DIR"
chmod 700 "$SAFE_TMP_DIR" 2>/dev/null || true

if [[ ! -w "$SAFE_TMP_DIR" ]]; then
  echo "Error: temp directory is not writable: $SAFE_TMP_DIR" >&2
  exit 1
fi

export TMPDIR="$SAFE_TMP_DIR"

exec docker compose "$@"
