#!/usr/bin/env bash
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
RESET='\033[0m'

info()    { echo -e "${CYAN}[install-native-systemd]${RESET} $*"; }
success() { echo -e "${GREEN}[install-native-systemd]${RESET} $*"; }
die()     { echo -e "${RED}[install-native-systemd] ERROR:${RESET} $*" >&2; exit 1; }

usage() {
  cat <<'EOF'
Usage:
  ./scripts/install_native_systemd.sh

Installs a systemd unit that starts/stops the native Debian Rustyfin runtime.

Behavior:
- Creates/updates /etc/systemd/system/rustyfin-native.service
- Enables it at boot
- Starts it immediately

Environment:
  RUSTFIN_NATIVE_USER     Service account username (defaults to invoking user)
  RUSTFIN_SYSTEMD_SERVICE Override service name (default: rustyfin-native.service)
EOF
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

[[ "$(uname -s)" == "Linux" ]] || die "This installer targets Linux hosts only."
command -v systemctl >/dev/null 2>&1 || die "systemctl is required."

if [[ "$(id -u)" -eq 0 ]]; then
  RUN_ROOT=()
  DEFAULT_NATIVE_USER="${SUDO_USER:-root}"
else
  command -v sudo >/dev/null 2>&1 || die "sudo is required when not running as root."
  RUN_ROOT=(sudo)
  DEFAULT_NATIVE_USER="${USER:-$(id -un)}"
fi

RUSTFIN_NATIVE_USER="${RUSTFIN_NATIVE_USER:-$DEFAULT_NATIVE_USER}"
RUSTFIN_NATIVE_HOME="$(getent passwd "$RUSTFIN_NATIVE_USER" | cut -d: -f6 || true)"
[[ -n "$RUSTFIN_NATIVE_HOME" ]] || die "Unable to resolve home directory for user: $RUSTFIN_NATIVE_USER"

SERVICE_NAME="${RUSTFIN_SYSTEMD_SERVICE:-rustyfin-native.service}"
SERVICE_PATH="/etc/systemd/system/${SERVICE_NAME}"
LOG_DIR="${REPO_ROOT}/.tmp/native-runtime/logs"

mkdir -p "$LOG_DIR"
if [[ "$(id -u)" -eq 0 ]]; then
  chown -R "$RUSTFIN_NATIVE_USER":"$RUSTFIN_NATIVE_USER" "${REPO_ROOT}/.tmp" 2>/dev/null || true
fi

tmp_service="$(mktemp)"
cat > "$tmp_service" <<EOF
[Unit]
Description=Rustyfin Native Runtime
Wants=network-online.target postgresql.service
After=network-online.target postgresql.service

[Service]
Type=oneshot
RemainAfterExit=yes
User=${RUSTFIN_NATIVE_USER}
Group=${RUSTFIN_NATIVE_USER}
WorkingDirectory=${REPO_ROOT}
Environment=HOME=${RUSTFIN_NATIVE_HOME}
Environment=PATH=${RUSTFIN_NATIVE_HOME}/.cargo/bin:/usr/local/bin:/usr/bin:/bin
ExecStart=/usr/bin/bash -lc 'source ${RUSTFIN_NATIVE_HOME}/.cargo/env && ${REPO_ROOT}/scripts/start-native.sh --no-build'
ExecStop=/usr/bin/bash -lc '${REPO_ROOT}/scripts/stop-native.sh'
TimeoutStartSec=0
TimeoutStopSec=120
StandardOutput=append:${LOG_DIR}/rustyfin-native-systemd.log
StandardError=append:${LOG_DIR}/rustyfin-native-systemd.log

[Install]
WantedBy=multi-user.target
EOF

trap 'rm -f "$tmp_service"' EXIT

info "Installing ${SERVICE_NAME} for user ${RUSTFIN_NATIVE_USER}..."
"${RUN_ROOT[@]}" cp "$tmp_service" "$SERVICE_PATH"
"${RUN_ROOT[@]}" chmod 644 "$SERVICE_PATH"
"${RUN_ROOT[@]}" systemctl daemon-reload
"${RUN_ROOT[@]}" systemctl enable --now "$SERVICE_NAME"

success "Installed and started ${SERVICE_NAME}"
success "Check status with: systemctl status ${SERVICE_NAME}"
