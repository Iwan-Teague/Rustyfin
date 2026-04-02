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
RUNTIME_ENV_FILE="$REPO_ROOT/.rustyfin.runtime.env"

if [[ -f "$RUNTIME_ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$RUNTIME_ENV_FILE" || true
fi

BACKEND_PORT="${RUSTFIN_BACKEND_PORT:-8096}"
UI_EDGE_PORT="${RUSTFIN_UI_PORT:-3000}"

cleanup() {
  "$REPO_ROOT/scripts/stop-native.sh" >/dev/null 2>&1 || true
}

pid_matches_service() {
  local service="$1"
  local pid="$2"
  local -a args=()
  local joined

  [[ -n "$pid" && "$pid" =~ ^[0-9]+$ && -r "/proc/${pid}/cmdline" ]] || return 1
  mapfile -d '' -t args < "/proc/${pid}/cmdline" 2>/dev/null || return 1
  [[ "${#args[@]}" -gt 0 ]] || return 1
  joined="$(printf '%s ' "${args[@]}")"

  case "$service" in
    rustfin) cmdline_has_executable rustfin-server "${args[@]}" ;;
    rustfin-calendar) cmdline_has_executable rustfin-calendar "${args[@]}" ;;
    rustfin-tmdb-agent) cmdline_has_executable rustfin-tmdb-agent "${args[@]}" ;;
    rustfin-youtube-agent) cmdline_has_executable rustfin-youtube-agent "${args[@]}" ;;
    rustfin-transcription-agent) cmdline_has_executable rustfin-transcription-agent "${args[@]}" ;;
    rustfin-servers-agent) cmdline_has_executable rustfin-servers-agent "${args[@]}" ;;
    rustfin-ui)
      if [[ "$joined" == *"next-server"* ]]; then
        return 0
      fi
      cmdline_has_executable node "${args[@]}" && [[ "$joined" == *"server.js"* ]]
      ;;
    rustfin-edge)
      cmdline_has_executable caddy "${args[@]}" && [[ "$joined" == *"Caddyfile.native"* ]]
      ;;
    *) return 1 ;;
  esac
}

cmdline_has_executable() {
  local expected="$1"
  shift
  local arg base
  for arg in "$@"; do
    [[ -n "$arg" ]] || continue
    base="$(basename -- "$arg")"
    if [[ "$arg" == "$expected" || "$base" == "$expected" ]]; then
      return 0
    fi
  done
  return 1
}

backend_health_ok() {
  curl -fsS "http://127.0.0.1:${BACKEND_PORT}/health" >/dev/null 2>&1
}

edge_health_ok() {
  curl -kfsS "https://127.0.0.1:${UI_EDGE_PORT}/health" >/dev/null 2>&1
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

if [[ -f "$RUNTIME_ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$RUNTIME_ENV_FILE" || true
fi
BACKEND_PORT="${RUSTFIN_BACKEND_PORT:-$BACKEND_PORT}"
UI_EDGE_PORT="${RUSTFIN_UI_PORT:-$UI_EDGE_PORT}"

while true; do
  for service in "${required_services[@]}"; do
    pidfile="$PID_DIR/${service}.pid"
    if [[ ! -f "$pidfile" ]]; then
      replacement_pid="$(find_service_pid "$service" || true)"
      if [[ -n "$replacement_pid" ]]; then
        printf '%s' "$replacement_pid" > "$pidfile"
      else
        die "Missing pid file for ${service}: ${pidfile}"
      fi
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
  if ! backend_health_ok; then
    die "Backend health check failed on http://127.0.0.1:${BACKEND_PORT}/health"
  fi
  if ! edge_health_ok; then
    die "Edge health check failed on https://127.0.0.1:${UI_EDGE_PORT}/health"
  fi
  sleep 2
done
