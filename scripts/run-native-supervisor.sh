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

pid_matches_service() {
  local service="$1"
  local pid="$2"
  local cmdline

  [[ -n "$pid" && "$pid" =~ ^[0-9]+$ && -r "/proc/${pid}/cmdline" ]] || return 1
  cmdline="$(tr '\000' ' ' < "/proc/${pid}/cmdline" 2>/dev/null || true)"
  [[ -n "$cmdline" ]] || return 1

  case "$service" in
    rustfin) [[ "$cmdline" == *"rustfin-server"* ]] ;;
    rustfin-calendar) [[ "$cmdline" == *"rustfin-calendar"* ]] ;;
    rustfin-tmdb-agent) [[ "$cmdline" == *"rustfin-tmdb-agent"* ]] ;;
    rustfin-youtube-agent) [[ "$cmdline" == *"rustfin-youtube-agent"* ]] ;;
    rustfin-transcription-agent) [[ "$cmdline" == *"rustfin-transcription-agent"* ]] ;;
    rustfin-servers-agent) [[ "$cmdline" == *"rustfin-servers-agent"* ]] ;;
    rustfin-ui) [[ "$cmdline" == *"next-server"* || ( "$cmdline" == *"node"* && "$cmdline" == *"server.js"* ) ]] ;;
    rustfin-edge) [[ "$cmdline" == *"caddy"* && "$cmdline" == *"Caddyfile.native"* ]] ;;
    *) return 1 ;;
  esac
}

find_service_pid() {
  local service="$1"
  local pid cmdline

  while read -r pid cmdline; do
    [[ -n "$pid" ]] || continue
    if pid_matches_service "$service" "$pid"; then
      printf '%s\n' "$pid"
      return 0
    fi
  done < <(ps -eo pid=,args=)

  return 1
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
    if ! pid_matches_service "$service" "$pid"; then
      replacement_pid="$(find_service_pid "$service" || true)"
      if [[ -n "$replacement_pid" ]]; then
        printf '%s' "$replacement_pid" > "$pidfile"
        pid="$replacement_pid"
      fi
    fi
    if ! pid_matches_service "$service" "$pid"; then
      die "Service ${service} is not running (pid=${pid:-missing})"
    fi
  done
  sleep 2
done
