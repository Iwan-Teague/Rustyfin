#!/usr/bin/env bash
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
RESET='\033[0m'

info()    { echo -e "${CYAN}[install-native]${RESET} $*"; }
success() { echo -e "${GREEN}[install-native]${RESET} $*"; }
warn()    { echo -e "${YELLOW}[install-native]${RESET} $*"; }
die()     { echo -e "${RED}[install-native] ERROR:${RESET} $*" >&2; exit 1; }

usage() {
  cat <<'EOF'
Usage:
  ./scripts/install_native_debian.sh

Installs supported Debian native host prerequisites for Rustyfin:
- PostgreSQL
- Caddy
- Rust toolchain
- Node/npm
- ffmpeg/ffprobe
- native Rust build deps
- yt-dlp runtime
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

[[ -f /etc/os-release ]] || die "/etc/os-release not found"
# shellcheck disable=SC1091
source /etc/os-release
# Supported: Debian 11/12/13/14 and Ubuntu LTS
case "${ID:-}" in
  debian)
    case "${VERSION_ID:-}" in
      11|12|13|14) ;;
      *) warn "Running on Debian ${VERSION_ID:-unknown}. Tested on Debian 12/13. Proceeding." ;;
    esac
    ;;
  ubuntu) ;;
  *)
    warn "Running on ${ID:-unknown} ${VERSION_ID:-}. Tested on Debian/Ubuntu LTS. Proceeding."
    ;;
esac

if [[ "$(id -u)" -eq 0 ]]; then
  RUN_ROOT=()
  RUN_POSTGRES=(runuser -u postgres --)
else
  command -v sudo >/dev/null 2>&1 || die "sudo is required when not running as root."
  RUN_ROOT=(sudo)
  RUN_POSTGRES=(sudo -u postgres)
fi

if [[ "$(id -u)" -eq 0 ]]; then
  RUSTFIN_NATIVE_USER="${SUDO_USER:-root}"
else
  RUSTFIN_NATIVE_USER="${USER:-$(id -un)}"
fi

RUSTFIN_NATIVE_USER_HOME="$(getent passwd "$RUSTFIN_NATIVE_USER" | cut -d: -f6 || true)"
[[ -n "$RUSTFIN_NATIVE_USER_HOME" ]] || die "Unable to resolve home directory for user: $RUSTFIN_NATIVE_USER"

RUSTFIN_MANAGED_JAVA_ROOT="/opt/rustyfin/java"
RUSTFIN_MANAGED_JAVA_CURRENT="${RUSTFIN_MANAGED_JAVA_ROOT}/current"

if [[ "$RUSTFIN_NATIVE_USER" == "root" ]]; then
  RUN_NATIVE_USER=()
else
  if [[ "$(id -u)" -eq 0 ]]; then
    RUN_NATIVE_USER=(runuser -u "$RUSTFIN_NATIVE_USER" --)
  else
    RUN_NATIVE_USER=()
  fi
fi

java_major_version() {
  local java_bin="$1"
  "$java_bin" -version 2>&1 | awk -F[\".] '/version/ {print $2; exit}' || true
}

install_managed_java_21() {
  local arch
  case "$(dpkg --print-architecture)" in
    arm64) arch="aarch64" ;;
    amd64) arch="x64" ;;
    *) die "Unsupported Debian architecture for managed Java 21 install: $(dpkg --print-architecture)" ;;
  esac

  local install_parent="${RUSTFIN_MANAGED_JAVA_ROOT}"
  local install_dir="${install_parent}/temurin-21"
  local temp_dir
  local archive_path
  local download_url="https://api.adoptium.net/v3/binary/latest/21/ga/linux/${arch}/jdk/hotspot/normal/eclipse"

  temp_dir="$(mktemp -d)"
  archive_path="${temp_dir}/temurin21.tar.gz"
  trap 'rm -rf "$temp_dir"' RETURN

  info "Installing managed Java 21 runtime for Minecraft..."
  "${RUN_ROOT[@]}" install -d -m 755 "$install_parent"
  "${RUN_ROOT[@]}" curl -fsSL "$download_url" -o "$archive_path"
  "${RUN_ROOT[@]}" rm -rf "$install_dir"
  "${RUN_ROOT[@]}" mkdir -p "$install_dir"
  "${RUN_ROOT[@]}" tar -xzf "$archive_path" -C "$install_dir" --strip-components=1
  "${RUN_ROOT[@]}" ln -sfn "$install_dir" "$RUSTFIN_MANAGED_JAVA_CURRENT"
  "${RUN_ROOT[@]}" chmod -R a+rX "$install_dir"

  if [[ ! -x "${RUSTFIN_MANAGED_JAVA_CURRENT}/bin/java" ]]; then
    die "Managed Java 21 install did not produce ${RUSTFIN_MANAGED_JAVA_CURRENT}/bin/java"
  fi

  success "Managed Java 21 installed at ${RUSTFIN_MANAGED_JAVA_CURRENT}"
}

info "Installing supported Debian native runtime dependencies..."
"${RUN_ROOT[@]}" apt-get update
"${RUN_ROOT[@]}" apt-get install -y \
  build-essential \
  ca-certificates \
  caddy \
  clang \
  clinfo \
  cmake \
  curl \
  ffmpeg \
  git \
  iproute2 \
  jq \
  libclang-dev \
  libclblast-dev \
  libpq-dev \
  libsqlite3-dev \
  libssl-dev \
  lsof \
  nodejs \
  npm \
  ocl-icd-libopencl1 \
  ocl-icd-opencl-dev \
  openssl \
  pkg-config \
  postgresql \
  postgresql-client \
  sudo \
  default-jre-headless \
  python3 \
  python3-pip \
  python3-venv


# Install CUDA toolkit if NVIDIA GPU present and toolkit not yet available
# Install CUDA toolkit if NVIDIA GPU present and toolkit not yet available
if command -v nvidia-smi >/dev/null 2>&1; then
  if ! command -v nvcc >/dev/null 2>&1 \
      && ! [[ -x /usr/local/cuda/bin/nvcc ]] \
      && ! [[ -x /usr/local/cuda-12/bin/nvcc ]] \
      && ! [[ -f /usr/include/cuda.h ]]; then
    info "NVIDIA GPU detected. Installing CUDA toolkit for GPU-accelerated AI..."
    "${RUN_ROOT[@]}" apt-get install -y nvidia-cuda-toolkit \
      || warn "CUDA toolkit install failed. Install manually: sudo apt-get install nvidia-cuda-toolkit"
  fi
fi

if [[ ! -x "${RUSTFIN_NATIVE_USER_HOME}/.cargo/bin/cargo" ]] || [[ ! -x "${RUSTFIN_NATIVE_USER_HOME}/.cargo/bin/rustc" ]]; then
  info "Installing Rust toolchain via rustup for user ${RUSTFIN_NATIVE_USER}..."
  if [[ "${#RUN_NATIVE_USER[@]}" -gt 0 ]]; then
    "${RUN_NATIVE_USER[@]}" bash -lc 'curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal'
  else
    curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal
  fi
fi

if [[ -f "${RUSTFIN_NATIVE_USER_HOME}/.cargo/env" ]]; then
  # shellcheck disable=SC1090
  source "${RUSTFIN_NATIVE_USER_HOME}/.cargo/env"
fi

command -v cargo >/dev/null 2>&1 || die "cargo was not installed successfully"
command -v rustc >/dev/null 2>&1 || die "rustc was not installed successfully"

info "Installing yt-dlp runtime..."
python3 -m pip install --break-system-packages --upgrade yt-dlp

info "Ensuring PostgreSQL is running..."
"${RUN_ROOT[@]}" systemctl enable --now postgresql

pg_user="${RUSTFIN_PG_USER:-rustfin}"
pg_password="${RUSTFIN_PG_PASSWORD:-rustfin}"
pg_db="${RUSTFIN_PG_DB:-rustfin}"

[[ "$pg_user" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]] || die "RUSTFIN_PG_USER must be alphanumeric/underscore and start with a letter/underscore"
[[ "$pg_db" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]] || die "RUSTFIN_PG_DB must be alphanumeric/underscore and start with a letter/underscore"
pg_password_sql="${pg_password//\'/\'\'}"

info "Configuring PostgreSQL role/database..."
"${RUN_POSTGRES[@]}" psql -v ON_ERROR_STOP=1 postgres <<SQL
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

if ! "${RUN_POSTGRES[@]}" psql -tAc "SELECT 1 FROM pg_database WHERE datname='${pg_db}'" postgres | grep -q 1; then
  "${RUN_POSTGRES[@]}" createdb -O "$pg_user" "$pg_db"
fi

if command -v java >/dev/null 2>&1; then
  java_major="$(java -version 2>&1 | awk -F[\".] '/version/ {print $2; exit}' || true)"
  if [[ -n "$java_major" ]] && [[ "$java_major" -lt 21 ]]; then
    warn "Detected Java ${java_major}. Installing managed Java 21 for Minecraft 1.21.x compatibility."
    install_managed_java_21
  fi
else
  warn "Java is not installed. Installing managed Java 21 for Minecraft."
  install_managed_java_21
fi

if [[ -x "${RUSTFIN_MANAGED_JAVA_CURRENT}/bin/java" ]]; then
  success "Rustyfin Minecraft default Java runtime: ${RUSTFIN_MANAGED_JAVA_CURRENT}/bin/java"
fi

success "Supported Debian native host prerequisites are installed."
success "Next steps:"
echo "  1. source ${RUSTFIN_NATIVE_USER_HOME}/.cargo/env"
echo "  2. ./scripts/start-native.sh"
