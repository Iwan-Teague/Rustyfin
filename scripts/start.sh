#!/usr/bin/env bash
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
RESET='\033[0m'

info()    { echo -e "${CYAN}[start]${RESET} $*"; }
success() { echo -e "${GREEN}[start]${RESET} $*"; }
warn()    { echo -e "${YELLOW}[start]${RESET} $*"; }
die()     { echo -e "${RED}[start] ERROR:${RESET} $*" >&2; exit 1; }

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

usage() {
  cat <<'USAGE'
Usage:
  ./scripts/start.sh [native start-native.sh options]

Native Debian 12 runtime entrypoint.
This is now a compatibility wrapper around ./scripts/start-native.sh.

Examples:
  ./scripts/start.sh
  ./scripts/start.sh --no-build
  ./scripts/start.sh --foreground
  ./scripts/start.sh --no-health-check
  ./scripts/start.sh --build-only
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exec "$REPO_ROOT/scripts/start-native.sh" --help
fi

forwarded_args=()
legacy_args=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --docker-rust-build|--docker-build|--docker|--compose)
      legacy_args+=("$1")
      shift
      ;;
    -f|--file|-p|--project-name)
      flag="$1"
      legacy_args+=("$flag")
      shift
      [[ $# -gt 0 ]] || die "${flag} requires a value"
      legacy_args+=("$1")
      shift
      ;;
    *)
      forwarded_args+=("$1")
      shift
      ;;
  esac
done

if [[ "${#legacy_args[@]}" -gt 0 ]]; then
  warn "Ignoring legacy Docker flags: ${legacy_args[*]}"
fi

info "Delegating to native Debian runtime..."
exec "$REPO_ROOT/scripts/start-native.sh" "${forwarded_args[@]}"
