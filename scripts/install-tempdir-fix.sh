#!/usr/bin/env bash
set -euo pipefail

# Persist a TMPDIR fallback in shell rc so docker/cargo/npm work reliably.
# Idempotent: safe to run multiple times.

TARGET_RC="${1:-$HOME/.zshrc}"
FALLBACK_TMP="${RUSTFIN_GLOBAL_TMPDIR:-$HOME/.tmp}"

mkdir -p "$FALLBACK_TMP"
chmod 700 "$FALLBACK_TMP" 2>/dev/null || true

if [[ ! -w "$FALLBACK_TMP" ]]; then
  echo "Error: fallback temp directory is not writable: $FALLBACK_TMP" >&2
  exit 1
fi

mkdir -p "$(dirname "$TARGET_RC")"
touch "$TARGET_RC"

BEGIN_MARKER="# >>> Rustyfin TMPDIR fix >>>"
END_MARKER="# <<< Rustyfin TMPDIR fix <<<"

if grep -Fq "$BEGIN_MARKER" "$TARGET_RC"; then
  echo "TMPDIR fix already present in: $TARGET_RC"
else
  cat >> "$TARGET_RC" <<EOF

$BEGIN_MARKER
# Keep TMPDIR usable for docker/cargo/npm if default macOS temp path is inaccessible.
if [ -z "\${TMPDIR:-}" ] || [ ! -d "\$TMPDIR" ] || [ ! -w "\$TMPDIR" ]; then
  export TMPDIR="$FALLBACK_TMP"
fi
$END_MARKER
EOF
  echo "Installed TMPDIR fix into: $TARGET_RC"
fi

echo "Done. Open a new terminal, or run:"
echo "  source \"$TARGET_RC\""
echo "Current TMPDIR fallback:"
echo "  $FALLBACK_TMP"
