#!/usr/bin/env bash
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
RESET='\033[0m'

info()    { echo -e "${CYAN}[rustyfin-bootstrap]${RESET} $*"; }
success() { echo -e "${GREEN}[rustyfin-bootstrap]${RESET} $*"; }
warn()    { echo -e "${YELLOW}[rustyfin-bootstrap]${RESET} $*"; }
die()     { echo -e "${RED}[rustyfin-bootstrap] ERROR:${RESET} $*" >&2; exit 1; }

usage() {
  cat <<'USAGE'
Usage:
  ./scripts/install_linux_complete.sh [rustfin-installer args...]

This wrapper prepares a supported Debian/Ubuntu host for Rustyfin, then runs:
  cargo run --locked -p rustfin-installer -- install --skip-prereqs [args...]

Supported hosts:
  - Debian 12
  - Debian 13
  - Ubuntu 22.04
  - Ubuntu 24.04

Environment:
  REPO_ROOT            Path to the Rustyfin checkout (default: current directory)
  RUSTFIN_NATIVE_USER  Native runtime/build user (default: current non-root user,
                       or SUDO_USER when invoked as root via sudo)
  RUSTFIN_PG_USER      PostgreSQL role to create/update (default: rustfin)
  RUSTFIN_PG_PASSWORD  PostgreSQL password (default: rustfin)
  RUSTFIN_PG_DB        PostgreSQL database to create (default: rustfin)
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

REPO_ROOT="${REPO_ROOT:-$(pwd)}"
[[ -d "$REPO_ROOT" ]] || die "REPO_ROOT does not exist: $REPO_ROOT"
[[ -f "$REPO_ROOT/Cargo.toml" ]] || die "Run this from the Rustyfin repo root, or set REPO_ROOT=/path/to/Rustyfin"
[[ -d "$REPO_ROOT/crates" ]] || die "REPO_ROOT does not look like the Rustyfin repository: $REPO_ROOT"

cd "$REPO_ROOT"

[[ -f /etc/os-release ]] || die "/etc/os-release not found"
# shellcheck disable=SC1091
source /etc/os-release

case "${ID:-}:${VERSION_ID:-}" in
  debian:12|debian:13|ubuntu:22.04|ubuntu:24.04)
    ;;
  *)
    die "Unsupported host: ${ID:-unknown} ${VERSION_ID:-unknown}. Supported: Debian 12/13, Ubuntu 22.04/24.04"
    ;;
esac

if [[ "$(id -u)" -eq 0 ]]; then
  RUN_ROOT=()
else
  command -v sudo >/dev/null 2>&1 || die "sudo is required when not running as root"
  sudo -v
  while true; do
    sudo -n true
    sleep 60
    kill -0 "$$" >/dev/null 2>&1 || exit
  done 2>/dev/null &
  SUDO_KEEPALIVE_PID=$!
  trap 'kill "$SUDO_KEEPALIVE_PID" >/dev/null 2>&1 || true' EXIT
  RUN_ROOT=(sudo)
fi

run_root() {
  if [[ ${#RUN_ROOT[@]} -gt 0 ]]; then
    "${RUN_ROOT[@]}" "$@"
  else
    "$@"
  fi
}

if [[ -n "${RUSTFIN_NATIVE_USER:-}" ]]; then
  NATIVE_USER="$RUSTFIN_NATIVE_USER"
elif [[ "$(id -u)" -eq 0 ]]; then
  NATIVE_USER="${SUDO_USER:-root}"
else
  NATIVE_USER="${USER:-$(id -un)}"
fi

NATIVE_HOME="$(getent passwd "$NATIVE_USER" | cut -d: -f6 || true)"
[[ -n "$NATIVE_HOME" ]] || die "Unable to resolve home directory for native user: $NATIVE_USER"

if [[ "$(id -u)" -eq 0 && "$NATIVE_USER" != "root" ]]; then
  run_as_native_user() {
    runuser -u "$NATIVE_USER" -- env \
      -u RUSTUP_HOME \
      -u CARGO_HOME \
      HOME="$NATIVE_HOME" \
      USER="$NATIVE_USER" \
      LOGNAME="$NATIVE_USER" \
      PATH="$NATIVE_HOME/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
      bash -lc "$1"
  }
else
  run_as_native_user() {
    bash -lc "$1"
  }
fi

version_ge() {
  local have="$1"
  local want="$2"
  [[ "$(printf '%s\n%s\n' "$want" "$have" | sort -V | tail -n1)" == "$have" ]]
}

node_ok() {
  command -v node >/dev/null 2>&1 || return 1
  local node_version
  node_version="$(node -v 2>/dev/null | sed 's/^v//')"
  [[ -n "$node_version" ]] || return 1
  version_ge "$node_version" "20.9.0"
}

npm_ok() {
  command -v npm >/dev/null 2>&1
}

ensure_ubuntu_components() {
  if [[ "${ID:-}" != "ubuntu" ]]; then
    return 0
  fi

  info "Ensuring Ubuntu Universe repository is enabled..."
  run_root apt-get update
  run_root apt-get install -y software-properties-common
  run_root add-apt-repository -y universe

  if command -v nvidia-smi >/dev/null 2>&1; then
    info "NVIDIA GPU detected; ensuring Ubuntu Multiverse repository is enabled for CUDA packages..."
    run_root add-apt-repository -y multiverse
  fi
}

setup_caddy_repo() {
  info "Configuring official Caddy APT repository..."
  run_root apt-get install -y debian-keyring debian-archive-keyring apt-transport-https curl gpg ca-certificates
  run_root mkdir -p /usr/share/keyrings /etc/apt/sources.list.d
  curl -fsSL 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | run_root gpg --batch --yes --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
  curl -fsSL 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' | run_root tee /etc/apt/sources.list.d/caddy-stable.list >/dev/null
  run_root chmod a+r /usr/share/keyrings/caddy-stable-archive-keyring.gpg
  run_root chmod a+r /etc/apt/sources.list.d/caddy-stable.list
}

install_official_caddy() {
  info "Installing official Caddy package..."
  run_root apt-get update
  run_root apt-get install -y caddy
}

setup_nodesource_20() {
  info "Configuring NodeSource Node.js 20.x repository..."
  local tmp_script
  tmp_script="$(mktemp)"
  curl -fsSL https://deb.nodesource.com/setup_20.x -o "$tmp_script"
  run_root bash "$tmp_script"
  rm -f "$tmp_script"
}

install_base_packages() {
  info "Installing system build/runtime prerequisites..."
  run_root apt-get update
  run_root apt-get install -y \
    build-essential \
    ca-certificates \
    clang \
    clinfo \
    cmake \
    curl \
    default-jre-headless \
    ffmpeg \
    git \
    gnupg \
    gpg \
    iproute2 \
    jq \
    libclblast-dev \
    libclang-dev \
    libpq-dev \
    libsqlite3-dev \
    libssl-dev \
    lsof \
    ocl-icd-libopencl1 \
    ocl-icd-opencl-dev \
    openssl \
    pkg-config \
    postgresql \
    postgresql-client \
    python3 \
    python3-pip \
    python3-venv \
    sudo \
    tar \
    xz-utils
}

ensure_node_and_npm() {
  if node_ok && npm_ok; then
    info "Node.js and npm already satisfy the UI build requirement."
    return 0
  fi

  case "${ID:-}:${VERSION_ID:-}" in
    debian:13)
      info "Installing distro Node.js/npm on Debian 13..."
      run_root apt-get update
      run_root apt-get install -y nodejs npm
      if node_ok && npm_ok; then
        info "Distro Node.js/npm satisfies the UI build requirement."
        return 0
      fi
      warn "Distro Node.js is still too old for the UI build. Falling back to NodeSource Node.js 20.x..."
      setup_nodesource_20
      run_root apt-get update
      run_root apt-get install -y nodejs
      ;;
    *)
      setup_nodesource_20
      run_root apt-get update
      run_root apt-get install -y nodejs
      ;;
  esac

  node_ok || die "Node.js >= 20.9.0 is required for the Rustyfin UI build"
  npm_ok || die "npm was not installed successfully"

  info "Node version: $(node -v)"
  info "npm version: $(npm -v)"
}

ensure_rustup_for_native_user() {
  if run_as_native_user 'test -x "$HOME/.cargo/bin/cargo" && test -x "$HOME/.cargo/bin/rustc"'; then
    info "Rust toolchain already present for native user $NATIVE_USER"
    return 0
  fi

  info "Installing Rust toolchain for native user $NATIVE_USER..."
  run_as_native_user 'tmp_rustup="$(mktemp)" && curl -fsSL https://sh.rustup.rs -o "$tmp_rustup" && sh "$tmp_rustup" -y --profile minimal && rm -f "$tmp_rustup"'
  run_as_native_user 'test -x "$HOME/.cargo/bin/cargo" && test -x "$HOME/.cargo/bin/rustc"'
}

install_ytdlp() {
  info "Installing yt-dlp runtime..."
  run_root python3 -m pip install --break-system-packages --upgrade yt-dlp
}

ensure_postgresql() {
  local pg_user pg_password pg_db pg_password_sql
  pg_user="${RUSTFIN_PG_USER:-rustfin}"
  pg_password="${RUSTFIN_PG_PASSWORD:-rustfin}"
  pg_db="${RUSTFIN_PG_DB:-rustfin}"

  [[ "$pg_user" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]] || die "RUSTFIN_PG_USER must be alphanumeric/underscore and start with a letter/underscore"
  [[ "$pg_db" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]] || die "RUSTFIN_PG_DB must be alphanumeric/underscore and start with a letter/underscore"
  pg_password_sql="${pg_password//\'/\'\'}"

  info "Ensuring PostgreSQL is enabled..."
  run_root systemctl enable --now postgresql

  info "Configuring PostgreSQL role/database..."
  if [[ "$(id -u)" -eq 0 ]]; then
    runuser -u postgres -- psql -v ON_ERROR_STOP=1 postgres <<SQL
DO \$\$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '${pg_user}') THEN
    EXECUTE 'CREATE ROLE "${pg_user}" LOGIN PASSWORD ''${pg_password_sql}''';
  ELSE
    EXECUTE 'ALTER ROLE "${pg_user}" WITH LOGIN PASSWORD ''${pg_password_sql}''';
  END IF;
END
\$\$;
SQL

    if ! runuser -u postgres -- psql -tAc "SELECT 1 FROM pg_database WHERE datname='${pg_db}'" postgres | grep -q 1; then
      runuser -u postgres -- createdb -O "$pg_user" "$pg_db"
    fi
  else
    sudo -u postgres psql -v ON_ERROR_STOP=1 postgres <<SQL
DO \$\$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '${pg_user}') THEN
    EXECUTE 'CREATE ROLE "${pg_user}" LOGIN PASSWORD ''${pg_password_sql}''';
  ELSE
    EXECUTE 'ALTER ROLE "${pg_user}" WITH LOGIN PASSWORD ''${pg_password_sql}''';
  END IF;
END
\$\$;
SQL

    if ! sudo -u postgres psql -tAc "SELECT 1 FROM pg_database WHERE datname='${pg_db}'" postgres | grep -q 1; then
      sudo -u postgres createdb -O "$pg_user" "$pg_db"
    fi
  fi
}

java_major_version() {
  local java_bin="$1"
  "$java_bin" -version 2>&1 | awk -F[\".] '/version/ {print $2; exit}' || true
}

install_managed_java_21() {
  local arch temp_dir archive_path download_url install_parent install_dir current_link
  case "$(dpkg --print-architecture)" in
    arm64) arch="aarch64" ;;
    amd64) arch="x64" ;;
    *) die "Unsupported architecture for managed Java 21 install: $(dpkg --print-architecture)" ;;
  esac

  install_parent="/opt/rustyfin/java"
  install_dir="${install_parent}/temurin-21"
  current_link="${install_parent}/current"
  temp_dir="$(mktemp -d)"
  archive_path="${temp_dir}/temurin21.tar.gz"
  download_url="https://api.adoptium.net/v3/binary/latest/21/ga/linux/${arch}/jdk/hotspot/normal/eclipse"
  trap 'rm -rf "${temp_dir:-}"' RETURN

  info "Installing managed Java 21 runtime..."
  curl -fsSL "$download_url" -o "$archive_path"
  run_root install -d -m 755 "$install_parent"
  run_root rm -rf "$install_dir"
  run_root mkdir -p "$install_dir"
  run_root tar -xzf "$archive_path" -C "$install_dir" --strip-components=1
  run_root ln -sfn "$install_dir" "$current_link"
  run_root chmod -R a+rX "$install_dir"
  [[ -x "${current_link}/bin/java" ]] || die "Managed Java 21 install did not produce ${current_link}/bin/java"
}

ensure_managed_java_21() {
  local current_java="/opt/rustyfin/java/current/bin/java"
  if [[ -x "$current_java" ]]; then
    local java_major
    java_major="$(java_major_version "$current_java")"
    if [[ "$java_major" == "21" ]]; then
      info "Managed Java 21 already available at $current_java"
      return 0
    fi
  fi
  install_managed_java_21
}

ensure_optional_cuda() {
  if ! command -v nvidia-smi >/dev/null 2>&1; then
    return 0
  fi

  if command -v nvcc >/dev/null 2>&1 || [[ -x /usr/local/cuda/bin/nvcc ]] || [[ -x /usr/local/cuda-12/bin/nvcc ]] || [[ -f /usr/include/cuda.h ]]; then
    info "CUDA toolkit already present."
    return 0
  fi

  info "NVIDIA GPU detected; attempting to install CUDA toolkit..."
  if run_root apt-get install -y nvidia-cuda-toolkit; then
    success "CUDA toolkit installed successfully."
  else
    warn "CUDA toolkit install failed. On Ubuntu, ensure Multiverse is enabled. On Debian, ensure the required non-free repository components are enabled. Rustyfin can still run with CPU inference."
  fi
}

run_rustfin_installer() {
  info "Running rustfin-installer with --skip-prereqs..."
  local installer_cmd='source "$HOME/.cargo/env" && cargo run --locked -p rustfin-installer -- install --skip-prereqs'
  local arg
  for arg in "$@"; do
    installer_cmd+=" $(printf '%q' "$arg")"
  done
  run_as_native_user "$installer_cmd"
}

info "Host: ${ID:-unknown} ${VERSION_ID:-unknown}"
info "Repo root: $REPO_ROOT"
info "Native user: $NATIVE_USER ($NATIVE_HOME)"

ensure_ubuntu_components
setup_caddy_repo
install_base_packages
install_official_caddy
ensure_node_and_npm
ensure_rustup_for_native_user
install_ytdlp
ensure_postgresql
ensure_managed_java_21
ensure_optional_cuda
run_rustfin_installer "$@"

success "Rustyfin bootstrap completed."
