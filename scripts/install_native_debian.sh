#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "[deprecated] install_native_debian.sh now delegates to install_linux.sh" >&2
exec "$SCRIPT_DIR/install_linux.sh" "$@"
