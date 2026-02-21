#!/usr/bin/env bash
set -euo pipefail

# Wipe Rustyfin user-generated runtime data for a true "first run" state.
# After this script, running ./scripts/start.sh should require setup wizard again.

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
RESET='\033[0m'

info()    { echo -e "${CYAN}[clean-install]${RESET} $*"; }
success() { echo -e "${GREEN}[clean-install]${RESET} $*"; }
warn()    { echo -e "${YELLOW}[clean-install]${RESET} $*"; }
die()     { echo -e "${RED}[clean-install] ERROR:${RESET} $*" >&2; exit 1; }

usage() {
  cat <<'EOF'
Usage:
  ./scripts/clean_install.sh [--yes] [-f <compose-file>]

Options:
  --yes        Skip interactive confirmation.
  -f, --file   Compose file path (default: docker-compose.yml).
  -h, --help   Show this help.
EOF
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

ASSUME_YES=false
COMPOSE_FILE="$REPO_ROOT/docker-compose.yml"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --yes|-y)
      ASSUME_YES=true
      shift
      ;;
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

if [[ "$ASSUME_YES" != "true" ]]; then
  echo
  warn "This will DELETE Rustyfin runtime/user data (DB, cache, transcode, volumes)."
  warn "After this, start.sh will boot as a first-time install."
  echo
  read -r -p "Type 'yes' to continue: " confirm
  [[ "$confirm" == "yes" ]] || { info "Aborted."; exit 0; }
fi

info "Stopping stack and removing compose volumes..."
docker compose -f "$COMPOSE_FILE" down --remove-orphans --volumes

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

# Local runtime/state paths (for non-docker or mixed usage)
delete_file_if_exists() {
  local p="$1"
  if [[ -f "$p" ]]; then
    rm -f "$p"
    info "Deleted file: $p"
  fi
}

delete_dir_if_exists() {
  local p="$1"
  if [[ -d "$p" ]]; then
    rm -rf "$p"
    info "Deleted dir: $p"
  fi
}

delete_file_if_exists "$REPO_ROOT/rustfin.db"
delete_file_if_exists "$REPO_ROOT/rustfin.db-shm"
delete_file_if_exists "$REPO_ROOT/rustfin.db-wal"
delete_file_if_exists "$REPO_ROOT/.rustyfin.runtime.env"
delete_file_if_exists "$SAFE_TMP_DIR/directory-picker-helper.py"
delete_file_if_exists "$SAFE_TMP_DIR/directory-picker-helper.log"

delete_file_if_exists "$REPO_ROOT/scripts/rustfin.db"
delete_file_if_exists "$REPO_ROOT/scripts/rustfin.db-shm"
delete_file_if_exists "$REPO_ROOT/scripts/rustfin.db-wal"

delete_dir_if_exists "/tmp/rustfin_cache"
delete_dir_if_exists "/tmp/rustfin_transcode"
delete_dir_if_exists "$REPO_ROOT/tests/_runs"

success "Clean install reset complete."
echo "Next step:"
echo "  ./scripts/start.sh"
