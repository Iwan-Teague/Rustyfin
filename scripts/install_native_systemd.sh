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

Installs systemd units that keep the native Debian Rustyfin runtime online:
- rustyfin-native.service
- rustfin-servers-agent.service

Behavior:
- creates/updates a shared env file at /etc/rustyfin/servers-agent.env
- enables both services at boot
- starts/restarts them immediately

Environment:
  RUSTFIN_NATIVE_USER            Main service account username (defaults to invoking user)
  RUSTFIN_SYSTEMD_SERVICE        Override main service name (default: rustyfin-native.service)
  RUSTFIN_SERVERS_AGENT_SERVICE  Override servers agent service name (default: rustfin-servers-agent.service)
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
command -v openssl >/dev/null 2>&1 || die "openssl is required."

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

MAIN_SERVICE_NAME="${RUSTFIN_SYSTEMD_SERVICE:-rustyfin-native.service}"
AGENT_SERVICE_NAME="${RUSTFIN_SERVERS_AGENT_SERVICE:-rustfin-servers-agent.service}"
MAIN_SERVICE_PATH="/etc/systemd/system/${MAIN_SERVICE_NAME}"
AGENT_SERVICE_PATH="/etc/systemd/system/${AGENT_SERVICE_NAME}"
ENV_DIR="/etc/rustyfin"
ENV_FILE="${ENV_DIR}/servers-agent.env"
LOG_DIR="${REPO_ROOT}/.tmp/native-runtime/logs"

mkdir -p "$LOG_DIR"
if [[ "$(id -u)" -eq 0 ]]; then
  chown -R "$RUSTFIN_NATIVE_USER":"$RUSTFIN_NATIVE_USER" "${REPO_ROOT}/.tmp" 2>/dev/null || true
fi

main_unit_tmp="$(mktemp)"
agent_unit_tmp="$(mktemp)"
env_tmp="$(mktemp)"
trap 'rm -f "$main_unit_tmp" "$agent_unit_tmp" "$env_tmp"' EXIT

existing_token=""
if "${RUN_ROOT[@]}" test -f "$ENV_FILE"; then
  existing_token="$("${RUN_ROOT[@]}" awk -F= '/^RUSTFIN_SERVERS_AGENT_TOKEN=/{print $2}' "$ENV_FILE" 2>/dev/null | tail -n 1 || true)"
fi
existing_token="${existing_token%\"}"
existing_token="${existing_token#\"}"
servers_agent_token="${existing_token:-$(openssl rand -hex 24)}"

cat > "$env_tmp" <<EOF
RUSTFIN_SERVERS_AGENT_BIND=127.0.0.1:8103
RUSTFIN_SERVERS_AGENT_URL=http://127.0.0.1:8103
RUSTFIN_SERVERS_AGENT_TOKEN=${servers_agent_token}
RUSTFIN_SERVERS_SYSTEM_USER=${RUSTFIN_NATIVE_USER}
RUSTFIN_SERVERS_SYSTEM_GROUP=${RUSTFIN_NATIVE_USER}
EOF

cat > "$agent_unit_tmp" <<EOF
[Unit]
Description=Rustyfin Privileged Servers Agent
Wants=network-online.target
After=network-online.target

[Service]
Type=simple
User=root
Group=root
WorkingDirectory=${REPO_ROOT}
Environment=HOME=/root
Environment=PATH=/usr/local/bin:/usr/bin:/bin
EnvironmentFile=-${ENV_FILE}
ExecStart=/usr/bin/bash -lc '${REPO_ROOT}/scripts/start-native-servers-agent.sh'
Restart=on-failure
RestartSec=2
StandardOutput=append:${LOG_DIR}/rustfin-servers-agent-systemd.log
StandardError=append:${LOG_DIR}/rustfin-servers-agent-systemd.log

[Install]
WantedBy=multi-user.target
EOF

cat > "$main_unit_tmp" <<EOF
[Unit]
Description=Rustyfin Native Runtime
Wants=network-online.target postgresql.service ${AGENT_SERVICE_NAME}
After=network-online.target postgresql.service ${AGENT_SERVICE_NAME}

[Service]
Type=oneshot
RemainAfterExit=yes
User=${RUSTFIN_NATIVE_USER}
Group=${RUSTFIN_NATIVE_USER}
WorkingDirectory=${REPO_ROOT}
Environment=HOME=${RUSTFIN_NATIVE_HOME}
Environment=PATH=${RUSTFIN_NATIVE_HOME}/.cargo/bin:/usr/local/bin:/usr/bin:/bin
Environment=RUSTFIN_ENABLE_SERVERS_AGENT=0
EnvironmentFile=-${ENV_FILE}
ExecStart=/usr/bin/bash -lc 'source ${RUSTFIN_NATIVE_HOME}/.cargo/env && ${REPO_ROOT}/scripts/start-native.sh --no-build'
ExecStop=/usr/bin/bash -lc '${REPO_ROOT}/scripts/stop-native.sh'
TimeoutStartSec=0
TimeoutStopSec=120
StandardOutput=append:${LOG_DIR}/rustyfin-native-systemd.log
StandardError=append:${LOG_DIR}/rustyfin-native-systemd.log

[Install]
WantedBy=multi-user.target
EOF

info "Installing shared servers-agent environment..."
"${RUN_ROOT[@]}" install -d -m 755 "$ENV_DIR"
"${RUN_ROOT[@]}" cp "$env_tmp" "$ENV_FILE"
"${RUN_ROOT[@]}" chmod 600 "$ENV_FILE"

info "Installing ${AGENT_SERVICE_NAME}..."
"${RUN_ROOT[@]}" cp "$agent_unit_tmp" "$AGENT_SERVICE_PATH"
"${RUN_ROOT[@]}" chmod 644 "$AGENT_SERVICE_PATH"

info "Installing ${MAIN_SERVICE_NAME} for user ${RUSTFIN_NATIVE_USER}..."
"${RUN_ROOT[@]}" cp "$main_unit_tmp" "$MAIN_SERVICE_PATH"
"${RUN_ROOT[@]}" chmod 644 "$MAIN_SERVICE_PATH"

"${RUN_ROOT[@]}" systemctl daemon-reload
"${RUN_ROOT[@]}" systemctl enable "$AGENT_SERVICE_NAME"
"${RUN_ROOT[@]}" systemctl enable "$MAIN_SERVICE_NAME"
"${RUN_ROOT[@]}" systemctl stop "$MAIN_SERVICE_NAME" 2>/dev/null || true
"${RUN_ROOT[@]}" systemctl restart "$AGENT_SERVICE_NAME"
"${RUN_ROOT[@]}" systemctl start "$MAIN_SERVICE_NAME"

success "Installed and started ${AGENT_SERVICE_NAME}"
success "Installed and started ${MAIN_SERVICE_NAME}"
success "Check status with:"
echo "  systemctl status ${AGENT_SERVICE_NAME}"
echo "  systemctl status ${MAIN_SERVICE_NAME}"
