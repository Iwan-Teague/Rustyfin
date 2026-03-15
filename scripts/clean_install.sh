#!/usr/bin/env bash
set -euo pipefail

RED='\033[0;31m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
RESET='\033[0m'

info() { echo -e "${CYAN}[clean-install]${RESET} $*"; }
warn() { echo -e "${YELLOW}[clean-install]${RESET} $*"; }
die()  { echo -e "${RED}[clean-install] ERROR:${RESET} $*" >&2; exit 1; }

usage() {
  cat <<'USAGE'
Usage:
  ./scripts/clean_install.sh [--yes]

Resets Rustyfin runtime/user data for the native Debian stack.
This keeps built artifacts in place but removes runtime state, cache, logs,
and database contents so the next startup goes through first-run setup again.
USAGE
}

ASSUME_YES=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --yes|-y) ASSUME_YES=true; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "Unknown argument: $1" ;;
  esac
done

if [[ "$ASSUME_YES" != "true" ]]; then
  echo
  warn "This will DELETE Rustyfin runtime/user data (PostgreSQL contents, cache, transcode, logs, runtime env)."
  warn "Built binaries and source code are kept. The next start will boot as a first-time install."
  echo
  read -r -p "Type 'yes' to continue: " confirm
  [[ "$confirm" == "yes" ]] || { info "Aborted."; exit 0; }
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

exec "$REPO_ROOT/scripts/rustfin-installer.sh" clean-native-runtime --yes
