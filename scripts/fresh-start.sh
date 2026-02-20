#!/usr/bin/env bash
# fresh-start.sh — Reset Rustyfin to a clean install state and start it up.
#
# Usage:
#   ./scripts/fresh-start.sh           # Local dev (default)
#   ./scripts/fresh-start.sh --docker  # Docker Compose
#   ./scripts/fresh-start.sh --no-run  # Wipe only, don't start the server
#
# What it wipes:
#   Local:  rustfin.db (or $RUSTFIN_DB), $RUSTFIN_TRANSCODE_DIR, $RUSTFIN_CACHE_DIR
#   Docker: stops containers, removes named volumes (config/cache/transcode)

set -euo pipefail

# ── Colours ────────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
RESET='\033[0m'

info()    { echo -e "${CYAN}[fresh-start]${RESET} $*"; }
success() { echo -e "${GREEN}[fresh-start]${RESET} $*"; }
warn()    { echo -e "${YELLOW}[fresh-start]${RESET} $*"; }
die()     { echo -e "${RED}[fresh-start] ERROR:${RESET} $*" >&2; exit 1; }

# ── Argument parsing ────────────────────────────────────────────────────────────
MODE="local"
RUN=true

for arg in "$@"; do
  case "$arg" in
    --docker)  MODE="docker" ;;
    --no-run)  RUN=false ;;
    -h|--help)
      echo "Usage: $0 [--docker] [--no-run]"
      echo "  --docker   Reset and start via Docker Compose instead of local binary"
      echo "  --no-run   Only wipe data, do not start the server afterwards"
      exit 0
      ;;
    *) die "Unknown argument: $arg" ;;
  esac
done

# ── Repo root ───────────────────────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# ── Confirm ─────────────────────────────────────────────────────────────────────
echo ""
warn "This will DELETE all Rustyfin data (database, cache, transcode files)."
warn "Mode: $MODE"
echo ""
read -r -p "Are you sure? Type 'yes' to continue: " CONFIRM
[[ "$CONFIRM" == "yes" ]] || { info "Aborted."; exit 0; }
echo ""

# ══════════════════════════════════════════════════════════════════════════════
# DOCKER MODE
# ══════════════════════════════════════════════════════════════════════════════
if [[ "$MODE" == "docker" ]]; then
  command -v docker >/dev/null 2>&1 || die "docker not found in PATH"

  info "Stopping containers..."
  docker compose -f "$REPO_ROOT/docker-compose.yml" down --remove-orphans || true

  info "Removing named volumes (config / cache / transcode)..."
  docker volume rm rustfin_rustfin-config  2>/dev/null && info "  removed rustfin_rustfin-config"  || warn "  rustfin_rustfin-config not found (skipping)"
  docker volume rm rustfin_rustfin-cache   2>/dev/null && info "  removed rustfin_rustfin-cache"   || warn "  rustfin_rustfin-cache not found (skipping)"
  docker volume rm rustfin_rustfin-transcode 2>/dev/null && info "  removed rustfin_rustfin-transcode" || warn "  rustfin_rustfin-transcode not found (skipping)"

  success "Data wiped."

  if [[ "$RUN" == true ]]; then
    info "Building and starting Docker Compose services..."
    docker compose -f "$REPO_ROOT/docker-compose.yml" up --build -d
    success "Services started."
    echo ""
    info "Backend → http://localhost:8096"
    info "Frontend → http://localhost:3000"
    info "Run 'docker compose logs -f' to follow logs."
  fi

  exit 0
fi

# ══════════════════════════════════════════════════════════════════════════════
# LOCAL DEV MODE
# ══════════════════════════════════════════════════════════════════════════════

# Resolve paths from env vars (matching main.rs defaults)
DB_PATH="${RUSTFIN_DB:-$REPO_ROOT/rustfin.db}"
TRANSCODE_DIR="${RUSTFIN_TRANSCODE_DIR:-/tmp/rustfin_transcode}"
CACHE_DIR="${RUSTFIN_CACHE_DIR:-/tmp/rustfin_cache}"

# Kill any running rustfin-server process before wiping
if pgrep -x "rustfin-server" >/dev/null 2>&1; then
  warn "Found running rustfin-server — stopping it..."
  pkill -x "rustfin-server" || true
  sleep 1
fi

# Remove database (including WAL and SHM sidecar files)
for f in "$DB_PATH" "${DB_PATH}-wal" "${DB_PATH}-shm"; do
  if [[ -f "$f" ]]; then
    rm -f "$f"
    info "Deleted: $f"
  fi
done

# Clear transcode directory
if [[ -d "$TRANSCODE_DIR" ]]; then
  rm -rf "$TRANSCODE_DIR"
  info "Deleted: $TRANSCODE_DIR"
fi

# Clear cache directory
if [[ -d "$CACHE_DIR" ]]; then
  rm -rf "$CACHE_DIR"
  info "Deleted: $CACHE_DIR"
fi

success "Data wiped — Rustyfin is now in a clean install state."
echo ""

if [[ "$RUN" == false ]]; then
  exit 0
fi

# Build and run
command -v cargo >/dev/null 2>&1 || die "cargo not found — install Rust first"

info "Building Rustyfin (release)..."
cargo build --release --manifest-path "$REPO_ROOT/Cargo.toml" 2>&1

BINARY="$REPO_ROOT/target/release/rustfin-server"
[[ -f "$BINARY" ]] || die "Build succeeded but binary not found at $BINARY"

info "Starting rustfin-server..."
echo ""
exec "$BINARY"
