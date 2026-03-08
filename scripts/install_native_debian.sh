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

Installs Debian 12 native host prerequisites for Rustyfin:
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
[[ "${ID:-}" == "debian" ]] || die "This installer targets Debian only."
[[ "${VERSION_ID:-}" == "12" ]] || die "This installer targets Debian 12 (bookworm)."

if [[ "$(id -u)" -eq 0 ]]; then
  RUN_ROOT=()
  RUN_POSTGRES=(runuser -u postgres --)
else
  command -v sudo >/dev/null 2>&1 || die "sudo is required when not running as root."
  RUN_ROOT=(sudo)
  RUN_POSTGRES=(sudo -u postgres)
fi

info "Installing Debian 12 native runtime dependencies..."
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
  default-jre-headless \
  python3 \
  python3-pip \
  python3-venv

if ! command -v cargo >/dev/null 2>&1 || ! command -v rustc >/dev/null 2>&1; then
  info "Installing Rust toolchain via rustup..."
  curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal
fi

if [[ -f "${HOME}/.cargo/env" ]]; then
  # shellcheck disable=SC1090
  source "${HOME}/.cargo/env"
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
    warn "Detected Java ${java_major}. Minecraft 1.21.x server runtime typically needs Java 21."
  fi
else
  warn "Java is not installed. Minecraft server provisioning/runtime will need a suitable JRE/JDK."
fi

success "Debian 12 native host prerequisites are installed."
success "Next steps:"
echo "  1. source ~/.cargo/env"
echo "  2. ./scripts/start-native.sh"
