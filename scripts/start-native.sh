#!/usr/bin/env bash
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
RESET='\033[0m'

info()    { echo -e "${CYAN}[start-native]${RESET} $*"; }
success() { echo -e "${GREEN}[start-native]${RESET} $*"; }
warn()    { echo -e "${YELLOW}[start-native]${RESET} $*"; }
die()     { echo -e "${RED}[start-native] ERROR:${RESET} $*" >&2; exit 1; }

usage() {
  cat <<'EOF'
Usage:
  ./scripts/start-native.sh [--no-build] [--build-only] [--foreground] [--no-health-check]

Options:
  --no-build         Skip Rust/UI build and reuse existing native artifacts.
  --build-only       Build native artifacts only; do not launch the runtime.
  --foreground       Run in attached mode and tail native runtime logs.
  --no-health-check  Skip startup health waits.
  -h, --help         Show this help.

Environment:
  RUSTFIN_RUST_BUILD_PROFILE            Cargo profile for native host build (default: dev)
  RUSTFIN_BACKEND_PORT                  Backend bind port (default: 8096)
  RUSTFIN_CALENDAR_PORT                 Calendar bind port (default: 8099)
  RUSTFIN_TMDB_AGENT_PORT               TMDB agent port (default: 8100)
  RUSTFIN_YOUTUBE_AGENT_PORT            YouTube agent port (default: 8101)
  RUSTFIN_TRANSCRIPTION_AGENT_PORT      Transcription agent port (default: 8102)
  RUSTFIN_SERVERS_AGENT_PORT            Servers agent port (default: 8103)
  RUSTFIN_UI_INTERNAL_PORT              Internal Next standalone port (default: 3001)
  RUSTFIN_UI_PORT                       HTTPS edge port (default: 3000)
  RUSTFIN_DATABASE_URL                  PostgreSQL URL (default: postgresql://rustfin:rustfin@127.0.0.1:5432/rustfin)
  RUSTFIN_JWT_SECRET                    Stable JWT signing secret for persistent sessions
  RUSTFIN_MEDIA_PATH                    Host media root (default: $HOME)
  RUSTFIN_ENABLE_SERVERS_AGENT          Start rustfin-servers-agent (default: 1)
  RUSTFIN_AI_GPU_BACKEND                AI inference backend: auto|disabled|cpu|cuda|rocm|vulkan (default: auto)
  RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES
                                        Cargo features for transcription agent (default: gpu-opencl)
EOF
}

BUILD=true
BUILD_ONLY=false
DETACH=true
HEALTH_CHECK=true

while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-build) BUILD=false; shift ;;
    --build-only) BUILD_ONLY=true; shift ;;
    --foreground) DETACH=false; shift ;;
    --no-health-check) HEALTH_CHECK=false; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "Unknown argument: $1" ;;
  esac
done

if [[ "$BUILD_ONLY" == "true" && "$BUILD" == "false" ]]; then
  die "--build-only cannot be combined with --no-build"
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

REPO_OWNER_USER="$(stat -c %U "$REPO_ROOT" 2>/dev/null || true)"
REPO_OWNER_GROUP="$(stat -c %G "$REPO_ROOT" 2>/dev/null || true)"
REPO_OWNER_HOME="$(getent passwd "${REPO_OWNER_USER:-}" | cut -d: -f6 || true)"

repair_repo_owner_paths() {
  if [[ "$(id -u)" -ne 0 ]] || [[ -z "$REPO_OWNER_USER" ]] || [[ "$REPO_OWNER_USER" == "root" ]]; then
    return
  fi

  local owner_spec="${REPO_OWNER_USER}:${REPO_OWNER_GROUP:-$REPO_OWNER_USER}"
  local path
  for path in \
    "$REPO_ROOT/target" \
    "$REPO_ROOT/.tmp" \
    "$REPO_ROOT/.native-bins" \
    "$REPO_ROOT/ui/.next" \
    "$REPO_ROOT/ui/node_modules"
  do
    [[ -e "$path" ]] || continue
    chown -R "$owner_spec" "$path"
  done
}

run_installer() {
  if [[ "$(id -u)" -eq 0 ]] && [[ -n "$REPO_OWNER_USER" ]] && [[ "$REPO_OWNER_USER" != "root" ]] && [[ -n "$REPO_OWNER_HOME" ]]; then
    repair_repo_owner_paths
    env \
      -u RUSTUP_HOME \
      -u CARGO_HOME \
      HOME="$REPO_OWNER_HOME" \
      USER="$REPO_OWNER_USER" \
      LOGNAME="$REPO_OWNER_USER" \
      PATH="$REPO_OWNER_HOME/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
      runuser -u "$REPO_OWNER_USER" -- "$REPO_ROOT/scripts/rustfin-installer.sh" "$@"
    return
  fi
  "$REPO_ROOT/scripts/rustfin-installer.sh" "$@"
}

[[ "$(uname -s)" == "Linux" ]] || die "Native runtime is supported on Linux hosts only. Use ./scripts/install_linux.sh on Debian 12/13 or Ubuntu 22.04/24.04."

if ! command -v cargo >/dev/null 2>&1 && [[ -f "${HOME}/.cargo/env" ]]; then
  # shellcheck disable=SC1090
  source "${HOME}/.cargo/env"
fi

command -v caddy >/dev/null 2>&1 || die "caddy is not installed. Run ./scripts/install_linux.sh first."
command -v node >/dev/null 2>&1 || die "node is not installed. Run ./scripts/install_linux.sh first."
command -v curl >/dev/null 2>&1 || die "curl is required for native runtime startup."
command -v openssl >/dev/null 2>&1 || die "openssl is required for native runtime startup."
command -v ffmpeg >/dev/null 2>&1 || die "ffmpeg is required for playback/transcoding."
command -v ffprobe >/dev/null 2>&1 || die "ffprobe is required for media probing."

if [[ "$BUILD" == "true" ]]; then
  command -v cargo >/dev/null 2>&1 || die "cargo is not installed. Run ./scripts/install_linux.sh first."
  command -v rustc >/dev/null 2>&1 || die "rustc is not installed. Run ./scripts/install_linux.sh first."
  command -v npm >/dev/null 2>&1 || die "npm is not installed. Run ./scripts/install_linux.sh first."
fi

user_enable_servers_agent="${RUSTFIN_ENABLE_SERVERS_AGENT-}"
user_ai_gpu_backend="${RUSTFIN_AI_GPU_BACKEND-}"
user_transcoder_hw_accel="${RUSTFIN_TRANSCODER_HW_ACCEL-}"
user_transcoder_require_hw_accel="${RUSTFIN_TRANSCODER_REQUIRE_HW_ACCEL-}"
user_transcode_idle_timeout_secs="${RUSTFIN_TRANSCODE_IDLE_TIMEOUT_SECS-}"
user_stream_token_ttl_seconds="${RUSTFIN_STREAM_TOKEN_TTL_SECONDS-}"
user_transcription_gpu_mode="${RUSTFIN_TRANSCRIPTION_GPU_MODE-}"
user_transcription_require_gpu="${RUSTFIN_TRANSCRIPTION_REQUIRE_GPU-}"
user_transcription_agent_cargo_features="${RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES-}"

RUSTFIN_RUST_BUILD_PROFILE="${RUSTFIN_RUST_BUILD_PROFILE:-dev}"
RUSTFIN_ENABLE_SERVERS_AGENT="${RUSTFIN_ENABLE_SERVERS_AGENT:-1}"
RUSTFIN_AI_GPU_BACKEND="${RUSTFIN_AI_GPU_BACKEND:-auto}"
RUSTFIN_TRANSCRIPTION_GPU_MODE="${RUSTFIN_TRANSCRIPTION_GPU_MODE:-opencl}"
RUSTFIN_TRANSCRIPTION_REQUIRE_GPU="${RUSTFIN_TRANSCRIPTION_REQUIRE_GPU:-0}"
RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES="${RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES:-gpu-opencl}"
RUSTFIN_TRANSCODER_HW_ACCEL="${RUSTFIN_TRANSCODER_HW_ACCEL:-auto}"
RUSTFIN_TRANSCODER_REQUIRE_HW_ACCEL="${RUSTFIN_TRANSCODER_REQUIRE_HW_ACCEL:-0}"
RUSTFIN_TRANSCODE_IDLE_TIMEOUT_SECS="${RUSTFIN_TRANSCODE_IDLE_TIMEOUT_SECS:-1800}"
RUSTFIN_STREAM_TOKEN_TTL_SECONDS="${RUSTFIN_STREAM_TOKEN_TTL_SECONDS:-21600}"

SAFE_TMP_DIR="${RUSTFIN_TMPDIR:-$REPO_ROOT/.tmp}"
mkdir -p "$SAFE_TMP_DIR" || die "Failed to create temp dir: $SAFE_TMP_DIR"
chmod 700 "$SAFE_TMP_DIR" 2>/dev/null || true
[[ -w "$SAFE_TMP_DIR" ]] || die "Temp dir is not writable: $SAFE_TMP_DIR"
export TMPDIR="$SAFE_TMP_DIR"

recompute_runtime_dirs() {
  RUNTIME_ROOT="${RUSTFIN_NATIVE_RUNTIME_DIR:-$SAFE_TMP_DIR/native-runtime}"
  PID_DIR="$RUNTIME_ROOT/pids"
  LOG_DIR="$RUNTIME_ROOT/logs"
  CACHE_DIR="$RUNTIME_ROOT/cache"
  CONFIG_DIR="$RUNTIME_ROOT/config"
  TRANSCODE_DIR="$RUNTIME_ROOT/transcode"
}

RUNTIME_ENV_FILE="$REPO_ROOT/.rustyfin.runtime.env"
INSTALL_DEFAULTS_FILE="/etc/rustyfin/native-runtime.defaults.sh"
BUILD_STATE_FILE="$SAFE_TMP_DIR/native-ui-deps.hash"

user_backend_port="${RUSTFIN_BACKEND_PORT:-}"
user_calendar_port="${RUSTFIN_CALENDAR_PORT:-}"
user_tmdb_port="${RUSTFIN_TMDB_AGENT_PORT:-}"
user_youtube_port="${RUSTFIN_YOUTUBE_AGENT_PORT:-}"
user_transcription_port="${RUSTFIN_TRANSCRIPTION_AGENT_PORT:-}"
user_servers_agent_port="${RUSTFIN_SERVERS_AGENT_PORT:-}"
user_ui_internal_port="${RUSTFIN_UI_INTERNAL_PORT:-}"
user_ui_port="${RUSTFIN_UI_PORT:-}"
user_browser_backend_origin="${RUSTYFIN_BROWSER_BACKEND_ORIGIN:-}"
user_ws_allowed_origins="${RUSTFIN_WS_ALLOWED_ORIGINS:-}"
user_webrtc_ice_servers_json="${RUSTFIN_WEBRTC_ICE_SERVERS_JSON:-}"
user_webrtc_stun_url="${RUSTFIN_WEBRTC_STUN_URL:-}"
user_webrtc_turn_url="${RUSTFIN_WEBRTC_TURN_URL:-}"
user_webrtc_turn_urls="${RUSTFIN_WEBRTC_TURN_URLS:-}"
user_webrtc_turn_username="${RUSTFIN_WEBRTC_TURN_USERNAME:-}"
user_webrtc_turn_credential="${RUSTFIN_WEBRTC_TURN_CREDENTIAL:-}"
user_public_host="${RUSTFIN_PUBLIC_HOST:-}"
user_media_path="${RUSTFIN_MEDIA_PATH:-}"
user_database_url="${RUSTFIN_DATABASE_URL:-}"
user_jwt_secret="${RUSTFIN_JWT_SECRET:-}"
if [[ -f "$INSTALL_DEFAULTS_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$INSTALL_DEFAULTS_FILE" || true
fi

if [[ -f "$RUNTIME_ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$RUNTIME_ENV_FILE" || true
fi

recompute_runtime_dirs
mkdir -p "$PID_DIR" "$LOG_DIR" "$CACHE_DIR" "$CONFIG_DIR" "$TRANSCODE_DIR"

if [[ -z "${RUSTFIN_SERVERS_DEFAULT_JAVA:-}" ]] && [[ -x /opt/rustyfin/java/current/bin/java ]]; then
  export RUSTFIN_SERVERS_DEFAULT_JAVA=/opt/rustyfin/java/current/bin/java
fi

[[ -n "$user_enable_servers_agent" ]] && RUSTFIN_ENABLE_SERVERS_AGENT="$user_enable_servers_agent"
[[ -n "$user_backend_port" ]] && RUSTFIN_BACKEND_PORT="$user_backend_port"
[[ -n "$user_calendar_port" ]] && RUSTFIN_CALENDAR_PORT="$user_calendar_port"
[[ -n "$user_tmdb_port" ]] && RUSTFIN_TMDB_AGENT_PORT="$user_tmdb_port"
[[ -n "$user_youtube_port" ]] && RUSTFIN_YOUTUBE_AGENT_PORT="$user_youtube_port"
[[ -n "$user_transcription_port" ]] && RUSTFIN_TRANSCRIPTION_AGENT_PORT="$user_transcription_port"
[[ -n "$user_servers_agent_port" ]] && RUSTFIN_SERVERS_AGENT_PORT="$user_servers_agent_port"
[[ -n "$user_ui_internal_port" ]] && RUSTFIN_UI_INTERNAL_PORT="$user_ui_internal_port"
[[ -n "$user_ui_port" ]] && RUSTFIN_UI_PORT="$user_ui_port"
[[ -n "$user_browser_backend_origin" ]] && RUSTYFIN_BROWSER_BACKEND_ORIGIN="$user_browser_backend_origin"
[[ -n "$user_ws_allowed_origins" ]] && RUSTFIN_WS_ALLOWED_ORIGINS="$user_ws_allowed_origins"
[[ -n "$user_webrtc_ice_servers_json" ]] && RUSTFIN_WEBRTC_ICE_SERVERS_JSON="$user_webrtc_ice_servers_json"
[[ -n "$user_webrtc_stun_url" ]] && RUSTFIN_WEBRTC_STUN_URL="$user_webrtc_stun_url"
[[ -n "$user_webrtc_turn_url" ]] && RUSTFIN_WEBRTC_TURN_URL="$user_webrtc_turn_url"
[[ -n "$user_webrtc_turn_urls" ]] && RUSTFIN_WEBRTC_TURN_URLS="$user_webrtc_turn_urls"
[[ -n "$user_webrtc_turn_username" ]] && RUSTFIN_WEBRTC_TURN_USERNAME="$user_webrtc_turn_username"
[[ -n "$user_webrtc_turn_credential" ]] && RUSTFIN_WEBRTC_TURN_CREDENTIAL="$user_webrtc_turn_credential"
[[ -n "$user_public_host" ]] && RUSTFIN_PUBLIC_HOST="$user_public_host"
[[ -n "$user_media_path" ]] && RUSTFIN_MEDIA_PATH="$user_media_path"
[[ -n "$user_database_url" ]] && RUSTFIN_DATABASE_URL="$user_database_url"
[[ -n "$user_jwt_secret" ]] && RUSTFIN_JWT_SECRET="$user_jwt_secret"
[[ -n "$user_ai_gpu_backend" ]] && RUSTFIN_AI_GPU_BACKEND="$user_ai_gpu_backend"
[[ -n "$user_transcoder_hw_accel" ]] && RUSTFIN_TRANSCODER_HW_ACCEL="$user_transcoder_hw_accel"
[[ -n "$user_transcoder_require_hw_accel" ]] && RUSTFIN_TRANSCODER_REQUIRE_HW_ACCEL="$user_transcoder_require_hw_accel"
[[ -n "$user_transcode_idle_timeout_secs" ]] && RUSTFIN_TRANSCODE_IDLE_TIMEOUT_SECS="$user_transcode_idle_timeout_secs"
[[ -n "$user_stream_token_ttl_seconds" ]] && RUSTFIN_STREAM_TOKEN_TTL_SECONDS="$user_stream_token_ttl_seconds"
[[ -n "$user_transcription_gpu_mode" ]] && RUSTFIN_TRANSCRIPTION_GPU_MODE="$user_transcription_gpu_mode"
[[ -n "$user_transcription_require_gpu" ]] && RUSTFIN_TRANSCRIPTION_REQUIRE_GPU="$user_transcription_require_gpu"
[[ -n "$user_transcription_agent_cargo_features" ]] && RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES="$user_transcription_agent_cargo_features"

PICKER_HELPER_PORT="${RUSTFIN_PICKER_HELPER_PORT:-43110}"

export RUSTFIN_ENABLE_SERVERS_AGENT
export RUSTFIN_BACKEND_PORT="${RUSTFIN_BACKEND_PORT:-}"
export RUSTFIN_CALENDAR_PORT="${RUSTFIN_CALENDAR_PORT:-}"
export RUSTFIN_TMDB_AGENT_PORT="${RUSTFIN_TMDB_AGENT_PORT:-}"
export RUSTFIN_YOUTUBE_AGENT_PORT="${RUSTFIN_YOUTUBE_AGENT_PORT:-}"
export RUSTFIN_TRANSCRIPTION_AGENT_PORT="${RUSTFIN_TRANSCRIPTION_AGENT_PORT:-}"
export RUSTFIN_SERVERS_AGENT_PORT="${RUSTFIN_SERVERS_AGENT_PORT:-}"
export RUSTFIN_UI_INTERNAL_PORT="${RUSTFIN_UI_INTERNAL_PORT:-}"
export RUSTFIN_UI_PORT="${RUSTFIN_UI_PORT:-}"
export RUSTFIN_PUBLIC_HOST="${RUSTFIN_PUBLIC_HOST:-}"
export RUSTYFIN_BROWSER_BACKEND_ORIGIN="${RUSTYFIN_BROWSER_BACKEND_ORIGIN:-}"
export RUSTFIN_WS_ALLOWED_ORIGINS="${RUSTFIN_WS_ALLOWED_ORIGINS:-}"
export RUSTFIN_WEBRTC_ICE_SERVERS_JSON="${RUSTFIN_WEBRTC_ICE_SERVERS_JSON:-}"
export RUSTFIN_WEBRTC_STUN_URL="${RUSTFIN_WEBRTC_STUN_URL:-}"
export RUSTFIN_WEBRTC_TURN_URL="${RUSTFIN_WEBRTC_TURN_URL:-}"
export RUSTFIN_WEBRTC_TURN_URLS="${RUSTFIN_WEBRTC_TURN_URLS:-}"
export RUSTFIN_WEBRTC_TURN_USERNAME="${RUSTFIN_WEBRTC_TURN_USERNAME:-}"
export RUSTFIN_WEBRTC_TURN_CREDENTIAL="${RUSTFIN_WEBRTC_TURN_CREDENTIAL:-}"
export RUSTFIN_MEDIA_PATH="${RUSTFIN_MEDIA_PATH:-}"
export RUSTFIN_DIRECTORY_PICKER_HELPER_URL="${RUSTFIN_DIRECTORY_PICKER_HELPER_URL:-}"
export RUSTFIN_DATABASE_URL="${RUSTFIN_DATABASE_URL:-}"
export RUSTFIN_JWT_SECRET="${RUSTFIN_JWT_SECRET:-}"
export RUSTFIN_PG_USER="${RUSTFIN_PG_USER:-}"
export RUSTFIN_PG_PASSWORD="${RUSTFIN_PG_PASSWORD:-}"
export RUSTFIN_PG_DB="${RUSTFIN_PG_DB:-}"
export RUSTFIN_NATIVE_LINUX_TARGET="${RUSTFIN_NATIVE_LINUX_TARGET:-}"
export RUSTFIN_SERVERS_AGENT_URL="${RUSTFIN_SERVERS_AGENT_URL:-}"
export RUSTFIN_SERVERS_AGENT_TOKEN="${RUSTFIN_SERVERS_AGENT_TOKEN:-}"
export RUSTFIN_AI_GPU_BACKEND="${RUSTFIN_AI_GPU_BACKEND:-}"
export RUSTFIN_TRANSCODER_HW_ACCEL="${RUSTFIN_TRANSCODER_HW_ACCEL:-}"
export RUSTFIN_TRANSCODER_REQUIRE_HW_ACCEL="${RUSTFIN_TRANSCODER_REQUIRE_HW_ACCEL:-}"
export RUSTFIN_TRANSCODE_IDLE_TIMEOUT_SECS="${RUSTFIN_TRANSCODE_IDLE_TIMEOUT_SECS:-}"
export RUSTFIN_STREAM_TOKEN_TTL_SECONDS="${RUSTFIN_STREAM_TOKEN_TTL_SECONDS:-}"
export RUSTFIN_TRANSCRIPTION_GPU_MODE="${RUSTFIN_TRANSCRIPTION_GPU_MODE:-}"
export RUSTFIN_TRANSCRIPTION_REQUIRE_GPU="${RUSTFIN_TRANSCRIPTION_REQUIRE_GPU:-}"
export RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES="${RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES:-}"
export RUSTFIN_PICKER_HELPER_PORT="$PICKER_HELPER_PORT"

RUNTIME_PLAN_FILE="$SAFE_TMP_DIR/native-runtime-plan.env"
run_installer plan-native-runtime \
  --repo-root "$REPO_ROOT" \
  --cache-dir "$CACHE_DIR" \
  --safe-tmp-dir "$SAFE_TMP_DIR" \
  --picker-helper-port "$PICKER_HELPER_PORT" \
  > "$RUNTIME_PLAN_FILE"
set -a
# shellcheck disable=SC1090
source "$RUNTIME_PLAN_FILE"
set +a

public_host="$RUSTFIN_PUBLIC_HOST"
db_target_log="$RUSTFIN_DATABASE_TARGET_LOG"

NATIVE_BIN_DIR_ABS="$REPO_ROOT/.native-bins/${RUSTFIN_NATIVE_TARGET}/${RUSTFIN_RUST_BUILD_PROFILE}"
mkdir -p "$NATIVE_BIN_DIR_ABS"

export RUSTFIN_HOST_OS="linux"
export RUSTFIN_RUNTIME_MODE="native"
export RUSTFIN_CACHE_DIR="$CACHE_DIR"
export RUSTFIN_RUN_MIGRATIONS="${RUSTFIN_RUN_MIGRATIONS:-true}"
export RUSTFIN_CALENDAR_RUN_MIGRATIONS="${RUSTFIN_CALENDAR_RUN_MIGRATIONS:-false}"
export RUSTFIN_TMDB_AGENT_RUN_MIGRATIONS="${RUSTFIN_TMDB_AGENT_RUN_MIGRATIONS:-false}"

export RUSTFIN_BIND="127.0.0.1:${RUSTFIN_BACKEND_PORT}"
export RUSTFIN_CALENDAR_BIND="127.0.0.1:${RUSTFIN_CALENDAR_PORT}"
export RUSTFIN_TMDB_AGENT_BIND="127.0.0.1:${RUSTFIN_TMDB_AGENT_PORT}"
export RUSTFIN_YOUTUBE_AGENT_BIND="127.0.0.1:${RUSTFIN_YOUTUBE_AGENT_PORT}"
export RUSTFIN_TRANSCRIPTION_AGENT_BIND="127.0.0.1:${RUSTFIN_TRANSCRIPTION_AGENT_PORT}"
export RUSTFIN_SERVERS_AGENT_BIND="127.0.0.1:${RUSTFIN_SERVERS_AGENT_PORT}"
export RUSTFIN_AUTH_BASE_URL="http://127.0.0.1:${RUSTFIN_BACKEND_PORT}"
export RUSTFIN_FFMPEG_PATH="${RUSTFIN_FFMPEG_PATH:-ffmpeg}"
export RUSTFIN_FFPROBE_PATH="${RUSTFIN_FFPROBE_PATH:-ffprobe}"
export RUSTFIN_WHISPER_MODEL_PATH="${RUSTFIN_WHISPER_MODEL_PATH:-$CACHE_DIR/whisper/ggml-small.en.bin}"
export RUSTFIN_WHISPER_MODEL_URL="${RUSTFIN_WHISPER_MODEL_URL:-https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin}"
export RUSTFIN_TRANSCRIPTION_MAX_PARALLEL_INFERENCES="${RUSTFIN_TRANSCRIPTION_MAX_PARALLEL_INFERENCES:-3}"
export RUSTFIN_TRANSCRIPTION_MAX_WORKERS="${RUSTFIN_TRANSCRIPTION_MAX_WORKERS:-6}"
export RUSTFIN_TRANSCRIPTION_MAX_WORKERS_PER_SESSION="${RUSTFIN_TRANSCRIPTION_MAX_WORKERS_PER_SESSION:-8}"
export RUSTFIN_TRANSCRIPTION_THREADS_PER_WORKER="${RUSTFIN_TRANSCRIPTION_THREADS_PER_WORKER:-2}"
export RUSTFIN_TRANSCRIPTION_ACQUIRE_TIMEOUT_MS="${RUSTFIN_TRANSCRIPTION_ACQUIRE_TIMEOUT_MS:-2500}"
export RUSTFIN_TRANSCODER_HW_ACCEL
export RUSTFIN_TRANSCODER_REQUIRE_HW_ACCEL
export RUSTFIN_TRANSCODE_IDLE_TIMEOUT_SECS
export RUSTFIN_STREAM_TOKEN_TTL_SECONDS

export RUSTFIN_TMDB_AGENT_URL="${RUSTFIN_TMDB_AGENT_URL:-http://127.0.0.1:${RUSTFIN_TMDB_AGENT_PORT}}"
export RUSTFIN_YOUTUBE_AGENT_URL="${RUSTFIN_YOUTUBE_AGENT_URL:-http://127.0.0.1:${RUSTFIN_YOUTUBE_AGENT_PORT}}"
export RUSTFIN_TRANSCRIPTION_AGENT_URL="${RUSTFIN_TRANSCRIPTION_AGENT_URL:-http://127.0.0.1:${RUSTFIN_TRANSCRIPTION_AGENT_PORT}}"

if [[ "$RUSTFIN_ENABLE_SERVERS_AGENT" != "1" ]]; then
  [[ -n "${RUSTFIN_SERVERS_AGENT_TOKEN:-}" ]] || unset RUSTFIN_SERVERS_AGENT_TOKEN
  [[ -n "${RUSTFIN_SERVERS_AGENT_URL:-}" ]] || unset RUSTFIN_SERVERS_AGENT_URL
fi

info "Using TMPDIR: $TMPDIR"
info "Native runtime dir: $RUNTIME_ROOT"
info "Using media path: $RUSTFIN_MEDIA_PATH"
info "Backend port: $RUSTFIN_BACKEND_PORT"
info "Calendar port: $RUSTFIN_CALENDAR_PORT"
info "TMDB agent port: $RUSTFIN_TMDB_AGENT_PORT"
info "YouTube agent port: $RUSTFIN_YOUTUBE_AGENT_PORT"
info "Transcription agent port: $RUSTFIN_TRANSCRIPTION_AGENT_PORT"
if [[ "$RUSTFIN_ENABLE_SERVERS_AGENT" == "1" ]]; then
  info "Servers agent port: $RUSTFIN_SERVERS_AGENT_PORT"
fi
info "UI internal port: $RUSTFIN_UI_INTERNAL_PORT"
info "UI edge port: $RUSTFIN_UI_PORT"
info "Public host: $public_host"
info "Browser backend origin: $RUSTYFIN_BROWSER_BACKEND_ORIGIN"
info "WebSocket allowed origins: $RUSTFIN_WS_ALLOWED_ORIGINS"
info "Edge TLS cert: $RUSTFIN_EDGE_TLS_CERT"
info "Database target: $db_target_log"
info "Rust build profile: $RUSTFIN_RUST_BUILD_PROFILE"
info "Rust target: $RUSTFIN_NATIVE_TARGET"
info "Native binary output dir: $NATIVE_BIN_DIR_ABS"
info "Transcription GPU mode: $RUSTFIN_TRANSCRIPTION_GPU_MODE"
info "Transcription GPU required: $RUSTFIN_TRANSCRIPTION_REQUIRE_GPU"
info "Transcription agent cargo features: $RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES"
if [[ "$BUILD_ONLY" == "true" ]]; then
  info "Mode: build-only"
fi

if [[ "$BUILD" == "true" ]]; then
  run_installer build-native-runtime-artifacts \
    --profile "$RUSTFIN_RUST_BUILD_PROFILE" \
    --target "$RUSTFIN_NATIVE_TARGET" \
    --output-dir "$NATIVE_BIN_DIR_ABS" \
    --cache-dir "$SAFE_TMP_DIR/native-linux/.build-cache" \
    --ui-deps-state-file "$BUILD_STATE_FILE" \
    --backend-port "$RUSTFIN_BACKEND_PORT" \
    --calendar-port "$RUSTFIN_CALENDAR_PORT"
else
  [[ -x "$NATIVE_BIN_DIR_ABS/rustfin-server" ]] || die "Native binaries are missing. Run without --no-build first."
  [[ -f "$REPO_ROOT/ui/.next/standalone/server.js" ]] || die "Native UI standalone build is missing. Run without --no-build first."

  # Guard against a stale/incomplete standalone tree when reusing existing UI artifacts.
  # Without these static chunks, Next will serve HTML but hydration/scripts fail at runtime.
  if [[ ! -d "$REPO_ROOT/ui/.next/standalone/.next/static" ]]; then
    if [[ -d "$REPO_ROOT/ui/.next/static" ]]; then
      warn "UI standalone static assets are missing. Restoring from ui/.next/static."
      mkdir -p "$REPO_ROOT/ui/.next/standalone/.next/static"
      cp -R "$REPO_ROOT/ui/.next/static/." "$REPO_ROOT/ui/.next/standalone/.next/static/"
    else
      die "UI standalone static assets are missing. Run without --no-build first."
    fi
  fi
fi

launch_args=()
if [[ "$BUILD_ONLY" == "true" ]]; then
  launch_args+=(--build-only)
fi
if [[ "$DETACH" == "false" ]]; then
  launch_args+=(--foreground)
fi
if [[ "$HEALTH_CHECK" != "true" ]]; then
  launch_args+=(--no-health-check)
fi

run_installer launch-native-runtime "${launch_args[@]}"
