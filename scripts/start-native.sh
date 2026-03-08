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
  ./scripts/start-native.sh [--no-build] [--foreground] [--no-health-check]

Options:
  --no-build         Skip Rust/UI build and reuse existing native artifacts.
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
  RUSTFIN_MEDIA_PATH                    Host media root (default: $HOME)
  RUSTFIN_ENABLE_SERVERS_AGENT          Start rustfin-servers-agent (default: 1)
  RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES
                                        Cargo features for transcription agent (default: gpu-opencl)
EOF
}

BUILD=true
DETACH=true
HEALTH_CHECK=true

while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-build) BUILD=false; shift ;;
    --foreground) DETACH=false; shift ;;
    --no-health-check) HEALTH_CHECK=false; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "Unknown argument: $1" ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

[[ "$(uname -s)" == "Linux" ]] || die "Native runtime is supported on Linux hosts only. Use Debian 12."

if ! command -v cargo >/dev/null 2>&1 && [[ -f "${HOME}/.cargo/env" ]]; then
  # shellcheck disable=SC1090
  source "${HOME}/.cargo/env"
fi

command -v cargo >/dev/null 2>&1 || die "cargo is not installed. Run ./scripts/install_native_debian.sh first."
command -v rustc >/dev/null 2>&1 || die "rustc is not installed. Run ./scripts/install_native_debian.sh first."
command -v node >/dev/null 2>&1 || die "node is not installed. Run ./scripts/install_native_debian.sh first."
command -v npm >/dev/null 2>&1 || die "npm is not installed. Run ./scripts/install_native_debian.sh first."
command -v caddy >/dev/null 2>&1 || die "caddy is not installed. Run ./scripts/install_native_debian.sh first."
command -v curl >/dev/null 2>&1 || die "curl is required for native runtime startup."
command -v openssl >/dev/null 2>&1 || die "openssl is required for native runtime startup."
command -v ffmpeg >/dev/null 2>&1 || die "ffmpeg is required for playback/transcoding."
command -v ffprobe >/dev/null 2>&1 || die "ffprobe is required for media probing."

RUSTFIN_RUST_BUILD_PROFILE="${RUSTFIN_RUST_BUILD_PROFILE:-dev}"
RUSTFIN_ENABLE_SERVERS_AGENT="${RUSTFIN_ENABLE_SERVERS_AGENT:-1}"
RUSTFIN_TRANSCRIPTION_GPU_MODE="${RUSTFIN_TRANSCRIPTION_GPU_MODE:-opencl}"
RUSTFIN_TRANSCRIPTION_REQUIRE_GPU="${RUSTFIN_TRANSCRIPTION_REQUIRE_GPU:-1}"
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

RUNTIME_ROOT="${RUSTFIN_NATIVE_RUNTIME_DIR:-$SAFE_TMP_DIR/native-runtime}"
PID_DIR="$RUNTIME_ROOT/pids"
LOG_DIR="$RUNTIME_ROOT/logs"
CACHE_DIR="$RUNTIME_ROOT/cache"
CONFIG_DIR="$RUNTIME_ROOT/config"
TRANSCODE_DIR="$RUNTIME_ROOT/transcode"
mkdir -p "$PID_DIR" "$LOG_DIR" "$CACHE_DIR" "$CONFIG_DIR" "$TRANSCODE_DIR"

RUNTIME_ENV_FILE="$REPO_ROOT/.rustyfin.runtime.env"
BUILD_STATE_FILE="$SAFE_TMP_DIR/native-ui-deps.hash"

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

if [[ -f "$RUNTIME_ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$RUNTIME_ENV_FILE" || true
fi

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

PICKER_HELPER_PORT="${RUSTFIN_PICKER_HELPER_PORT:-43110}"
PICKER_HELPER_HOST="${RUSTFIN_PICKER_HELPER_HOST:-127.0.0.1}"
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

  if curl -fsS "http://127.0.0.1:${PICKER_HELPER_PORT}/health" >/dev/null 2>&1; then
    info "Directory picker helper already running on port ${PICKER_HELPER_PORT}."
    return
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

HOST = os.environ.get("RUSTFIN_PICKER_HELPER_HOST", "127.0.0.1")
PORT = int(os.environ.get("RUSTFIN_PICKER_HELPER_PORT", "43110"))

def pick_directory():
    system = platform.system()
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

  for _ in $(seq 1 20); do
    if curl -fsS "http://127.0.0.1:${PICKER_HELPER_PORT}/health" >/dev/null 2>&1; then
      info "Directory picker helper started on http://127.0.0.1:${PICKER_HELPER_PORT} (pid $helper_pid)"
      return
    fi
    sleep 0.2
  done

  warn "Directory picker helper did not report healthy; check: $PICKER_HELPER_LOG_FILE"
}

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
  local ip=""
  ip="$(ip route get 1.1.1.1 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="src"){print $(i+1); exit}}')"
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

cleanup_stale_pid() {
  local pidfile="$1"
  if [[ ! -f "$pidfile" ]]; then
    return
  fi
  local pid
  pid="$(cat "$pidfile" 2>/dev/null || true)"
  if [[ -z "$pid" || ! "$pid" =~ ^[0-9]+$ ]]; then
    rm -f "$pidfile"
    return
  fi
  if ! kill -0 "$pid" 2>/dev/null; then
    rm -f "$pidfile"
  fi
}

assert_not_running() {
  local service=""
  for service in rustfin rustfin-calendar rustfin-tmdb-agent rustfin-youtube-agent rustfin-transcription-agent rustfin-servers-agent rustfin-ui rustfin-edge; do
    cleanup_stale_pid "$PID_DIR/${service}.pid"
    if [[ -f "$PID_DIR/${service}.pid" ]]; then
      die "Native runtime already appears to be running (${service}). Stop it first with ./scripts/stop-native.sh"
    fi
  done
}

wait_for_http() {
  local name="$1"
  local url="$2"
  local max_attempts="${3:-60}"
  local curl_args=("${@:4}")
  for _ in $(seq 1 "$max_attempts"); do
    if curl -fsS "${curl_args[@]}" "$url" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  warn "${name} did not become ready: ${url}"
  return 1
}

generate_secret() {
  openssl rand -hex 24
}

start_process() {
  local name="$1"
  local workdir="$2"
  shift 2
  local logfile="$LOG_DIR/${name}.log"
  local pidfile="$PID_DIR/${name}.pid"

  cleanup_stale_pid "$pidfile"
  [[ ! -f "$pidfile" ]] || die "${name} is already running"

  (
    cd "$workdir"
    nohup "$@" </dev/null >>"$logfile" 2>&1 &
    echo "$!" > "$pidfile"
  )

  local pid
  pid="$(cat "$pidfile" 2>/dev/null || true)"
  sleep 0.3
  if [[ -z "$pid" || ! "$pid" =~ ^[0-9]+$ || ! -d "/proc/${pid}" ]]; then
    die "Failed starting ${name}. Check ${logfile}"
  fi
  info "Started ${name} (pid ${pid})"
}

host_arch="$(uname -m)"
case "$host_arch" in
  arm64|aarch64) RUSTFIN_NATIVE_TARGET="${RUSTFIN_NATIVE_LINUX_TARGET:-aarch64-unknown-linux-gnu}" ;;
  x86_64|amd64) RUSTFIN_NATIVE_TARGET="${RUSTFIN_NATIVE_LINUX_TARGET:-x86_64-unknown-linux-gnu}" ;;
  *) die "Unsupported host arch '$host_arch'; set RUSTFIN_NATIVE_LINUX_TARGET explicitly." ;;
esac

NATIVE_BIN_DIR_ABS="$REPO_ROOT/.native-bins/${RUSTFIN_NATIVE_TARGET}/${RUSTFIN_RUST_BUILD_PROFILE}"
mkdir -p "$NATIVE_BIN_DIR_ABS"

backend_port="${RUSTFIN_BACKEND_PORT:-8096}"
calendar_port="${RUSTFIN_CALENDAR_PORT:-8099}"
tmdb_port="${RUSTFIN_TMDB_AGENT_PORT:-8100}"
youtube_port="${RUSTFIN_YOUTUBE_AGENT_PORT:-8101}"
transcription_port="${RUSTFIN_TRANSCRIPTION_AGENT_PORT:-8102}"
servers_agent_port="${RUSTFIN_SERVERS_AGENT_PORT:-8103}"
ui_internal_port="${RUSTFIN_UI_INTERNAL_PORT:-3001}"
ui_port="${RUSTFIN_UI_PORT:-3000}"

backend_port="$(pick_free_port "$backend_port")"
calendar_port="$(pick_free_port "$calendar_port")"
tmdb_port="$(pick_free_port "$tmdb_port")"
youtube_port="$(pick_free_port "$youtube_port")"
transcription_port="$(pick_free_port "$transcription_port")"
if [[ "$RUSTFIN_ENABLE_SERVERS_AGENT" == "1" ]]; then
  servers_agent_port="$(pick_free_port "$servers_agent_port")"
fi
ui_internal_port="$(pick_free_port "$ui_internal_port")"
ui_port="$(pick_free_port "$ui_port")"

export RUSTFIN_BACKEND_PORT="$backend_port"
export RUSTFIN_CALENDAR_PORT="$calendar_port"
export RUSTFIN_TMDB_AGENT_PORT="$tmdb_port"
export RUSTFIN_YOUTUBE_AGENT_PORT="$youtube_port"
export RUSTFIN_TRANSCRIPTION_AGENT_PORT="$transcription_port"
export RUSTFIN_SERVERS_AGENT_PORT="$servers_agent_port"
export RUSTFIN_UI_INTERNAL_PORT="$ui_internal_port"
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

MEDIA_PATH="${RUSTFIN_MEDIA_PATH:-${HOME:-$REPO_ROOT/media}}"
mkdir -p "$MEDIA_PATH" || die "Failed to create media path: $MEDIA_PATH"
MEDIA_PATH="$(cd "$MEDIA_PATH" && pwd -L)" || die "Failed to resolve media path: $MEDIA_PATH"
[[ -d "$MEDIA_PATH" ]] || die "Resolved media path is not a directory: $MEDIA_PATH"
[[ -r "$MEDIA_PATH" ]] || die "Media path is not readable: $MEDIA_PATH"
[[ -x "$MEDIA_PATH" ]] || die "Media path is not traversable: $MEDIA_PATH"

export RUSTFIN_MEDIA_PATH="$MEDIA_PATH"
export RUSTFIN_MEDIA_HOST_PATH="${RUSTFIN_MEDIA_HOST_PATH:-$MEDIA_PATH}"
export RUSTFIN_MEDIA_CONTAINER_ROOT="${RUSTFIN_MEDIA_CONTAINER_ROOT:-$MEDIA_PATH}"
export RUSTFIN_DIRECTORY_PICKER_HELPER_URL="${RUSTFIN_DIRECTORY_PICKER_HELPER_URL:-http://127.0.0.1:${PICKER_HELPER_PORT}/pick}"
export RUSTFIN_HOST_OS="linux"
export RUSTFIN_RUNTIME_MODE="native"
export RUSTFIN_CACHE_DIR="$CACHE_DIR"
export RUSTFIN_RUN_MIGRATIONS="${RUSTFIN_RUN_MIGRATIONS:-true}"
export RUSTFIN_CALENDAR_RUN_MIGRATIONS="${RUSTFIN_CALENDAR_RUN_MIGRATIONS:-false}"
export RUSTFIN_TMDB_AGENT_RUN_MIGRATIONS="${RUSTFIN_TMDB_AGENT_RUN_MIGRATIONS:-false}"

if [[ -z "${RUSTFIN_DATABASE_URL:-}" ]]; then
  pg_user="${RUSTFIN_PG_USER:-rustfin}"
  pg_password="${RUSTFIN_PG_PASSWORD:-rustfin}"
  pg_db="${RUSTFIN_PG_DB:-rustfin}"
  RUSTFIN_DATABASE_URL="postgresql://${pg_user}:${pg_password}@127.0.0.1:5432/${pg_db}"
fi
export RUSTFIN_DATABASE_URL

db_target_lc="$(printf '%s' "$RUSTFIN_DATABASE_URL" | tr '[:upper:]' '[:lower:]')"
if [[ "$db_target_lc" != postgres://* && "$db_target_lc" != postgresql://* ]]; then
  die "RUSTFIN_DATABASE_URL must be a PostgreSQL URL (postgres:// or postgresql://)."
fi
db_target_log="$(printf '%s' "$RUSTFIN_DATABASE_URL" | sed -E 's#(postgres(ql)?://)[^@/]+@#\1<redacted>@#')"

if command -v pg_isready >/dev/null 2>&1; then
  if ! pg_isready -d "$RUSTFIN_DATABASE_URL" >/dev/null 2>&1; then
    die "PostgreSQL is not ready at ${db_target_log}. Run ./scripts/install_native_debian.sh first, or start PostgreSQL."
  fi
else
  warn "pg_isready not found; skipping explicit PostgreSQL readiness check."
fi

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

export RUSTFIN_TMDB_AGENT_TOKEN="${RUSTFIN_TMDB_AGENT_TOKEN:-$(generate_secret)}"
export RUSTFIN_YOUTUBE_AGENT_TOKEN="${RUSTFIN_YOUTUBE_AGENT_TOKEN:-$(generate_secret)}"
export RUSTFIN_TRANSCRIPTION_AGENT_TOKEN="${RUSTFIN_TRANSCRIPTION_AGENT_TOKEN:-$(generate_secret)}"
if [[ "$RUSTFIN_ENABLE_SERVERS_AGENT" == "1" ]]; then
  export RUSTFIN_SERVERS_AGENT_TOKEN="${RUSTFIN_SERVERS_AGENT_TOKEN:-$(generate_secret)}"
  export RUSTFIN_SERVERS_AGENT_URL="${RUSTFIN_SERVERS_AGENT_URL:-http://127.0.0.1:${RUSTFIN_SERVERS_AGENT_PORT}}"
else
  if [[ -n "${RUSTFIN_SERVERS_AGENT_TOKEN:-}" ]]; then
    export RUSTFIN_SERVERS_AGENT_TOKEN
  else
    unset RUSTFIN_SERVERS_AGENT_TOKEN
  fi
  if [[ -n "${RUSTFIN_SERVERS_AGENT_URL:-}" ]]; then
    export RUSTFIN_SERVERS_AGENT_URL
  else
    unset RUSTFIN_SERVERS_AGENT_URL
  fi
fi

start_directory_picker_helper
assert_not_running

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

if [[ "$BUILD" == "true" ]]; then
  export RUSTFIN_NATIVE_GNU_COMPAT_BUILD=0
  "$REPO_ROOT/scripts/build_linux_binaries.sh" \
    --profile "$RUSTFIN_RUST_BUILD_PROFILE" \
    --target "$RUSTFIN_NATIVE_TARGET" \
    --output-dir "$NATIVE_BIN_DIR_ABS" \
    --cache-dir "$SAFE_TMP_DIR/native-linux/.build-cache" \
    --bin rustfin-server \
    --bin rustfin-calendar \
    --bin rustfin-tmdb-agent \
    --bin rustfin-youtube-agent \
    --bin rustfin-transcription-agent \
    --bin rustfin-servers-agent

  ui_dep_hash="$(
    {
      hash_file "$REPO_ROOT/ui/package.json"
      hash_file "$REPO_ROOT/ui/package-lock.json"
    } | hash_stdin
  )"
  current_ui_dep_hash="$(cat "$BUILD_STATE_FILE" 2>/dev/null || true)"
  if [[ ! -d "$REPO_ROOT/ui/node_modules" || "$current_ui_dep_hash" != "$ui_dep_hash" ]]; then
    info "Installing UI dependencies natively..."
    (cd "$REPO_ROOT/ui" && npm ci)
    printf '%s' "$ui_dep_hash" > "$BUILD_STATE_FILE"
  fi

  info "Building Next.js UI natively..."
  (
    cd "$REPO_ROOT/ui"
    NEXT_TELEMETRY_DISABLED=1 \
      RUSTYFIN_API_BASE_URL="http://127.0.0.1:${RUSTFIN_BACKEND_PORT}" \
      RUSTYFIN_CALENDAR_API_BASE_URL="http://127.0.0.1:${RUSTFIN_CALENDAR_PORT}" \
      npm run build

    mkdir -p .next/standalone/public .next/standalone/.next/static
    if [[ -d public ]]; then
      cp -R public/. .next/standalone/public/
    fi
    if [[ -d .next/static ]]; then
      cp -R .next/static/. .next/standalone/.next/static/
    fi
  )
else
  [[ -x "$NATIVE_BIN_DIR_ABS/rustfin-server" ]] || die "Native binaries are missing. Run without --no-build first."
  [[ -f "$REPO_ROOT/ui/.next/standalone/server.js" ]] || die "Native UI standalone build is missing. Run without --no-build first."
fi

start_process "rustfin-tmdb-agent" "$REPO_ROOT" "$NATIVE_BIN_DIR_ABS/rustfin-tmdb-agent"
start_process "rustfin-youtube-agent" "$REPO_ROOT" "$NATIVE_BIN_DIR_ABS/rustfin-youtube-agent"
start_process "rustfin-transcription-agent" "$REPO_ROOT" "$NATIVE_BIN_DIR_ABS/rustfin-transcription-agent"
if [[ "$RUSTFIN_ENABLE_SERVERS_AGENT" == "1" ]]; then
  start_process "rustfin-servers-agent" "$REPO_ROOT" "$NATIVE_BIN_DIR_ABS/rustfin-servers-agent"
fi
start_process "rustfin" "$REPO_ROOT" "$NATIVE_BIN_DIR_ABS/rustfin-server"
start_process "rustfin-calendar" "$REPO_ROOT" "$NATIVE_BIN_DIR_ABS/rustfin-calendar"
start_process "rustfin-ui" "$REPO_ROOT/ui/.next/standalone" env PORT="$RUSTFIN_UI_INTERNAL_PORT" HOSTNAME="127.0.0.1" node server.js
start_process "rustfin-edge" "$REPO_ROOT" caddy run --config "$REPO_ROOT/scripts/caddy/Caddyfile.native" --adapter caddyfile

{
  echo "# Generated by scripts/start-native.sh"
  printf "RUSTFIN_RUNTIME_MODE=%q\n" "native"
  printf "RUSTFIN_NATIVE_RUNTIME_DIR=%q\n" "$RUNTIME_ROOT"
  printf "RUSTFIN_BACKEND_PORT=%q\n" "$RUSTFIN_BACKEND_PORT"
  printf "RUSTFIN_CALENDAR_PORT=%q\n" "$RUSTFIN_CALENDAR_PORT"
  printf "RUSTFIN_TMDB_AGENT_PORT=%q\n" "$RUSTFIN_TMDB_AGENT_PORT"
  printf "RUSTFIN_YOUTUBE_AGENT_PORT=%q\n" "$RUSTFIN_YOUTUBE_AGENT_PORT"
  printf "RUSTFIN_TRANSCRIPTION_AGENT_PORT=%q\n" "$RUSTFIN_TRANSCRIPTION_AGENT_PORT"
  printf "RUSTFIN_SERVERS_AGENT_PORT=%q\n" "$RUSTFIN_SERVERS_AGENT_PORT"
  printf "RUSTFIN_UI_INTERNAL_PORT=%q\n" "$RUSTFIN_UI_INTERNAL_PORT"
  printf "RUSTFIN_UI_PORT=%q\n" "$RUSTFIN_UI_PORT"
  printf "RUSTFIN_MEDIA_PATH=%q\n" "$RUSTFIN_MEDIA_PATH"
  printf "RUSTYFIN_BROWSER_BACKEND_ORIGIN=%q\n" "$RUSTYFIN_BROWSER_BACKEND_ORIGIN"
  printf "RUSTFIN_WS_ALLOWED_ORIGINS=%q\n" "$RUSTFIN_WS_ALLOWED_ORIGINS"
  printf "RUSTFIN_DIRECTORY_PICKER_HELPER_URL=%q\n" "$RUSTFIN_DIRECTORY_PICKER_HELPER_URL"
  printf "RUSTFIN_DATABASE_URL=%q\n" "$RUSTFIN_DATABASE_URL"
} > "$RUNTIME_ENV_FILE"
chmod 600 "$RUNTIME_ENV_FILE" 2>/dev/null || true

if [[ "$HEALTH_CHECK" == "true" ]]; then
  wait_for_http "rustfin" "http://127.0.0.1:${RUSTFIN_BACKEND_PORT}/health" 120 || true
  wait_for_http "calendar" "http://127.0.0.1:${RUSTFIN_CALENDAR_PORT}/health" 60 || true
  wait_for_http "tmdb-agent" "http://127.0.0.1:${RUSTFIN_TMDB_AGENT_PORT}/health" 60 || true
  wait_for_http "youtube-agent" "http://127.0.0.1:${RUSTFIN_YOUTUBE_AGENT_PORT}/health" 60 || true
  wait_for_http "transcription-agent" "http://127.0.0.1:${RUSTFIN_TRANSCRIPTION_AGENT_PORT}/health" 60 || true
  if [[ "$RUSTFIN_ENABLE_SERVERS_AGENT" == "1" ]]; then
    wait_for_http "servers-agent" "http://127.0.0.1:${RUSTFIN_SERVERS_AGENT_PORT}/health" 60 || true
  fi
  wait_for_http "ui-internal" "http://127.0.0.1:${RUSTFIN_UI_INTERNAL_PORT}" 60 || true
  wait_for_http "ui-edge" "https://127.0.0.1:${RUSTFIN_UI_PORT}/health" 60 -k || true
fi

success "Rustyfin native runtime is up."
success "UI: https://${public_host}:${RUSTFIN_UI_PORT}"
info "Logs: $LOG_DIR"

if [[ "$DETACH" == "false" ]]; then
  exec tail -n 50 -f "$LOG_DIR"/*.log
fi
