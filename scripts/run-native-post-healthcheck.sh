#!/usr/bin/env bash
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
RESET='\033[0m'

info()    { echo -e "${CYAN}[post-healthcheck]${RESET} $*"; }
success() { echo -e "${GREEN}[post-healthcheck]${RESET} $*"; }
warn()    { echo -e "${YELLOW}[post-healthcheck]${RESET} $*"; }
die()     { echo -e "${RED}[post-healthcheck] ERROR:${RESET} $*" >&2; exit 1; }

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

RUNTIME_ENV_FILE="$REPO_ROOT/.rustyfin.runtime.env"
if [[ -f "$RUNTIME_ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$RUNTIME_ENV_FILE" || true
fi

MAIN_SERVICE_NAME="${RUSTFIN_SYSTEMD_SERVICE:-rustyfin-native.service}"
AGENT_SERVICE_NAME="${RUSTFIN_SERVERS_AGENT_SERVICE:-rustfin-servers-agent.service}"
BACKEND_PORT="${RUSTFIN_BACKEND_PORT:-8096}"
CALENDAR_PORT="${RUSTFIN_CALENDAR_PORT:-8099}"
TMDB_PORT="${RUSTFIN_TMDB_AGENT_PORT:-8100}"
YOUTUBE_PORT="${RUSTFIN_YOUTUBE_AGENT_PORT:-8101}"
TRANSCRIPTION_PORT="${RUSTFIN_TRANSCRIPTION_AGENT_PORT:-8102}"
SERVERS_AGENT_PORT="${RUSTFIN_SERVERS_AGENT_PORT:-8103}"
UI_EDGE_PORT="${RUSTFIN_UI_PORT:-3000}"

wait_http_ok() {
  local name="$1"
  local url="$2"
  local insecure="${3:-0}"
  local attempts="${4:-60}"
  local curl_args=(-fsS)
  if [[ "$insecure" == "1" ]]; then
    curl_args+=(-k)
  fi
  for _ in $(seq 1 "$attempts"); do
    if curl "${curl_args[@]}" "$url" >/dev/null 2>&1; then
      info "${name} ready"
      return 0
    fi
    sleep 2
  done
  return 1
}

wait_ws_route() {
  local name="$1"
  local url="$2"
  local attempts="${3:-60}"
  local status=""
  for _ in $(seq 1 "$attempts"); do
    status="$(curl -sS -o /dev/null -w '%{http_code}' "$url" || true)"
    case "$status" in
      101|400|401|403|405|426)
        info "${name} route reachable (${status})"
        return 0
        ;;
    esac
    sleep 2
  done
  echo "${name} returned ${status:-unknown}" >&2
  return 1
}

check_systemd_active() {
  systemctl is-active --quiet "$MAIN_SERVICE_NAME" || return 1
  systemctl is-active --quiet "$AGENT_SERVICE_NAME" || return 1
}

run_checks() {
  check_systemd_active &&
  wait_http_ok "backend" "http://127.0.0.1:${BACKEND_PORT}/health" 0 60 &&
  wait_http_ok "calendar" "http://127.0.0.1:${CALENDAR_PORT}/health" 0 60 &&
  wait_http_ok "tmdb-agent" "http://127.0.0.1:${TMDB_PORT}/health" 0 60 &&
  wait_http_ok "youtube-agent" "http://127.0.0.1:${YOUTUBE_PORT}/health" 0 60 &&
  wait_http_ok "transcription-agent" "http://127.0.0.1:${TRANSCRIPTION_PORT}/health" 0 60 &&
  wait_http_ok "servers-agent" "http://127.0.0.1:${SERVERS_AGENT_PORT}/health" 0 60 &&
  wait_http_ok "edge-health" "https://127.0.0.1:${UI_EDGE_PORT}/health" 1 60 &&
  wait_http_ok "runtime-config" "https://127.0.0.1:${UI_EDGE_PORT}/runtime-config" 1 60 &&
  wait_http_ok "watch-party" "http://127.0.0.1:${BACKEND_PORT}/api/v1/watch-party/health" 0 60 &&
  wait_ws_route "channels websocket" "http://127.0.0.1:${BACKEND_PORT}/api/v1/channels/ws" 60
}

info "Running native post-start health checks..."
if run_checks; then
  success "Rustyfin native runtime passed post-start health checks."
  exit 0
fi

warn "Post-start health checks failed. Restarting native services once..."
systemctl restart "$AGENT_SERVICE_NAME"
systemctl restart "$MAIN_SERVICE_NAME"
sleep 5

if run_checks; then
  success "Rustyfin native runtime recovered after restart."
  exit 0
fi

die "Rustyfin native runtime failed post-start health checks after restart."
