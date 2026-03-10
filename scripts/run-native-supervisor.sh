#!/usr/bin/env bash
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
RESET='\033[0m'

info()    { echo -e "${CYAN}[run-native-supervisor]${RESET} $*"; }
success() { echo -e "${GREEN}[run-native-supervisor]${RESET} $*"; }
die()     { echo -e "${RED}[run-native-supervisor] ERROR:${RESET} $*" >&2; exit 1; }

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

SAFE_TMP_DIR="${RUSTFIN_TMPDIR:-$REPO_ROOT/.tmp}"
RUNTIME_ROOT="${RUSTFIN_NATIVE_RUNTIME_DIR:-$SAFE_TMP_DIR/native-runtime}"
PID_DIR="$RUNTIME_ROOT/pids"

cleanup() {
  "$REPO_ROOT/scripts/stop-native.sh" >/dev/null 2>&1 || true
}

trap cleanup EXIT INT TERM

required_services=(
  rustfin-tmdb-agent
  rustfin-youtube-agent
  rustfin-transcription-agent
  rustfin
  rustfin-calendar
  rustfin-ui
  rustfin-edge
)

export RUSTFIN_ENABLE_SERVERS_AGENT="${RUSTFIN_ENABLE_SERVERS_AGENT:-0}"

info "Starting Rustyfin native runtime under supervisor..."
"$REPO_ROOT/scripts/start-native.sh" --no-build
success "Rustyfin native runtime started. Monitoring child processes..."

while true; do
  for service in "${required_services[@]}"; do
    pidfile="$PID_DIR/${service}.pid"
    if [[ ! -f "$pidfile" ]]; then
      die "Missing pid file for ${service}: ${pidfile}"
    fi
    pid="$(cat "$pidfile" 2>/dev/null || true)"
    if [[ -z "$pid" || ! "$pid" =~ ^[0-9]+$ || ! -d "/proc/${pid}" ]]; then
      die "Service ${service} is not running (pid=${pid:-missing})"
    fi
  done
  sleep 2
done
