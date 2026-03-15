#!/usr/bin/env bash
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
RESET='\033[0m'

info()    { echo -e "${CYAN}[install-linux]${RESET} $*"; }
success() { echo -e "${GREEN}[install-linux]${RESET} $*"; }
warn()    { echo -e "${YELLOW}[install-linux]${RESET} $*"; }
die()     { echo -e "${RED}[install-linux] ERROR:${RESET} $*" >&2; exit 1; }

usage() {
  cat <<'EOF'
Usage:
  ./scripts/install_linux.sh [installer-args...]

Bootstraps a Linux host for Rustyfin by:
- detecting the OS/package manager
- installing minimal Rust bootstrap dependencies
- installing Rust via rustup when needed
- handing off to `cargo run -p rustfin-installer`

Installer args are forwarded to the Rust installer.

Examples:
  ./scripts/install_linux.sh
  ./scripts/install_linux.sh --skip-prereqs
  ./scripts/install_linux.sh --skip-systemd

Root-led install targeting a non-root runtime user:
  RUSTFIN_NATIVE_USER=tempo ./scripts/install_linux.sh
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

[[ "$(uname -s)" == "Linux" ]] || die "Rustyfin Linux bootstrap currently supports Linux hosts only."
[[ -f /etc/os-release ]] || die "/etc/os-release not found."

# shellcheck disable=SC1091
source /etc/os-release

if [[ "$(id -u)" -eq 0 ]]; then
  RUN_ROOT=()
  INSTALL_USER="${RUSTFIN_NATIVE_USER:-${SUDO_USER:-root}}"
  INSTALLER_RUN_AS_ROOT=true
else
  command -v sudo >/dev/null 2>&1 || die "sudo is required when not running as root."
  RUN_ROOT=(sudo)
  INSTALL_USER="${RUSTFIN_NATIVE_USER:-${USER:-$(id -un)}}"
  INSTALLER_RUN_AS_ROOT=false
fi

INSTALL_HOME="$(getent passwd "$INSTALL_USER" | cut -d: -f6 || true)"
[[ -n "$INSTALL_HOME" ]] || die "Unable to resolve home directory for user: $INSTALL_USER"
ROOT_HOME="$(getent passwd root | cut -d: -f6 || true)"
[[ -n "$ROOT_HOME" ]] || die "Unable to resolve home directory for user: root"

if [[ "$(id -u)" -eq 0 && "$INSTALL_USER" != "root" ]]; then
  RUN_AS_INSTALL_USER=(runuser -u "$INSTALL_USER" --)
else
  RUN_AS_INSTALL_USER=()
fi

detect_package_manager() {
  if command -v apt-get >/dev/null 2>&1; then
    echo "apt"
    return
  fi
  if command -v dnf >/dev/null 2>&1; then
    echo "dnf"
    return
  fi
  if command -v pacman >/dev/null 2>&1; then
    echo "pacman"
    return
  fi
  if command -v zypper >/dev/null 2>&1; then
    echo "zypper"
    return
  fi
  die "No supported package manager detected (expected apt-get, dnf, pacman, or zypper)."
}

install_bootstrap_packages() {
  local manager="$1"
  case "$manager" in
    apt)
      info "Installing Rust bootstrap packages with apt..."
      "${RUN_ROOT[@]}" apt-get update
      "${RUN_ROOT[@]}" apt-get install -y \
        build-essential \
        ca-certificates \
        curl \
        git \
        pkg-config \
        sudo
      ;;
    dnf)
      info "Installing Rust bootstrap packages with dnf..."
      "${RUN_ROOT[@]}" dnf install -y \
        ca-certificates \
        curl \
        gcc \
        gcc-c++ \
        git \
        make \
        pkgconf-pkg-config \
        sudo
      ;;
    pacman)
      info "Installing Rust bootstrap packages with pacman..."
      "${RUN_ROOT[@]}" pacman -Sy --noconfirm --needed \
        base-devel \
        ca-certificates \
        curl \
        git \
        pkgconf \
        sudo
      ;;
    zypper)
      info "Installing Rust bootstrap packages with zypper..."
      "${RUN_ROOT[@]}" zypper --non-interactive install \
        ca-certificates \
        curl \
        gcc \
        gcc-c++ \
        git \
        make \
        pkg-config \
        sudo
      ;;
    *)
      die "Unsupported package manager: $manager"
      ;;
  esac
}

ensure_rustup() {
  local cargo_bin="${INSTALL_HOME}/.cargo/bin/cargo"
  local rustc_bin="${INSTALL_HOME}/.cargo/bin/rustc"

  if [[ -x "$cargo_bin" && -x "$rustc_bin" ]]; then
    info "Rust toolchain already present for user ${INSTALL_USER}."
    return
  fi

  info "Installing Rust toolchain via rustup for user ${INSTALL_USER}..."
  if [[ "${#RUN_AS_INSTALL_USER[@]}" -gt 0 ]]; then
    "${RUN_AS_INSTALL_USER[@]}" bash -lc 'curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal'
  else
    curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal
  fi

  [[ -x "$cargo_bin" ]] || die "cargo was not installed successfully for user ${INSTALL_USER}"
}

ensure_root_rustup() {
  local cargo_bin="${ROOT_HOME}/.cargo/bin/cargo"
  local rustc_bin="${ROOT_HOME}/.cargo/bin/rustc"

  if [[ -x "$cargo_bin" && -x "$rustc_bin" ]]; then
    info "Rust toolchain already present for user root."
    return
  fi

  info "Installing Rust toolchain via rustup for user root..."
  HOME="$ROOT_HOME" curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal

  [[ -x "$cargo_bin" ]] || die "cargo was not installed successfully for user root"
}

PACKAGE_MANAGER="$(detect_package_manager)"
info "Detected host: ${ID:-unknown} ${VERSION_ID:-unknown}"
info "Detected package manager: ${PACKAGE_MANAGER}"

install_bootstrap_packages "$PACKAGE_MANAGER"
if [[ "$INSTALLER_RUN_AS_ROOT" == "true" ]]; then
  ensure_root_rustup
  CARGO_BIN="${ROOT_HOME}/.cargo/bin/cargo"
  [[ -x "$CARGO_BIN" ]] || die "cargo not found at $CARGO_BIN"

  success "Bootstrap complete. Handing off to rustfin-installer as root for native user ${INSTALL_USER}..."
  export RUSTFIN_NATIVE_USER="$INSTALL_USER"
  export PATH="${ROOT_HOME}/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
  exec "$CARGO_BIN" run --locked -p rustfin-installer -- "$@"
else
  ensure_rustup
  CARGO_BIN="${INSTALL_HOME}/.cargo/bin/cargo"
  [[ -x "$CARGO_BIN" ]] || die "cargo not found at $CARGO_BIN"

  success "Bootstrap complete. Handing off to rustfin-installer..."
  if [[ "${#RUN_AS_INSTALL_USER[@]}" -gt 0 ]]; then
    exec "${RUN_AS_INSTALL_USER[@]}" env \
      HOME="$INSTALL_HOME" \
      PATH="${INSTALL_HOME}/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
      "$CARGO_BIN" run --locked -p rustfin-installer -- "$@"
  fi

  export PATH="${INSTALL_HOME}/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
  exec "$CARGO_BIN" run --locked -p rustfin-installer -- "$@"
fi
