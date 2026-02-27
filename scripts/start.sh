#!/usr/bin/env bash
set -euo pipefail

# Start the full Rustyfin Docker stack in a fresh clone or existing workspace.
# Safe defaults:
# - uses a writable repo-local TMPDIR (fixes macOS temp permission issues)
# - auto-creates a local media directory if none is provided
# - can auto-pick free host ports when defaults are occupied

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
RESET='\033[0m'

info()    { echo -e "${CYAN}[start]${RESET} $*"; }
success() { echo -e "${GREEN}[start]${RESET} $*"; }
warn()    { echo -e "${YELLOW}[start]${RESET} $*"; }
die()     { echo -e "${RED}[start] ERROR:${RESET} $*" >&2; exit 1; }

usage() {
  cat <<'EOF'
Usage:
  ./scripts/start.sh [--no-build|--full-rebuild] [--foreground] [--no-health-check] [--youtube-cookie <cookie>] [--docker-rust-build|--native-rust-build] [-f <compose-file>]

Options:
  --build            Smart rebuild (only changed services, default behavior).
  --full-rebuild     Rebuild without cache (slowest, strictest).
  --cached-build     Alias for --build.
  --no-build         Skip image rebuild step.
  --native-rust-build
                     Build Rust service binaries on host and copy them into Docker runtime images (default).
  --docker-rust-build
                     Build Rust service binaries inside Docker builder stages.
  --foreground       Run compose in foreground (default is detached).
  --no-health-check  Skip backend health wait loop.
  --youtube-cookie   Set/persist RUSTFIN_YOUTUBE_COOKIE for online listen-together.
  RUSTFIN_RUST_BUILD_PROFILE
                     Rust Docker build profile (default: dev). Set to release for optimized builds.
  RUSTFIN_NATIVE_LINUX_TARGET
                     Override host->Linux target triple for native Rust builds.
  RUSTFIN_NATIVE_RUST_BUILD
                     Set to 0 to disable native Rust build mode globally.
  -f, --file         Compose file path (default: docker-compose.yml).
  -h, --help         Show this help.
EOF
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

BUILD=true
NO_CACHE_BUILD=false
DETACH=true
HEALTH_CHECK=true
COMPOSE_FILE="$REPO_ROOT/docker-compose.yml"
CLI_YOUTUBE_COOKIE=""
RUSTFIN_RUST_BUILD_PROFILE="${RUSTFIN_RUST_BUILD_PROFILE:-dev}"
NATIVE_RUST_BUILD="${RUSTFIN_NATIVE_RUST_BUILD:-1}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --build|--cached-build) BUILD=true; NO_CACHE_BUILD=false; shift ;;
    --full-rebuild) BUILD=true; NO_CACHE_BUILD=true; shift ;;
    --no-build) BUILD=false; shift ;;
    --native-rust-build) NATIVE_RUST_BUILD=1; shift ;;
    --docker-rust-build) NATIVE_RUST_BUILD=0; shift ;;
    --foreground) DETACH=false; shift ;;
    --no-health-check) HEALTH_CHECK=false; shift ;;
    --youtube-cookie)
      [[ $# -ge 2 ]] || die "Missing value for $1"
      CLI_YOUTUBE_COOKIE="$2"
      shift 2
      ;;
    -f|--file)
      [[ $# -ge 2 ]] || die "Missing value for $1"
      COMPOSE_FILE="$2"
      shift 2
      ;;
    -h|--help) usage; exit 0 ;;
    *) die "Unknown argument: $1" ;;
  esac
done

if [[ "$COMPOSE_FILE" != /* ]]; then
  COMPOSE_FILE="$REPO_ROOT/$COMPOSE_FILE"
fi

cd "$REPO_ROOT"

[[ -f "$COMPOSE_FILE" ]] || die "docker-compose.yml not found at $COMPOSE_FILE"
command -v docker >/dev/null 2>&1 || die "docker is not installed or not in PATH"
docker compose version >/dev/null 2>&1 || die "docker compose is not available"

if [[ -n "$CLI_YOUTUBE_COOKIE" ]]; then
  export RUSTFIN_YOUTUBE_COOKIE="$CLI_YOUTUBE_COOKIE"
fi

RUNTIME_ENV_FILE="$REPO_ROOT/.rustyfin.runtime.env"

SAFE_TMP_DIR="${RUSTFIN_TMPDIR:-$REPO_ROOT/.tmp}"
mkdir -p "$SAFE_TMP_DIR" || die "Failed to create temp dir: $SAFE_TMP_DIR"
chmod 700 "$SAFE_TMP_DIR" 2>/dev/null || true
[[ -w "$SAFE_TMP_DIR" ]] || die "Temp dir is not writable: $SAFE_TMP_DIR"
export TMPDIR="$SAFE_TMP_DIR"

BUILD_STATE_FILE="$SAFE_TMP_DIR/build-fingerprints.env"

if command -v shasum >/dev/null 2>&1; then
  hash_file() {
    shasum -a 256 "$1" | awk '{print $1}'
  }
  hash_stdin() {
    shasum -a 256 | awk '{print $1}'
  }
elif command -v sha256sum >/dev/null 2>&1; then
  hash_file() {
    sha256sum "$1" | awk '{print $1}'
  }
  hash_stdin() {
    sha256sum | awk '{print $1}'
  }
else
  die "No SHA-256 tool found (expected shasum or sha256sum)"
fi

collect_scope_files() {
  local input=""
  for input in "$@"; do
    if [[ -f "$input" ]]; then
      printf '%s\n' "$input"
    elif [[ -d "$input" ]]; then
      find "$input" -type f \
        ! -path '*/.git/*' \
        ! -path '*/target/*' \
        ! -path '*/node_modules/*' \
        ! -path '*/.next/*' \
        ! -path '*/dist/*' \
        ! -path '*/coverage/*'
    fi
  done | sort -u
}

compute_scope_fingerprint() {
  local -a files=()
  local file=""
  while IFS= read -r file; do
    [[ -n "$file" ]] && files+=("$file")
  done < <(collect_scope_files "$@")

  if [[ ${#files[@]} -eq 0 ]]; then
    printf '%s' "empty"
    return
  fi

  {
    for file in "${files[@]}"; do
      printf '%s  %s\n' "$(hash_file "$file")" "${file#$REPO_ROOT/}"
    done
  } | hash_stdin
}

compose_service_hash() {
  local service="$1"
  local line=""
  local hash=""

  line="$(docker compose -f "$COMPOSE_FILE" config --hash "$service" 2>/dev/null | awk -v svc="$service" '$1 == svc { print $2; exit }' || true)"
  hash="${line##*$'\n'}"

  if [[ -n "$hash" ]]; then
    printf '%s' "$hash"
    return
  fi

  # Safe fallback: if per-service compose hashing is unavailable, use the
  # compose file hash so build invalidation is still correct (but broader).
  printf '%s' "$(hash_file "$COMPOSE_FILE")"
}

project_name_default() {
  basename "$REPO_ROOT" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9]//g'
}

service_image_name() {
  local project_name
  project_name="$(project_name_default)"
  printf '%s-%s:latest' "$project_name" "$1"
}

image_exists_for_service() {
  local image_name
  image_name="$(service_image_name "$1")"
  docker image inspect "$image_name" >/dev/null 2>&1
}

resolve_native_linux_target() {
  local override="${RUSTFIN_NATIVE_LINUX_TARGET:-}"
  if [[ -n "$override" ]]; then
    printf '%s' "$override"
    return
  fi

  local arch
  arch="$(uname -m)"
  case "$arch" in
    arm64|aarch64) printf '%s' "aarch64-unknown-linux-gnu" ;;
    x86_64|amd64) printf '%s' "x86_64-unknown-linux-gnu" ;;
    *)
      die "Unsupported host arch '$arch' for native Linux Rust build. Set RUSTFIN_NATIVE_LINUX_TARGET explicitly."
      ;;
  esac
}

service_to_bin() {
  case "$1" in
    rustfin) printf '%s' "rustfin-server" ;;
    rustfin-calendar) printf '%s' "rustfin-calendar" ;;
    rustfin-tmdb-agent) printf '%s' "rustfin-tmdb-agent" ;;
    rustfin-transcription-agent) printf '%s' "rustfin-transcription-agent" ;;
    rustfin-youtube-agent) printf '%s' "rustfin-youtube-agent" ;;
    *) return 1 ;;
  esac
}

build_native_rust_bins() {
  local native_target="$1"
  local native_bin_dir="$2"
  shift 2
  local services=("$@")
  local bins=()
  local service=""
  local bin=""

  for service in "${services[@]}"; do
    if bin="$(service_to_bin "$service" 2>/dev/null)"; then
      bins+=("$bin")
    fi
  done

  if [[ ${#bins[@]} -eq 0 ]]; then
    return
  fi

  info "Native Rust build enabled; compiling Linux binaries on host (${native_target})..."
  info "Native binary output: ${native_bin_dir}"

  local -a cmd=( "$REPO_ROOT/scripts/build_linux_binaries.sh"
    --profile "$RUSTFIN_RUST_BUILD_PROFILE"
    --target "$native_target"
    --output-dir "$native_bin_dir"
    --cache-dir "$SAFE_TMP_DIR/native-linux/.build-cache"
  )

  for bin in "${bins[@]}"; do
    cmd+=(--bin "$bin")
  done

  if ! "${cmd[@]}"; then
    die "Native Rust Linux binary build failed. Install prerequisites (zig + cargo-zigbuild for macOS/Windows) or run with --docker-rust-build."
  fi
}

service_build_reason() {
  local service="$1"
  local prev_fingerprint="$2"
  local current_fingerprint="$3"
  local reasons=()

  if ! image_exists_for_service "$service"; then
    reasons+=("image missing")
  fi

  if [[ -z "$prev_fingerprint" ]]; then
    # If a prior build state file exists but this specific key is missing
    # (for example after smart-rebuild schema evolution), do not force a
    # one-time rebuild purely due absent tracking metadata.
    # We still rebuild when the image is missing (handled above).
    if [[ "${BUILD_STATE_EXISTS:-false}" == "true" ]]; then
      :
    else
      reasons+=("first tracked build")
    fi
  elif [[ "$prev_fingerprint" != "$current_fingerprint" ]]; then
    reasons+=("source changed")
  fi

  if [[ ${#reasons[@]} -eq 0 ]]; then
    return 1
  fi

  local joined
  joined="$(IFS=', '; echo "${reasons[*]}")"
  printf '%s' "$joined"
}

save_build_fingerprints() {
  local rustfin_fp="$1"
  local calendar_fp="$2"
  local tmdb_agent_fp="$3"
  local transcription_agent_fp="$4"
  local youtube_agent_fp="$5"
  local ui_fp="$6"

  {
    echo "# Generated by scripts/start.sh"
    printf "RUSTFIN_FP=%q\n" "$rustfin_fp"
    printf "CALENDAR_FP=%q\n" "$calendar_fp"
    printf "TMDB_AGENT_FP=%q\n" "$tmdb_agent_fp"
    printf "TRANSCRIPTION_AGENT_FP=%q\n" "$transcription_agent_fp"
    printf "YOUTUBE_AGENT_FP=%q\n" "$youtube_agent_fp"
    printf "UI_FP=%q\n" "$ui_fp"
    printf "UPDATED_TS=%q\n" "$(date +%s)"
  } > "$BUILD_STATE_FILE"
  chmod 600 "$BUILD_STATE_FILE" 2>/dev/null || true
}

compute_all_service_fingerprints() {
  local rustfin_compose_hash=""
  local calendar_compose_hash=""
  local tmdb_agent_compose_hash=""
  local transcription_agent_compose_hash=""
  local youtube_agent_compose_hash=""
  local ui_compose_hash=""

  rustfin_compose_hash="$(compose_service_hash "rustfin")"
  calendar_compose_hash="$(compose_service_hash "rustfin-calendar")"
  tmdb_agent_compose_hash="$(compose_service_hash "rustfin-tmdb-agent")"
  transcription_agent_compose_hash="$(compose_service_hash "rustfin-transcription-agent")"
  youtube_agent_compose_hash="$(compose_service_hash "rustfin-youtube-agent")"
  ui_compose_hash="$(compose_service_hash "rustfin-ui")"

  current_rustfin_fp="${RUSTFIN_RUST_BUILD_MODE_KEY}:${RUSTFIN_RUST_BUILD_PROFILE}:compose=${rustfin_compose_hash}:$(compute_scope_fingerprint \
    "$REPO_ROOT/Dockerfile" \
    "$REPO_ROOT/docker/native" \
    "$REPO_ROOT/scripts/build_linux_binaries.sh" \
    "$REPO_ROOT/Cargo.toml" \
    "$REPO_ROOT/Cargo.lock" \
    "$REPO_ROOT/crates/server" \
    "$REPO_ROOT/crates/core" \
    "$REPO_ROOT/crates/db" \
    "$REPO_ROOT/crates/scanner" \
    "$REPO_ROOT/crates/metadata" \
    "$REPO_ROOT/crates/transcoder")"
  current_calendar_fp="${RUSTFIN_RUST_BUILD_MODE_KEY}:${RUSTFIN_RUST_BUILD_PROFILE}:compose=${calendar_compose_hash}:$(compute_scope_fingerprint \
    "$REPO_ROOT/crates/calendar/Dockerfile" \
    "$REPO_ROOT/docker/native" \
    "$REPO_ROOT/scripts/build_linux_binaries.sh" \
    "$REPO_ROOT/Cargo.toml" \
    "$REPO_ROOT/Cargo.lock" \
    "$REPO_ROOT/crates/calendar" \
    "$REPO_ROOT/crates/core" \
    "$REPO_ROOT/crates/db")"
  current_tmdb_agent_fp="${RUSTFIN_RUST_BUILD_MODE_KEY}:${RUSTFIN_RUST_BUILD_PROFILE}:compose=${tmdb_agent_compose_hash}:$(compute_scope_fingerprint \
    "$REPO_ROOT/crates/tmdb-agent/Dockerfile" \
    "$REPO_ROOT/docker/native" \
    "$REPO_ROOT/scripts/build_linux_binaries.sh" \
    "$REPO_ROOT/Cargo.toml" \
    "$REPO_ROOT/Cargo.lock" \
    "$REPO_ROOT/crates/tmdb-agent" \
    "$REPO_ROOT/crates/core" \
    "$REPO_ROOT/crates/db" \
    "$REPO_ROOT/crates/metadata")"
  current_transcription_agent_fp="${RUSTFIN_RUST_BUILD_MODE_KEY}:${RUSTFIN_RUST_BUILD_PROFILE}:compose=${transcription_agent_compose_hash}:$(compute_scope_fingerprint \
    "$REPO_ROOT/crates/transcription-agent/Dockerfile" \
    "$REPO_ROOT/docker/native" \
    "$REPO_ROOT/scripts/build_linux_binaries.sh" \
    "$REPO_ROOT/Cargo.toml" \
    "$REPO_ROOT/Cargo.lock" \
    "$REPO_ROOT/crates/transcription-agent" \
    "$REPO_ROOT/crates/core")"
  current_youtube_agent_fp="${RUSTFIN_RUST_BUILD_MODE_KEY}:${RUSTFIN_RUST_BUILD_PROFILE}:compose=${youtube_agent_compose_hash}:$(compute_scope_fingerprint \
    "$REPO_ROOT/crates/youtube-agent/Dockerfile" \
    "$REPO_ROOT/docker/native" \
    "$REPO_ROOT/scripts/build_linux_binaries.sh" \
    "$REPO_ROOT/Cargo.toml" \
    "$REPO_ROOT/Cargo.lock" \
    "$REPO_ROOT/crates/youtube-agent" \
    "$REPO_ROOT/crates/core")"
  current_ui_fp="compose=${ui_compose_hash}:$(compute_scope_fingerprint "$REPO_ROOT/ui")"
}

persist_secret_env_var() {
  local key="$1"
  local value="$2"
  local file="$3"

  local dir
  dir="$(dirname "$file")"
  mkdir -p "$dir" || die "Failed to create secrets dir: $dir"
  chmod 700 "$dir" 2>/dev/null || true

  local tmp_file="${file}.tmp.$$"
  if [[ -f "$file" ]]; then
    grep -v -E "^${key}=" "$file" > "$tmp_file" || true
  else
    : > "$tmp_file"
  fi
  printf "%s=%q\n" "$key" "$value" >> "$tmp_file"
  mv "$tmp_file" "$file"
  chmod 600 "$file" 2>/dev/null || true
}

# Load prior runtime settings so repeated runs stay stable.
user_backend_port="${RUSTFIN_BACKEND_PORT:-}"
user_ui_port="${RUSTFIN_UI_PORT:-}"
user_media_path="${RUSTFIN_MEDIA_PATH:-}"
user_database_url="${RUSTFIN_DATABASE_URL:-}"
user_legacy_db_path="${RUSTFIN_DB:-}"
user_browser_backend_origin="${RUSTYFIN_BROWSER_BACKEND_ORIGIN:-}"
user_ws_allowed_origins="${RUSTFIN_WS_ALLOWED_ORIGINS:-}"
user_youtube_cookie="${RUSTFIN_YOUTUBE_COOKIE:-}"
youtube_cookie_source="unset"
SECRETS_ENV_FILE_DEFAULT="${HOME:-$REPO_ROOT}/.config/rustfin/secrets.env"
SECRETS_ENV_FILE="${RUSTFIN_SECRETS_ENV_FILE:-$SECRETS_ENV_FILE_DEFAULT}"

if [[ -f "$RUNTIME_ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$RUNTIME_ENV_FILE"
fi

if [[ -f "$SECRETS_ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$SECRETS_ENV_FILE" || true
  if [[ -z "$user_youtube_cookie" && -n "${RUSTFIN_YOUTUBE_COOKIE:-}" ]]; then
    youtube_cookie_source="secrets file"
  fi
fi

# Explicit shell/env values always win over runtime file values.
[[ -n "$user_backend_port" ]] && RUSTFIN_BACKEND_PORT="$user_backend_port"
[[ -n "$user_ui_port" ]] && RUSTFIN_UI_PORT="$user_ui_port"
[[ -n "$user_media_path" ]] && RUSTFIN_MEDIA_PATH="$user_media_path"
[[ -n "$user_database_url" ]] && RUSTFIN_DATABASE_URL="$user_database_url"
[[ -n "$user_browser_backend_origin" ]] && RUSTYFIN_BROWSER_BACKEND_ORIGIN="$user_browser_backend_origin"
[[ -n "$user_ws_allowed_origins" ]] && RUSTFIN_WS_ALLOWED_ORIGINS="$user_ws_allowed_origins"
if [[ -n "$user_youtube_cookie" ]]; then
  RUSTFIN_YOUTUBE_COOKIE="$user_youtube_cookie"
  youtube_cookie_source="shell env"
  persist_secret_env_var "RUSTFIN_YOUTUBE_COOKIE" "$user_youtube_cookie" "$SECRETS_ENV_FILE"
elif [[ -n "${RUSTFIN_YOUTUBE_COOKIE:-}" && "$youtube_cookie_source" == "unset" ]]; then
  youtube_cookie_source="loaded env"
fi

case "$RUSTFIN_RUST_BUILD_PROFILE" in
  ''|*[!A-Za-z0-9_-]*)
    die "Invalid RUSTFIN_RUST_BUILD_PROFILE='$RUSTFIN_RUST_BUILD_PROFILE' (allowed: letters, numbers, _, -)"
    ;;
esac
export RUSTFIN_RUST_BUILD_PROFILE

case "$NATIVE_RUST_BUILD" in
  0|1) ;;
  *) die "Invalid native build toggle '$NATIVE_RUST_BUILD' (expected 0 or 1)" ;;
esac

RUSTFIN_NATIVE_TARGET=""
RUSTFIN_NATIVE_BIN_DIR=""
RUSTFIN_NATIVE_BIN_DIR_ABS=""
RUSTFIN_RUST_BUILD_MODE_KEY="docker-build"
if [[ "$NATIVE_RUST_BUILD" == "1" ]]; then
  RUSTFIN_NATIVE_TARGET="$(resolve_native_linux_target)"
  RUSTFIN_NATIVE_BIN_DIR=".native-bins/${RUSTFIN_NATIVE_TARGET}/${RUSTFIN_RUST_BUILD_PROFILE}"
  RUSTFIN_NATIVE_BIN_DIR_ABS="$REPO_ROOT/${RUSTFIN_NATIVE_BIN_DIR}"
  RUSTFIN_RUST_BUILD_MODE_KEY="native-linux:${RUSTFIN_NATIVE_TARGET}"

  export RUSTFIN_NATIVE_BIN_DIR
  export RUSTFIN_SERVER_DOCKERFILE="docker/native/rustfin-server.Dockerfile"
  export RUSTFIN_CALENDAR_DOCKERFILE="docker/native/rustfin-calendar.Dockerfile"
  export RUSTFIN_TMDB_AGENT_DOCKERFILE="docker/native/rustfin-tmdb-agent.Dockerfile"
  export RUSTFIN_YOUTUBE_AGENT_DOCKERFILE="docker/native/rustfin-youtube-agent.Dockerfile"
  export RUSTFIN_TRANSCRIPTION_AGENT_DOCKERFILE="docker/native/rustfin-transcription-agent.Dockerfile"
fi

# Migrate legacy repo-local default media root from older starts so Browse can
# map typical user-selected folders without extra configuration.
legacy_media_root="$REPO_ROOT/media"
if [[ -z "$user_media_path" && "${RUSTFIN_MEDIA_PATH:-}" == "$legacy_media_root" ]]; then
  RUSTFIN_MEDIA_PATH="$HOME"
fi

backend_locked=false
ui_locked=false
[[ -n "$user_backend_port" ]] && backend_locked=true
[[ -n "$user_ui_port" ]] && ui_locked=true

# Default media path for first-time setup on any machine.
# Use HOME by default so the native picker can map common user folders.
MEDIA_PATH="${RUSTFIN_MEDIA_PATH:-${HOME:-$REPO_ROOT/media}}"
mkdir -p "$MEDIA_PATH" || die "Failed to create media path: $MEDIA_PATH"
# Keep logical path form (e.g. /Users/... on macOS) to match chooser output.
MEDIA_PATH="$(cd "$MEDIA_PATH" && pwd -L)" || die "Failed to resolve media path: $MEDIA_PATH"
[[ -d "$MEDIA_PATH" ]] || die "Resolved media path is not a directory: $MEDIA_PATH"
[[ -r "$MEDIA_PATH" ]] || die "Media path is not readable: $MEDIA_PATH"
[[ -x "$MEDIA_PATH" ]] || die "Media path is not traversable: $MEDIA_PATH"
export RUSTFIN_MEDIA_PATH="$MEDIA_PATH"

# Database target defaulting:
# - Prefer explicit RUSTFIN_DATABASE_URL.
# - If legacy RUSTFIN_DB is explicitly set, keep SQLite path behavior.
# - Otherwise default to local Postgres service in docker-compose.
if [[ -z "${RUSTFIN_DATABASE_URL:-}" ]]; then
  if [[ -n "$user_legacy_db_path" ]]; then
    export RUSTFIN_DATABASE_URL="$user_legacy_db_path"
  else
    pg_user="${RUSTFIN_PG_USER:-rustfin}"
    pg_password="${RUSTFIN_PG_PASSWORD:-rustfin}"
    pg_db="${RUSTFIN_PG_DB:-rustfin}"
    export RUSTFIN_DATABASE_URL="postgresql://${pg_user}:${pg_password}@postgres:5432/${pg_db}"
  fi
fi

db_target_log="$RUSTFIN_DATABASE_URL"
db_mode="sqlite"
db_target_lc="$(printf '%s' "$RUSTFIN_DATABASE_URL" | tr '[:upper:]' '[:lower:]')"
if [[ "$db_target_lc" == postgres://* || "$db_target_lc" == postgresql://* ]]; then
  db_mode="postgres"
  db_target_log="$(printf '%s' "$RUSTFIN_DATABASE_URL" | sed -E 's#(postgres(ql)?://)[^@/]+@#\1<redacted>@#')"
fi

PICKER_HELPER_PORT="${RUSTFIN_PICKER_HELPER_PORT:-43110}"
PICKER_HELPER_HOST="${RUSTFIN_PICKER_HELPER_HOST:-0.0.0.0}"
PICKER_HELPER_PID_FILE="$SAFE_TMP_DIR/directory-picker-helper.pid"
PICKER_HELPER_LOG_FILE="$SAFE_TMP_DIR/directory-picker-helper.log"
PICKER_HELPER_SCRIPT="$SAFE_TMP_DIR/directory-picker-helper.py"

start_directory_picker_helper() {
  local enabled="${RUSTFIN_ENABLE_PICKER_HELPER:-1}"
  if [[ "$enabled" == "0" ]]; then
    warn "Directory picker helper disabled (RUSTFIN_ENABLE_PICKER_HELPER=0)."
    return
  fi

  local py_bin=""
  if command -v python3 >/dev/null 2>&1; then
    py_bin="python3"
  elif command -v python >/dev/null 2>&1; then
    py_bin="python"
  else
    warn "Python not found; native host directory picker helper not started."
    return
  fi

  if command -v curl >/dev/null 2>&1; then
    if curl -fsS "http://127.0.0.1:${PICKER_HELPER_PORT}/health" >/dev/null 2>&1; then
      info "Directory picker helper already running on port ${PICKER_HELPER_PORT}."
      return
    fi
  fi

  if [[ -f "$PICKER_HELPER_PID_FILE" ]]; then
    local existing_pid
    existing_pid="$(cat "$PICKER_HELPER_PID_FILE" 2>/dev/null || true)"
    if [[ -n "$existing_pid" ]] && kill -0 "$existing_pid" 2>/dev/null; then
      info "Directory picker helper already running (pid $existing_pid)."
      return
    fi
    rm -f "$PICKER_HELPER_PID_FILE"
  fi

  cat > "$PICKER_HELPER_SCRIPT" <<'PY'
#!/usr/bin/env python3
import json
import os
import platform
import shutil
import subprocess
from http.server import BaseHTTPRequestHandler, HTTPServer

HOST = os.environ.get("RUSTFIN_PICKER_HELPER_HOST", "0.0.0.0")
PORT = int(os.environ.get("RUSTFIN_PICKER_HELPER_PORT", "43110"))

def pick_directory():
    system = platform.system()
    if system == "Darwin":
        script = 'set chosenFolder to choose folder with prompt "Select a media directory for Rustyfin"\nPOSIX path of chosenFolder'
        out = subprocess.run(["osascript", "-e", script], capture_output=True, text=True)
        if out.returncode == 0:
            return out.stdout.strip()
        err = (out.stderr or "").strip()
        if "User canceled" in err or "(-128)" in err:
            return ""
        raise RuntimeError(err or "folder picker failed")

    if system == "Linux":
        if shutil.which("zenity"):
            out = subprocess.run(
                ["zenity", "--file-selection", "--directory", "--title=Select a media directory for Rustyfin"],
                capture_output=True,
                text=True,
            )
            if out.returncode == 0:
                return (out.stdout or "").strip()
            if out.returncode == 1:
                return ""
            raise RuntimeError((out.stderr or "").strip() or "zenity folder picker failed")
        if shutil.which("kdialog"):
            out = subprocess.run(
                ["kdialog", "--getexistingdirectory", ".", "Select a media directory for Rustyfin"],
                capture_output=True,
                text=True,
            )
            if out.returncode == 0:
                return (out.stdout or "").strip()
            if out.returncode == 1:
                return ""
            raise RuntimeError((out.stderr or "").strip() or "kdialog folder picker failed")
        raise RuntimeError("no supported Linux picker found (install zenity or kdialog)")

    if system == "Windows":
        ps_script = r"""
Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.FolderBrowserDialog
$dialog.Description = 'Select a media directory for Rustyfin'
$result = $dialog.ShowDialog()
if ($result -eq [System.Windows.Forms.DialogResult]::OK) {
  Write-Output $dialog.SelectedPath
}
"""
        out = subprocess.run(
            ["powershell", "-NoProfile", "-NonInteractive", "-Command", ps_script],
            capture_output=True,
            text=True,
        )
        if out.returncode == 0:
            return (out.stdout or "").strip()
        raise RuntimeError((out.stderr or "").strip() or "PowerShell folder picker failed")

    raise RuntimeError(f"unsupported host OS for picker helper: {system}")

class Handler(BaseHTTPRequestHandler):
    def _write_json(self, status, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == "/health":
            self._write_json(200, {"ok": True})
        else:
            self._write_json(404, {"error": "not found"})

    def do_POST(self):
        if self.path != "/pick":
            self._write_json(404, {"error": "not found"})
            return
        try:
            selected = pick_directory()
            if not selected:
                self._write_json(400, {"error": "directory selection cancelled"})
                return
            self._write_json(200, {"path": selected})
        except Exception as exc:
            self._write_json(500, {"error": str(exc)})

    def log_message(self, format, *args):
        return

def main():
    server = HTTPServer((HOST, PORT), Handler)
    server.serve_forever()

if __name__ == "__main__":
    main()
PY
  chmod 700 "$PICKER_HELPER_SCRIPT"

  nohup env RUSTFIN_PICKER_HELPER_PORT="$PICKER_HELPER_PORT" \
    RUSTFIN_PICKER_HELPER_HOST="$PICKER_HELPER_HOST" \
    "$py_bin" "$PICKER_HELPER_SCRIPT" </dev/null >>"$PICKER_HELPER_LOG_FILE" 2>&1 &
  local helper_pid=$!
  echo "$helper_pid" > "$PICKER_HELPER_PID_FILE"

  if command -v curl >/dev/null 2>&1; then
    for _ in $(seq 1 20); do
      if curl -fsS "http://127.0.0.1:${PICKER_HELPER_PORT}/health" >/dev/null 2>&1; then
        info "Directory picker helper started on http://127.0.0.1:${PICKER_HELPER_PORT} (pid $helper_pid)"
        return
      fi
      sleep 0.2
    done
    warn "Directory picker helper did not report healthy; check: $PICKER_HELPER_LOG_FILE"
  else
    info "Directory picker helper started (pid $helper_pid)"
  fi
}

start_directory_picker_helper

export RUSTFIN_PICKER_HELPER_PORT="$PICKER_HELPER_PORT"
export RUSTFIN_DIRECTORY_PICKER_HELPER_URL="${RUSTFIN_DIRECTORY_PICKER_HELPER_URL:-http://host.docker.internal:${PICKER_HELPER_PORT}/pick}"
export RUSTFIN_MEDIA_HOST_PATH="${RUSTFIN_MEDIA_HOST_PATH:-$RUSTFIN_MEDIA_PATH}"
export RUSTFIN_MEDIA_CONTAINER_ROOT="${RUSTFIN_MEDIA_CONTAINER_ROOT:-$RUSTFIN_MEDIA_PATH}"
export RUSTFIN_HOST_OS="$(uname -s | tr '[:upper:]' '[:lower:]')"

is_port_in_use() {
  local port="$1"
  if command -v lsof >/dev/null 2>&1; then
    lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1
  else
    return 1
  fi
}

pick_free_port() {
  local preferred="$1"
  local max_hops="${2:-200}"
  local p="$preferred"
  local hops=0
  while is_port_in_use "$p"; do
    p=$((p + 1))
    hops=$((hops + 1))
    if (( hops > max_hops )); then
      die "Unable to find a free port near $preferred"
    fi
  done
  echo "$p"
}

detect_primary_lan_ipv4() {
  local uname_s
  uname_s="$(uname -s 2>/dev/null || echo "")"
  local ip=""

  case "$uname_s" in
    Darwin)
      local iface
      iface="$(route -n get default 2>/dev/null | awk '/interface:/{print $2; exit}')"
      if [[ -n "$iface" ]]; then
        ip="$(ipconfig getifaddr "$iface" 2>/dev/null || true)"
      fi
      if [[ -z "$ip" ]]; then
        ip="$(ipconfig getifaddr en0 2>/dev/null || true)"
      fi
      if [[ -z "$ip" ]]; then
        ip="$(ipconfig getifaddr en1 2>/dev/null || true)"
      fi
      ;;
    Linux)
      ip="$(ip route get 1.1.1.1 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="src"){print $(i+1); exit}}')"
      ;;
    *)
      ;;
  esac

  if [[ -z "$ip" || "$ip" == 127.* ]]; then
    return 1
  fi

  echo "$ip"
}

is_ipv4() {
  local value="$1"
  [[ "$value" =~ ^([0-9]{1,3}\.){3}[0-9]{1,3}$ ]]
}

ensure_edge_tls_cert() {
  local host="$1"
  local cert_dir="$SAFE_TMP_DIR/edge-tls"
  local cert_path="$cert_dir/tls.crt"
  local key_path="$cert_dir/tls.key"
  local meta_path="$cert_dir/meta.host"

  mkdir -p "$cert_dir" || die "Failed to create TLS cert dir: $cert_dir"
  chmod 700 "$cert_dir" 2>/dev/null || true

  local need_regen=false
  if [[ ! -f "$cert_path" || ! -f "$key_path" ]]; then
    need_regen=true
  elif [[ ! -f "$meta_path" || "$(cat "$meta_path" 2>/dev/null || true)" != "$host" ]]; then
    need_regen=true
  fi

  if [[ "$need_regen" == "false" ]]; then
    export RUSTFIN_EDGE_TLS_CERT="$cert_path"
    export RUSTFIN_EDGE_TLS_KEY="$key_path"
    return
  fi

  command -v openssl >/dev/null 2>&1 || die "openssl is required to generate local TLS certificates"

  local san="DNS:localhost,IP:127.0.0.1"
  if is_ipv4 "$host"; then
    san="${san},IP:${host}"
  else
    san="${san},DNS:${host}"
  fi

  rm -f "$cert_path" "$key_path"
  openssl req \
    -x509 \
    -newkey rsa:2048 \
    -sha256 \
    -days 365 \
    -nodes \
    -keyout "$key_path" \
    -out "$cert_path" \
    -subj "/CN=${host}" \
    -addext "subjectAltName=${san}" >/dev/null 2>&1 || die "Failed generating local TLS cert"

  chmod 600 "$key_path" "$cert_path" 2>/dev/null || true
  printf "%s" "$host" > "$meta_path"
  chmod 600 "$meta_path" 2>/dev/null || true

  export RUSTFIN_EDGE_TLS_CERT="$cert_path"
  export RUSTFIN_EDGE_TLS_KEY="$key_path"
}

compose_has_service() {
  local service="$1"
  docker compose -f "$COMPOSE_FILE" config --services 2>/dev/null | grep -Fxq "$service"
}

wait_for_service_ready() {
  local service="$1"
  local timeout_seconds="$2"
  local start_ts now container_id health_mode status
  start_ts="$(date +%s)"

  while true; do
    container_id="$(docker compose -f "$COMPOSE_FILE" ps -q "$service" 2>/dev/null || true)"
    if [[ -n "$container_id" ]]; then
      break
    fi
    now="$(date +%s)"
    if (( now - start_ts >= timeout_seconds )); then
      warn "Timed out waiting for container id for service '$service'."
      return 1
    fi
    sleep 1
  done

  health_mode="$(docker inspect --format '{{if .Config.Healthcheck}}health{{else}}state{{end}}' "$container_id" 2>/dev/null || echo "state")"
  while true; do
    if [[ "$health_mode" == "health" ]]; then
      status="$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}unknown{{end}}' "$container_id" 2>/dev/null || echo "unknown")"
      case "$status" in
        healthy)
          info "Service ready: $service (healthy)"
          return 0
          ;;
        unhealthy)
          warn "Service unhealthy: $service"
          return 1
          ;;
      esac
    else
      status="$(docker inspect --format '{{.State.Status}}' "$container_id" 2>/dev/null || echo "unknown")"
      case "$status" in
        running)
          info "Service ready: $service (running)"
          return 0
          ;;
        exited|dead)
          warn "Service failed to stay up: $service ($status)"
          return 1
          ;;
      esac
    fi

    now="$(date +%s)"
    if (( now - start_ts >= timeout_seconds )); then
      warn "Timed out waiting for service '$service' to become ready (last status: ${status})."
      return 1
    fi
    sleep 1
  done
}

wait_for_critical_services() {
  local timeout_seconds="${1:-120}"
  local service=""
  local failures=0
  local critical_services=(
    postgres
    rustfin
    rustfin-calendar
    rustfin-tmdb-agent
    rustfin-youtube-agent
    rustfin-transcription-agent
    rustfin-ui
    rustfin-edge
  )

  info "Waiting for critical services to become ready..."
  for service in "${critical_services[@]}"; do
    if ! compose_has_service "$service"; then
      continue
    fi
    if ! wait_for_service_ready "$service" "$timeout_seconds"; then
      failures=$((failures + 1))
    fi
  done

  if (( failures > 0 )); then
    warn "One or more services did not report ready."
    warn "Inspect logs with: docker compose -f \"$COMPOSE_FILE\" logs -f"
    return 1
  fi

  return 0
}

project_running=false
if docker compose -f "$COMPOSE_FILE" ps --status running -q 2>/dev/null | grep -q .; then
  project_running=true
fi

backend_port="${RUSTFIN_BACKEND_PORT:-8096}"
ui_port="${RUSTFIN_UI_PORT:-3000}"

# If stack is not currently running, choose free ports unless user explicitly
# locked the ports via environment variables.
if [[ "$backend_locked" == "false" && "$project_running" == "false" ]]; then
  backend_selected="$(pick_free_port "$backend_port")"
  if [[ "$backend_selected" != "$backend_port" ]]; then
    warn "Port $backend_port is busy; using backend port $backend_selected"
  fi
  backend_port="$backend_selected"
fi

if [[ "$ui_locked" == "false" && "$project_running" == "false" ]]; then
  ui_selected="$(pick_free_port "$ui_port")"
  if [[ "$ui_selected" != "$ui_port" ]]; then
    warn "Port $ui_port is busy; using UI port $ui_selected"
  fi
  ui_port="$ui_selected"
fi

export RUSTFIN_BACKEND_PORT="$backend_port"
export RUSTFIN_UI_PORT="$ui_port"

public_host="${RUSTFIN_PUBLIC_HOST:-}"
if [[ -z "$public_host" ]]; then
  if detected_lan_ip="$(detect_primary_lan_ipv4 2>/dev/null)"; then
    public_host="$detected_lan_ip"
  else
    public_host="localhost"
  fi
fi
export RUSTFIN_PUBLIC_HOST="$public_host"
ensure_edge_tls_cert "$public_host"

if [[ -n "$user_browser_backend_origin" ]]; then
  export RUSTYFIN_BROWSER_BACKEND_ORIGIN="$user_browser_backend_origin"
else
  export RUSTYFIN_BROWSER_BACKEND_ORIGIN="http://${public_host}:${RUSTFIN_BACKEND_PORT}"
fi

if [[ -n "$user_ws_allowed_origins" ]]; then
  export RUSTFIN_WS_ALLOWED_ORIGINS="$user_ws_allowed_origins"
else
  ws_origins=(
    "http://localhost:${RUSTFIN_UI_PORT}"
    "http://127.0.0.1:${RUSTFIN_UI_PORT}"
    "https://localhost:${RUSTFIN_UI_PORT}"
    "https://127.0.0.1:${RUSTFIN_UI_PORT}"
  )
  if [[ "$public_host" != "localhost" && "$public_host" != "127.0.0.1" ]]; then
    ws_origins+=(
      "http://${public_host}:${RUSTFIN_UI_PORT}"
      "https://${public_host}:${RUSTFIN_UI_PORT}"
    )
  fi
  export RUSTFIN_WS_ALLOWED_ORIGINS="$(IFS=,; echo "${ws_origins[*]}")"
fi

info "Using TMPDIR: $TMPDIR"
info "Using media path: $RUSTFIN_MEDIA_PATH"
info "Backend port: $RUSTFIN_BACKEND_PORT"
info "UI port: $RUSTFIN_UI_PORT"
info "Public host: $public_host"
info "Browser backend origin: $RUSTYFIN_BROWSER_BACKEND_ORIGIN"
info "WebSocket allowed origins: $RUSTFIN_WS_ALLOWED_ORIGINS"
info "UI transport: HTTPS (secure context for microphone/WebRTC on LAN)"
info "Edge TLS cert: $RUSTFIN_EDGE_TLS_CERT"
info "Database mode: ${db_mode}"
info "Database target: ${db_target_log}"
info "Rust build profile: $RUSTFIN_RUST_BUILD_PROFILE"
if [[ "$NATIVE_RUST_BUILD" == "1" ]]; then
  info "Rust binary build mode: native host cross-compile -> Docker copy"
  info "Native Linux target: $RUSTFIN_NATIVE_TARGET"
  info "Native binary output dir: $RUSTFIN_NATIVE_BIN_DIR_ABS"
else
  warn "Rust binary build mode: Docker builder stages (--docker-rust-build)"
fi
if [[ "$BUILD" == "true" ]]; then
  if [[ "$NO_CACHE_BUILD" == "true" ]]; then
    info "Build mode: full rebuild (no Docker cache)"
  else
    info "Build mode: smart rebuild (Docker cache enabled, changed services only)"
  fi
else
  warn "Build mode: skipped (--no-build)"
fi
if [[ -n "${RUSTFIN_TMDB_KEY:-}" ]]; then
  info "TMDB metadata enrichment: enabled"
else
  warn "TMDB metadata enrichment disabled (set RUSTFIN_TMDB_KEY to fetch online posters/metadata)"
fi
if [[ -n "${RUSTFIN_YOUTUBE_COOKIE:-}" ]]; then
  info "YouTube online-audio cookie: configured (${youtube_cookie_source})"
else
  warn "YouTube online-audio cookie is not configured (export RUSTFIN_YOUTUBE_COOKIE once; start.sh will persist and auto-load it from ${SECRETS_ENV_FILE})"
fi

if [[ "$BUILD" == "true" ]]; then
  if [[ "$NO_CACHE_BUILD" == "true" ]]; then
    if [[ "$NATIVE_RUST_BUILD" == "1" ]]; then
      build_native_rust_bins \
        "$RUSTFIN_NATIVE_TARGET" \
        "$RUSTFIN_NATIVE_BIN_DIR_ABS" \
        rustfin \
        rustfin-calendar \
        rustfin-tmdb-agent \
        rustfin-transcription-agent \
        rustfin-youtube-agent
    fi

    info "Rebuilding all Docker images..."
    if ! docker compose -f "$COMPOSE_FILE" build --pull --no-cache; then
      warn "Full no-cache rebuild failed (likely transient network issue). Retrying once with Docker cache."
      if ! docker compose -f "$COMPOSE_FILE" build --pull; then
        die "Docker image rebuild failed after retry. Check your internet connection and retry."
      fi
    fi

    compute_all_service_fingerprints
    save_build_fingerprints \
      "$current_rustfin_fp" \
      "$current_calendar_fp" \
      "$current_tmdb_agent_fp" \
      "$current_transcription_agent_fp" \
      "$current_youtube_agent_fp" \
      "$current_ui_fp"
  else
    prev_rustfin_fp=""
    prev_calendar_fp=""
    prev_tmdb_agent_fp=""
    prev_transcription_agent_fp=""
    prev_youtube_agent_fp=""
    prev_ui_fp=""
    BUILD_STATE_EXISTS=false
    if [[ -f "$BUILD_STATE_FILE" ]]; then
      BUILD_STATE_EXISTS=true
      # shellcheck disable=SC1090
      source "$BUILD_STATE_FILE" || true
      prev_rustfin_fp="${RUSTFIN_FP:-}"
      prev_calendar_fp="${CALENDAR_FP:-}"
      prev_tmdb_agent_fp="${TMDB_AGENT_FP:-}"
      prev_transcription_agent_fp="${TRANSCRIPTION_AGENT_FP:-}"
      prev_youtube_agent_fp="${YOUTUBE_AGENT_FP:-}"
      prev_ui_fp="${UI_FP:-}"
    fi

    compute_all_service_fingerprints

    changed_services=()
    changed_reasons=()

    rustfin_reason="$(service_build_reason "rustfin" "$prev_rustfin_fp" "$current_rustfin_fp" || true)"
    if [[ -n "$rustfin_reason" ]]; then
      changed_services+=("rustfin")
      changed_reasons+=("rustfin (${rustfin_reason})")
    fi

    calendar_reason="$(service_build_reason "rustfin-calendar" "$prev_calendar_fp" "$current_calendar_fp" || true)"
    if [[ -n "$calendar_reason" ]]; then
      changed_services+=("rustfin-calendar")
      changed_reasons+=("rustfin-calendar (${calendar_reason})")
    fi

    tmdb_agent_reason="$(service_build_reason "rustfin-tmdb-agent" "$prev_tmdb_agent_fp" "$current_tmdb_agent_fp" || true)"
    if [[ -n "$tmdb_agent_reason" ]]; then
      changed_services+=("rustfin-tmdb-agent")
      changed_reasons+=("rustfin-tmdb-agent (${tmdb_agent_reason})")
    fi

    transcription_agent_reason="$(service_build_reason "rustfin-transcription-agent" "$prev_transcription_agent_fp" "$current_transcription_agent_fp" || true)"
    if [[ -n "$transcription_agent_reason" ]]; then
      changed_services+=("rustfin-transcription-agent")
      changed_reasons+=("rustfin-transcription-agent (${transcription_agent_reason})")
    fi

    youtube_agent_reason="$(service_build_reason "rustfin-youtube-agent" "$prev_youtube_agent_fp" "$current_youtube_agent_fp" || true)"
    if [[ -n "$youtube_agent_reason" ]]; then
      changed_services+=("rustfin-youtube-agent")
      changed_reasons+=("rustfin-youtube-agent (${youtube_agent_reason})")
    fi

    ui_reason="$(service_build_reason "rustfin-ui" "$prev_ui_fp" "$current_ui_fp" || true)"
    if [[ -n "$ui_reason" ]]; then
      changed_services+=("rustfin-ui")
      changed_reasons+=("rustfin-ui (${ui_reason})")
    fi

    if [[ ${#changed_services[@]} -eq 0 ]]; then
      info "No build-scope changes detected since previous build; reusing existing images."
    else
      info "Rebuilding changed Docker services: ${changed_services[*]}"
      for reason in "${changed_reasons[@]}"; do
        info " - ${reason}"
      done
      if [[ "$NATIVE_RUST_BUILD" == "1" ]]; then
        native_rust_changed_services=()
        for service in "${changed_services[@]}"; do
          case "$service" in
            rustfin|rustfin-calendar|rustfin-tmdb-agent|rustfin-transcription-agent|rustfin-youtube-agent)
              native_rust_changed_services+=("$service")
              ;;
          esac
        done
        if [[ ${#native_rust_changed_services[@]} -gt 0 ]]; then
          build_native_rust_bins \
            "$RUSTFIN_NATIVE_TARGET" \
            "$RUSTFIN_NATIVE_BIN_DIR_ABS" \
            "${native_rust_changed_services[@]}"
        fi
      fi
      if ! docker compose -f "$COMPOSE_FILE" build --pull "${changed_services[@]}"; then
        die "Docker image rebuild failed. Check your internet connection and retry."
      fi
    fi

    save_build_fingerprints \
      "$current_rustfin_fp" \
      "$current_calendar_fp" \
      "$current_tmdb_agent_fp" \
      "$current_transcription_agent_fp" \
      "$current_youtube_agent_fp" \
      "$current_ui_fp"
  fi
fi

compose_args=(up --remove-orphans)
if [[ "$DETACH" == "true" ]]; then
  compose_args+=(-d)
fi

docker compose -f "$COMPOSE_FILE" "${compose_args[@]}"

{
  echo "# Generated by scripts/start.sh"
  printf "RUSTFIN_BACKEND_PORT=%q\n" "$RUSTFIN_BACKEND_PORT"
  printf "RUSTFIN_UI_PORT=%q\n" "$RUSTFIN_UI_PORT"
  printf "RUSTFIN_MEDIA_PATH=%q\n" "$RUSTFIN_MEDIA_PATH"
  printf "RUSTYFIN_BROWSER_BACKEND_ORIGIN=%q\n" "$RUSTYFIN_BROWSER_BACKEND_ORIGIN"
  printf "RUSTFIN_WS_ALLOWED_ORIGINS=%q\n" "$RUSTFIN_WS_ALLOWED_ORIGINS"
} > "$RUNTIME_ENV_FILE"
chmod 600 "$RUNTIME_ENV_FILE" 2>/dev/null || true

if [[ "$DETACH" == "true" && "$HEALTH_CHECK" == "true" ]]; then
  wait_for_critical_services 120 || true

  if command -v curl >/dev/null 2>&1; then
    info "Waiting for backend health endpoint..."
    ok=false
    for _ in $(seq 1 60); do
      if curl -fsS "http://127.0.0.1:${RUSTFIN_BACKEND_PORT}/health" >/dev/null 2>&1; then
        ok=true
        break
      fi
      sleep 1
    done
    if [[ "$ok" != "true" ]]; then
      warn "Backend health check did not pass within 60s."
      warn "Check logs with: docker compose -f \"$COMPOSE_FILE\" logs -f"
    fi
  else
    warn "curl is not installed; skipping host-level backend health check."
  fi
fi

success "Rustyfin stack is up."
echo "  Backend: http://localhost:${RUSTFIN_BACKEND_PORT}"
echo "  UI:      https://localhost:${RUSTFIN_UI_PORT}"
if [[ "$public_host" != "localhost" && "$public_host" != "127.0.0.1" ]]; then
  echo "  Backend (LAN): http://${public_host}:${RUSTFIN_BACKEND_PORT}"
  echo "  UI (LAN):      https://${public_host}:${RUSTFIN_UI_PORT}"
fi
echo "  Note: if your browser warns about a local certificate, accept/trust it to enable microphone access."
