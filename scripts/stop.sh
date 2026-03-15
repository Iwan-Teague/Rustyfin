#!/usr/bin/env bash
set -euo pipefail

RED='\033[0;31m'
CYAN='\033[0;36m'
RESET='\033[0m'

info() { echo -e "${CYAN}[stop]${RESET} $*"; }
die()  { echo -e "${RED}[stop] ERROR:${RESET} $*" >&2; exit 1; }

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  echo "Usage: ./scripts/stop.sh"
  echo
  echo "Native Debian runtime stop wrapper around ./scripts/stop-native.sh"
  exit 0
fi

[[ $# -eq 0 ]] || die "Legacy stop options are no longer supported. Use ./scripts/stop-native.sh directly if needed."

info "Delegating to native Debian runtime stop..."
exec "$REPO_ROOT/scripts/stop-native.sh"
