#!/usr/bin/env bash
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
RESET='\033[0m'

info()    { echo -e "${CYAN}[stop-native]${RESET} $*"; }
success() { echo -e "${GREEN}[stop-native]${RESET} $*"; }
die()     { echo -e "${RED}[stop-native] ERROR:${RESET} $*" >&2; exit 1; }

usage() {
  echo "Usage: ./scripts/stop-native.sh"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    *) die "Unknown argument: $1" ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

SAFE_TMP_DIR="${RUSTFIN_TMPDIR:-$REPO_ROOT/.tmp}"
RUNTIME_ROOT="${RUSTFIN_NATIVE_RUNTIME_DIR:-$SAFE_TMP_DIR/native-runtime}"
PID_DIR="$RUNTIME_ROOT/pids"
RUNTIME_ENV_FILE="$REPO_ROOT/.rustyfin.runtime.env"

if [[ -f "$RUNTIME_ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$RUNTIME_ENV_FILE" || true
fi

stop_one() {
  local name="$1"
  local pidfile="$PID_DIR/${name}.pid"
  if [[ ! -f "$pidfile" ]]; then
    return
  fi

  local pid
  pid="$(cat "$pidfile" 2>/dev/null || true)"
  if [[ -n "$pid" ]] && [[ "$pid" =~ ^[0-9]+$ ]] && kill -0 "$pid" 2>/dev/null; then
    info "Stopping ${name} (pid ${pid})..."
    kill "$pid" 2>/dev/null || true
    for _ in $(seq 1 20); do
      if ! kill -0 "$pid" 2>/dev/null; then
        break
      fi
      sleep 0.2
    done
    if kill -0 "$pid" 2>/dev/null; then
      kill -9 "$pid" 2>/dev/null || true
    fi
  fi
  rm -f "$pidfile"
}

for service in rustfin-edge rustfin-ui rustfin-calendar rustfin rustfin-servers-agent rustfin-transcription-agent rustfin-youtube-agent rustfin-tmdb-agent; do
  stop_one "$service"
done

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

success "Rustyfin native runtime stopped."
