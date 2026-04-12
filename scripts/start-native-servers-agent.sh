#!/usr/bin/env bash
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
RESET='\033[0m'

info()    { echo -e "${CYAN}[start-native-servers-agent]${RESET} $*"; }
success() { echo -e "${GREEN}[start-native-servers-agent]${RESET} $*"; }
die()     { echo -e "${RED}[start-native-servers-agent] ERROR:${RESET} $*" >&2; exit 1; }

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

[[ "$(uname -s)" == "Linux" ]] || die "Native servers agent is supported on Linux hosts only."

RUSTFIN_RUST_BUILD_PROFILE="${RUSTFIN_RUST_BUILD_PROFILE:-release}"

host_arch="$(uname -m)"
case "$host_arch" in
  arm64|aarch64) RUSTFIN_NATIVE_TARGET="${RUSTFIN_NATIVE_LINUX_TARGET:-aarch64-unknown-linux-gnu}" ;;
  x86_64|amd64) RUSTFIN_NATIVE_TARGET="${RUSTFIN_NATIVE_LINUX_TARGET:-x86_64-unknown-linux-gnu}" ;;
  *) die "Unsupported host arch '$host_arch'; set RUSTFIN_NATIVE_LINUX_TARGET explicitly." ;;
esac

NATIVE_BIN_DIR_ABS="$REPO_ROOT/.native-bins/${RUSTFIN_NATIVE_TARGET}/${RUSTFIN_RUST_BUILD_PROFILE}"
BIN_PATH="$NATIVE_BIN_DIR_ABS/rustfin-servers-agent"
[[ -x "$BIN_PATH" ]] || die "Native servers agent binary is missing at $BIN_PATH. Build Rustyfin natively first."

export RUSTFIN_SERVERS_AGENT_BIND="${RUSTFIN_SERVERS_AGENT_BIND:-127.0.0.1:8103}"

info "Starting rustfin-servers-agent from $BIN_PATH"
exec "$BIN_PATH"
