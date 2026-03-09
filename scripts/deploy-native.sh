#!/usr/bin/env bash
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
RESET='\033[0m'

info()    { echo -e "${CYAN}[deploy-native]${RESET} $*"; }
success() { echo -e "${GREEN}[deploy-native]${RESET} $*"; }
warn()    { echo -e "${YELLOW}[deploy-native]${RESET} $*"; }
die()     { echo -e "${RED}[deploy-native] ERROR:${RESET} $*" >&2; exit 1; }

usage() {
  cat <<'EOF'
Usage:
  ./scripts/deploy-native.sh [--skip-git-pull] [--foreground] [--no-health-check]

Behavior:
  - stops native Rustyfin runtime/systemd units
  - pulls the latest code from the current git branch (unless --skip-git-pull)
  - rebuilds native Rust/UI artifacts without launching processes
  - restarts via systemd if native units are installed
  - otherwise starts the runtime directly

Options:
  --skip-git-pull    Reuse the current local checkout.
  --foreground       When no systemd unit is installed, start attached.
  --no-health-check  Skip post-start health waits.
  -h, --help         Show this help.
EOF
}

GIT_PULL=true
FOREGROUND=false
HEALTH_CHECK=true

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-git-pull) GIT_PULL=false; shift ;;
    --foreground) FOREGROUND=true; shift ;;
    --no-health-check) HEALTH_CHECK=false; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "Unknown argument: $1" ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

[[ "$(uname -s)" == "Linux" ]] || die "Native deployment is supported on Linux hosts only. Use Debian 12."
command -v git >/dev/null 2>&1 || die "git is required."
command -v systemctl >/dev/null 2>&1 || warn "systemctl not found; deploy will fall back to direct start."

if ! command -v cargo >/dev/null 2>&1 && [[ -f "${HOME}/.cargo/env" ]]; then
  # shellcheck disable=SC1090
  source "${HOME}/.cargo/env"
fi

command -v cargo >/dev/null 2>&1 || die "cargo is not installed. Run ./scripts/install_native_debian.sh first."
command -v rustc >/dev/null 2>&1 || die "rustc is not installed. Run ./scripts/install_native_debian.sh first."
command -v node >/dev/null 2>&1 || die "node is not installed. Run ./scripts/install_native_debian.sh first."
command -v npm >/dev/null 2>&1 || die "npm is not installed. Run ./scripts/install_native_debian.sh first."

if [[ "$(id -u)" -eq 0 ]]; then
  RUN_ROOT=()
else
  command -v sudo >/dev/null 2>&1 || die "sudo is required when not running as root."
  RUN_ROOT=(sudo)
fi

REPO_OWNER_USER="$(id -un)"
REPO_OWNER_GROUP="$(id -gn)"

MAIN_SERVICE_NAME="${RUSTFIN_SYSTEMD_SERVICE:-rustyfin-native.service}"
AGENT_SERVICE_NAME="${RUSTFIN_SERVERS_AGENT_SERVICE:-rustfin-servers-agent.service}"

service_exists() {
  local service_name="$1"
  systemctl cat "$service_name" >/dev/null 2>&1
}

if [[ "$(id -u)" -ne 0 ]] && ( service_exists "${RUSTFIN_SYSTEMD_SERVICE:-rustyfin-native.service}" || service_exists "${RUSTFIN_SERVERS_AGENT_SERVICE:-rustfin-servers-agent.service}" ); then
  info "Refreshing sudo credentials for systemd operations..."
  sudo -v
fi

stop_service_if_present() {
  local service_name="$1"
  if service_exists "$service_name"; then
    info "Stopping ${service_name}..."
    "${RUN_ROOT[@]}" systemctl stop "$service_name" 2>/dev/null || true
  fi
}

start_service_required() {
  local service_name="$1"
  info "Starting ${service_name}..."
  "${RUN_ROOT[@]}" systemctl start "$service_name"
}

repair_build_artifact_ownership() {
  local path=""
  for path in \
    "$REPO_ROOT/ui/.next" \
    "$REPO_ROOT/.native-bins" \
    "$REPO_ROOT/target" \
    "$REPO_ROOT/.tmp/native-runtime"
  do
    [[ -e "$path" ]] || continue
    if [[ "$(id -u)" -eq 0 ]]; then
      chown -R "${REPO_OWNER_USER}:${REPO_OWNER_GROUP}" "$path"
    else
      "${RUN_ROOT[@]}" chown -R "${REPO_OWNER_USER}:${REPO_OWNER_GROUP}" "$path"
    fi
  done
}

if [[ "$GIT_PULL" == "true" ]]; then
  branch_name="$(git rev-parse --abbrev-ref HEAD)"
  [[ "$branch_name" != "HEAD" ]] || die "Repository is in detached HEAD state. Check out a branch before deploying."
  if [[ -n "$(git status --short)" ]]; then
    die "Working tree is not clean. Commit or stash local changes before deploying."
  fi
fi

stop_service_if_present "$MAIN_SERVICE_NAME"
stop_service_if_present "$AGENT_SERVICE_NAME"

info "Stopping any running native runtime processes..."
"$REPO_ROOT/scripts/stop-native.sh" || true

if [[ "$GIT_PULL" == "true" ]]; then
  branch_name="$(git rev-parse --abbrev-ref HEAD)"
  info "Pulling latest ${branch_name}..."
  git pull --ff-only origin "$branch_name"
else
  info "Skipping git pull."
fi

info "Rebuilding native artifacts..."
repair_build_artifact_ownership
"$REPO_ROOT/scripts/start-native.sh" --build-only

if service_exists "$MAIN_SERVICE_NAME"; then
  if service_exists "$AGENT_SERVICE_NAME"; then
    start_service_required "$AGENT_SERVICE_NAME"
  fi
  start_service_required "$MAIN_SERVICE_NAME"
  success "Native systemd deployment completed."
  info "Check status with:"
  echo "  systemctl status ${MAIN_SERVICE_NAME}"
  if service_exists "$AGENT_SERVICE_NAME"; then
    echo "  systemctl status ${AGENT_SERVICE_NAME}"
  fi
else
  info "No native systemd unit detected; starting runtime directly..."
  start_args=(--no-build)
  if [[ "$FOREGROUND" == "true" ]]; then
    start_args+=(--foreground)
  fi
  if [[ "$HEALTH_CHECK" == "false" ]]; then
    start_args+=(--no-health-check)
  fi
  "$REPO_ROOT/scripts/start-native.sh" "${start_args[@]}"
fi
