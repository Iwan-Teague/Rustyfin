#!/usr/bin/env bash
set -euo pipefail

# Start the full Rustyfin Docker stack in a fresh clone or existing workspace.
# Safe defaults:
# - uses a writable repo-local TMPDIR (fixes macOS temp permission issues)
# - auto-creates a local media directory if none is provided
# - can auto-pick free host ports when defaults are occupied

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
RESET='\033[0m'

info()    { echo -e "${CYAN}[start]${RESET} $*"; }
success() { echo -e "${GREEN}[start]${RESET} $*"; }
warn()    { echo -e "${YELLOW}[start]${RESET} $*"; }
die()     { echo -e "${RED}[start] ERROR:${RESET} $*" >&2; exit 1; }

usage() {
  cat <<'EOF'
Usage:
  ./scripts/start.sh [--no-build] [--foreground] [--no-health-check] [-f <compose-file>]

Options:
  --no-build         Skip image rebuild step.
  --foreground       Run compose in foreground (default is detached).
  --no-health-check  Skip backend health wait loop.
  -f, --file         Compose file path (default: docker-compose.yml).
  -h, --help         Show this help.
EOF
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

BUILD=true
DETACH=true
HEALTH_CHECK=true
COMPOSE_FILE="$REPO_ROOT/docker-compose.yml"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-build) BUILD=false; shift ;;
    --foreground) DETACH=false; shift ;;
    --no-health-check) HEALTH_CHECK=false; shift ;;
    -f|--file)
      [[ $# -ge 2 ]] || die "Missing value for $1"
      COMPOSE_FILE="$2"
      shift 2
      ;;
    -h|--help) usage; exit 0 ;;
    *) die "Unknown argument: $1" ;;
  esac
done

if [[ "$COMPOSE_FILE" != /* ]]; then
  COMPOSE_FILE="$REPO_ROOT/$COMPOSE_FILE"
fi

cd "$REPO_ROOT"

[[ -f "$COMPOSE_FILE" ]] || die "docker-compose.yml not found at $COMPOSE_FILE"
command -v docker >/dev/null 2>&1 || die "docker is not installed or not in PATH"
docker compose version >/dev/null 2>&1 || die "docker compose is not available"

RUNTIME_ENV_FILE="$REPO_ROOT/.rustyfin.runtime.env"

SAFE_TMP_DIR="${RUSTFIN_TMPDIR:-$REPO_ROOT/.tmp}"
mkdir -p "$SAFE_TMP_DIR" || die "Failed to create temp dir: $SAFE_TMP_DIR"
chmod 700 "$SAFE_TMP_DIR" 2>/dev/null || true
[[ -w "$SAFE_TMP_DIR" ]] || die "Temp dir is not writable: $SAFE_TMP_DIR"
export TMPDIR="$SAFE_TMP_DIR"

# Load prior runtime settings so repeated runs stay stable.
user_backend_port="${RUSTFIN_BACKEND_PORT:-}"
user_ui_port="${RUSTFIN_UI_PORT:-}"
user_media_path="${RUSTFIN_MEDIA_PATH:-}"

if [[ -f "$RUNTIME_ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$RUNTIME_ENV_FILE"
fi

# Explicit shell/env values always win over runtime file values.
[[ -n "$user_backend_port" ]] && RUSTFIN_BACKEND_PORT="$user_backend_port"
[[ -n "$user_ui_port" ]] && RUSTFIN_UI_PORT="$user_ui_port"
[[ -n "$user_media_path" ]] && RUSTFIN_MEDIA_PATH="$user_media_path"

backend_locked=false
ui_locked=false
[[ -n "$user_backend_port" ]] && backend_locked=true
[[ -n "$user_ui_port" ]] && ui_locked=true

# Default media path for first-time setup on any machine.
MEDIA_PATH="${RUSTFIN_MEDIA_PATH:-$REPO_ROOT/media}"
mkdir -p "$MEDIA_PATH" || die "Failed to create media path: $MEDIA_PATH"
export RUSTFIN_MEDIA_PATH="$MEDIA_PATH"

is_port_in_use() {
  local port="$1"
  if command -v lsof >/dev/null 2>&1; then
    lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1
  else
    return 1
  fi
}

pick_free_port() {
  local preferred="$1"
  local max_hops="${2:-200}"
  local p="$preferred"
  local hops=0
  while is_port_in_use "$p"; do
    p=$((p + 1))
    hops=$((hops + 1))
    if (( hops > max_hops )); then
      die "Unable to find a free port near $preferred"
    fi
  done
  echo "$p"
}

project_running=false
if docker compose -f "$COMPOSE_FILE" ps --status running -q 2>/dev/null | grep -q .; then
  project_running=true
fi

backend_port="${RUSTFIN_BACKEND_PORT:-8096}"
ui_port="${RUSTFIN_UI_PORT:-3000}"

# If stack is not currently running, choose free ports unless user explicitly
# locked the ports via environment variables.
if [[ "$backend_locked" == "false" && "$project_running" == "false" ]]; then
  backend_selected="$(pick_free_port "$backend_port")"
  if [[ "$backend_selected" != "$backend_port" ]]; then
    warn "Port $backend_port is busy; using backend port $backend_selected"
  fi
  backend_port="$backend_selected"
fi

if [[ "$ui_locked" == "false" && "$project_running" == "false" ]]; then
  ui_selected="$(pick_free_port "$ui_port")"
  if [[ "$ui_selected" != "$ui_port" ]]; then
    warn "Port $ui_port is busy; using UI port $ui_selected"
  fi
  ui_port="$ui_selected"
fi

export RUSTFIN_BACKEND_PORT="$backend_port"
export RUSTFIN_UI_PORT="$ui_port"

info "Using TMPDIR: $TMPDIR"
info "Using media path: $RUSTFIN_MEDIA_PATH"
info "Backend port: $RUSTFIN_BACKEND_PORT"
info "UI port: $RUSTFIN_UI_PORT"

compose_args=(up --remove-orphans)
if [[ "$DETACH" == "true" ]]; then
  compose_args+=(-d)
fi
if [[ "$BUILD" == "true" ]]; then
  compose_args+=(--build)
fi

docker compose -f "$COMPOSE_FILE" "${compose_args[@]}"

{
  echo "# Generated by scripts/start.sh"
  printf "RUSTFIN_BACKEND_PORT=%q\n" "$RUSTFIN_BACKEND_PORT"
  printf "RUSTFIN_UI_PORT=%q\n" "$RUSTFIN_UI_PORT"
  printf "RUSTFIN_MEDIA_PATH=%q\n" "$RUSTFIN_MEDIA_PATH"
} > "$RUNTIME_ENV_FILE"
chmod 600 "$RUNTIME_ENV_FILE" 2>/dev/null || true

if [[ "$DETACH" == "true" && "$HEALTH_CHECK" == "true" && -n "$(command -v curl || true)" ]]; then
  info "Waiting for backend health endpoint..."
  ok=false
  for _ in $(seq 1 60); do
    if curl -fsS "http://127.0.0.1:${RUSTFIN_BACKEND_PORT}/health" >/dev/null 2>&1; then
      ok=true
      break
    fi
    sleep 1
  done
  if [[ "$ok" != "true" ]]; then
    warn "Backend health check did not pass within 60s."
    warn "Check logs with: docker compose -f \"$COMPOSE_FILE\" logs -f"
  fi
fi

success "Rustyfin stack is up."
echo "  Backend: http://localhost:${RUSTFIN_BACKEND_PORT}"
echo "  UI:      http://localhost:${RUSTFIN_UI_PORT}"
