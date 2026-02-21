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
  ./scripts/start.sh [--build|--no-build] [--foreground] [--no-health-check] [-f <compose-file>]

Options:
  --build            Force image rebuild step (default behavior).
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
DETACH=true
HEALTH_CHECK=true
COMPOSE_FILE="$REPO_ROOT/docker-compose.yml"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --build) BUILD=true; shift ;;
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

if [[ -f "$RUNTIME_ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$RUNTIME_ENV_FILE"
fi

# Explicit shell/env values always win over runtime file values.
[[ -n "$user_backend_port" ]] && RUSTFIN_BACKEND_PORT="$user_backend_port"
[[ -n "$user_ui_port" ]] && RUSTFIN_UI_PORT="$user_ui_port"
[[ -n "$user_media_path" ]] && RUSTFIN_MEDIA_PATH="$user_media_path"

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

info "Using TMPDIR: $TMPDIR"
info "Using media path: $RUSTFIN_MEDIA_PATH"
info "Backend port: $RUSTFIN_BACKEND_PORT"
info "UI port: $RUSTFIN_UI_PORT"

compose_args=(up --remove-orphans)
if [[ "$DETACH" == "true" ]]; then
  compose_args+=(-d)
fi
if [[ "$BUILD" == "true" ]]; then
  compose_args+=(--build)
fi

docker compose -f "$COMPOSE_FILE" "${compose_args[@]}"

{
  echo "# Generated by scripts/start.sh"
  printf "RUSTFIN_BACKEND_PORT=%q\n" "$RUSTFIN_BACKEND_PORT"
  printf "RUSTFIN_UI_PORT=%q\n" "$RUSTFIN_UI_PORT"
  printf "RUSTFIN_MEDIA_PATH=%q\n" "$RUSTFIN_MEDIA_PATH"
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
echo "  UI:      http://localhost:${RUSTFIN_UI_PORT}"
