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
  ./scripts/start.sh [--no-build|--full-rebuild] [--foreground] [--no-health-check] [-f <compose-file>]

Options:
  --build            Rebuild images (cached, default behavior).
  --full-rebuild     Rebuild without cache (slowest, strictest).
  --cached-build     Alias for --build.
  --no-build         Skip image rebuild step.
  --foreground       Run compose in foreground (default is detached).
  --no-health-check  Skip backend health wait loop.
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

while [[ $# -gt 0 ]]; do
  case "$1" in
    --build|--cached-build) BUILD=true; NO_CACHE_BUILD=false; shift ;;
    --full-rebuild) BUILD=true; NO_CACHE_BUILD=true; shift ;;
    --no-build) BUILD=false; shift ;;
    --foreground) DETACH=false; shift ;;
    --no-health-check) HEALTH_CHECK=false; shift ;;
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

RUNTIME_ENV_FILE="$REPO_ROOT/.rustyfin.runtime.env"

SAFE_TMP_DIR="${RUSTFIN_TMPDIR:-$REPO_ROOT/.tmp}"
mkdir -p "$SAFE_TMP_DIR" || die "Failed to create temp dir: $SAFE_TMP_DIR"
chmod 700 "$SAFE_TMP_DIR" 2>/dev/null || true
[[ -w "$SAFE_TMP_DIR" ]] || die "Temp dir is not writable: $SAFE_TMP_DIR"
export TMPDIR="$SAFE_TMP_DIR"

# Load prior runtime settings so repeated runs stay stable.
user_backend_port="${RUSTFIN_BACKEND_PORT:-}"
user_ui_port="${RUSTFIN_UI_PORT:-}"
user_media_path="${RUSTFIN_MEDIA_PATH:-}"
user_browser_backend_origin="${RUSTYFIN_BROWSER_BACKEND_ORIGIN:-}"
user_ws_allowed_origins="${RUSTFIN_WS_ALLOWED_ORIGINS:-}"

if [[ -f "$RUNTIME_ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$RUNTIME_ENV_FILE"
fi

# Explicit shell/env values always win over runtime file values.
[[ -n "$user_backend_port" ]] && RUSTFIN_BACKEND_PORT="$user_backend_port"
[[ -n "$user_ui_port" ]] && RUSTFIN_UI_PORT="$user_ui_port"
[[ -n "$user_media_path" ]] && RUSTFIN_MEDIA_PATH="$user_media_path"
[[ -n "$user_browser_backend_origin" ]] && RUSTYFIN_BROWSER_BACKEND_ORIGIN="$user_browser_backend_origin"
[[ -n "$user_ws_allowed_origins" ]] && RUSTFIN_WS_ALLOWED_ORIGINS="$user_ws_allowed_origins"

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
if [[ "$BUILD" == "true" ]]; then
  if [[ "$NO_CACHE_BUILD" == "true" ]]; then
    info "Build mode: full rebuild (no Docker cache)"
  else
    info "Build mode: rebuild (Docker cache enabled)"
  fi
else
  warn "Build mode: skipped (--no-build)"
fi
if [[ -n "${RUSTFIN_TMDB_KEY:-}" ]]; then
  info "TMDB metadata enrichment: enabled"
else
  warn "TMDB metadata enrichment disabled (set RUSTFIN_TMDB_KEY to fetch online posters/metadata)"
fi

if [[ "$BUILD" == "true" ]]; then
  build_args=(build --pull)
  if [[ "$NO_CACHE_BUILD" == "true" ]]; then
    build_args+=(--no-cache)
  fi
  info "Rebuilding Docker images..."
  if ! docker compose -f "$COMPOSE_FILE" "${build_args[@]}"; then
    if [[ "$NO_CACHE_BUILD" == "true" ]]; then
      warn "Full no-cache rebuild failed (likely transient network issue). Retrying once with Docker cache."
      if ! docker compose -f "$COMPOSE_FILE" build --pull; then
        die "Docker image rebuild failed after retry. Check your internet connection and retry."
      fi
    else
      die "Docker image rebuild failed. Check your internet connection and retry."
    fi
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

if [[ "$DETACH" == "true" && "$HEALTH_CHECK" == "true" && -n "$(command -v curl || true)" ]]; then
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
fi

success "Rustyfin stack is up."
echo "  Backend: http://localhost:${RUSTFIN_BACKEND_PORT}"
echo "  UI:      https://localhost:${RUSTFIN_UI_PORT}"
if [[ "$public_host" != "localhost" && "$public_host" != "127.0.0.1" ]]; then
  echo "  Backend (LAN): http://${public_host}:${RUSTFIN_BACKEND_PORT}"
  echo "  UI (LAN):      https://${public_host}:${RUSTFIN_UI_PORT}"
fi
echo "  Note: if your browser warns about a local certificate, accept/trust it to enable microphone access."
