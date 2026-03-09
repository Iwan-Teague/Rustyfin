#!/usr/bin/env bash
set -euo pipefail

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
  cat <<'USAGE'
Usage:
  ./scripts/clean_install.sh [--yes]

Resets Rustyfin runtime/user data for the native Debian 12 stack.
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

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

if [[ "$ASSUME_YES" != "true" ]]; then
  echo
  warn "This will DELETE Rustyfin runtime/user data (PostgreSQL contents, cache, transcode, logs, runtime env)."
  warn "Built binaries and source code are kept. The next start will boot as a first-time install."
  echo
  read -r -p "Type 'yes' to continue: " confirm
  [[ "$confirm" == "yes" ]] || { info "Aborted."; exit 0; }
fi

MAIN_SERVICE_NAME="${RUSTFIN_SYSTEMD_SERVICE:-rustyfin-native.service}"
AGENT_SERVICE_NAME="${RUSTFIN_SERVERS_AGENT_SERVICE:-rustfin-servers-agent.service}"
if command -v systemctl >/dev/null 2>&1; then
  if systemctl cat "$MAIN_SERVICE_NAME" >/dev/null 2>&1; then
    info "Stopping ${MAIN_SERVICE_NAME}..."
    sudo systemctl stop "$MAIN_SERVICE_NAME" 2>/dev/null || true
  fi
  if systemctl cat "$AGENT_SERVICE_NAME" >/dev/null 2>&1; then
    info "Stopping ${AGENT_SERVICE_NAME}..."
    sudo systemctl stop "$AGENT_SERVICE_NAME" 2>/dev/null || true
  fi
fi

"$REPO_ROOT/scripts/stop-native.sh" || true

SAFE_TMP_DIR="${RUSTFIN_TMPDIR:-$REPO_ROOT/.tmp}"
RUNTIME_ROOT="${RUSTFIN_NATIVE_RUNTIME_DIR:-$SAFE_TMP_DIR/native-runtime}"
RUNTIME_ENV_FILE="$REPO_ROOT/.rustyfin.runtime.env"

DB_URL="${RUSTFIN_DATABASE_URL:-}"
if [[ -z "$DB_URL" && -f "$RUNTIME_ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$RUNTIME_ENV_FILE" || true
  DB_URL="${RUSTFIN_DATABASE_URL:-}"
fi
if [[ -z "$DB_URL" ]]; then
  pg_user="${RUSTFIN_PG_USER:-rustfin}"
  pg_password="${RUSTFIN_PG_PASSWORD:-rustfin}"
  pg_db="${RUSTFIN_PG_DB:-rustfin}"
  DB_URL="postgresql://${pg_user}:${pg_password}@127.0.0.1:5432/${pg_db}"
fi

if command -v psql >/dev/null 2>&1; then
  info "Resetting PostgreSQL schema contents..."
  psql "$DB_URL" -v ON_ERROR_STOP=1 <<'SQL'
DROP SCHEMA IF EXISTS public CASCADE;
CREATE SCHEMA public;
GRANT ALL ON SCHEMA public TO CURRENT_USER;
GRANT ALL ON SCHEMA public TO public;
SQL
else
  warn "psql not found; skipping database reset."
fi

rm -f "$RUNTIME_ENV_FILE"
rm -f "$SAFE_TMP_DIR/directory-picker-helper.pid" "$SAFE_TMP_DIR/directory-picker-helper.py" "$SAFE_TMP_DIR/directory-picker-helper.log"
rm -rf "$RUNTIME_ROOT" /tmp/rustfin_cache /tmp/rustfin_transcode "$REPO_ROOT/tests/_runs"

success "Native clean install reset complete."
echo "Next step:"
echo "  ./scripts/start.sh"
