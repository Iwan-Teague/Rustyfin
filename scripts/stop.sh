#!/usr/bin/env bash
set -euo pipefail

# Stop and remove the Rustyfin Docker stack (containers + network).
# Does not remove persistent volumes.

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
RESET='\033[0m'

info()    { echo -e "${CYAN}[stop]${RESET} $*"; }
success() { echo -e "${GREEN}[stop]${RESET} $*"; }
die()     { echo -e "${RED}[stop] ERROR:${RESET} $*" >&2; exit 1; }

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
COMPOSE_FILE="$REPO_ROOT/docker-compose.yml"

usage() {
  echo "Usage: ./scripts/stop.sh [-f <compose-file>]"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -f|--file)
      [[ $# -ge 2 ]] || die "Missing value for $1"
      COMPOSE_FILE="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "Unknown argument: $1"
      ;;
  esac
done

if [[ "$COMPOSE_FILE" != /* ]]; then
  COMPOSE_FILE="$REPO_ROOT/$COMPOSE_FILE"
fi

cd "$REPO_ROOT"

[[ -f "$COMPOSE_FILE" ]] || die "docker-compose.yml not found at $COMPOSE_FILE"
command -v docker >/dev/null 2>&1 || die "docker is not installed or not in PATH"
docker compose version >/dev/null 2>&1 || die "docker compose is not available"

SAFE_TMP_DIR="${RUSTFIN_TMPDIR:-$REPO_ROOT/.tmp}"
mkdir -p "$SAFE_TMP_DIR" || die "Failed to create temp dir: $SAFE_TMP_DIR"
chmod 700 "$SAFE_TMP_DIR" 2>/dev/null || true
[[ -w "$SAFE_TMP_DIR" ]] || die "Temp dir is not writable: $SAFE_TMP_DIR"
export TMPDIR="$SAFE_TMP_DIR"

info "Stopping Rustyfin stack..."
docker compose -f "$COMPOSE_FILE" down --remove-orphans

PICKER_HELPER_PID_FILE="$SAFE_TMP_DIR/directory-picker-helper.pid"
if [[ -f "$PICKER_HELPER_PID_FILE" ]]; then
  helper_pid="$(cat "$PICKER_HELPER_PID_FILE" 2>/dev/null || true)"
  if [[ -n "${helper_pid:-}" ]] && kill -0 "$helper_pid" 2>/dev/null; then
    info "Stopping directory picker helper (pid $helper_pid)..."
    kill "$helper_pid" 2>/dev/null || true
  fi
  rm -f "$PICKER_HELPER_PID_FILE"
fi

PICKER_HELPER_PORT="${RUSTFIN_PICKER_HELPER_PORT:-43110}"
if command -v lsof >/dev/null 2>&1; then
  helper_pids="$(lsof -ti tcp:${PICKER_HELPER_PORT} -sTCP:LISTEN 2>/dev/null || true)"
  if [[ -n "$helper_pids" ]]; then
    info "Stopping picker helper listener(s) on port ${PICKER_HELPER_PORT}..."
    for pid in $helper_pids; do
      kill "$pid" 2>/dev/null || true
    done
  fi
fi

success "Rustyfin stack stopped."
