#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

# Stable writable temp/cache paths for macOS and sandboxed envs.
SAFE_TMP_DIR="${RUSTFIN_TMPDIR:-${REPO_ROOT}/.tmp}"
SAFE_NPM_CACHE="${RUSTFIN_NPM_CACHE:-${REPO_ROOT}/.npm-cache}"
SAFE_PW_BROWSERS="${RUSTFIN_PLAYWRIGHT_BROWSERS:-${REPO_ROOT}/.playwright-browsers}"
mkdir -p "${SAFE_TMP_DIR}" "${SAFE_NPM_CACHE}" "${SAFE_PW_BROWSERS}"
chmod 700 "${SAFE_TMP_DIR}" 2>/dev/null || true

export TMPDIR="${SAFE_TMP_DIR}"
export npm_config_cache="${SAFE_NPM_CACHE}"
export PLAYWRIGHT_BROWSERS_PATH="${SAFE_PW_BROWSERS}"

# Use caller-provided ports when set; otherwise allocate free loopback ports at runtime.
TEST_BACKEND_PORT="${RUSTFIN_TEST_BACKEND_PORT:-}"
TEST_UI_PORT="${RUSTFIN_TEST_UI_PORT:-}"
TEST_BACKEND_BIND=""
TEST_BACKEND_URL=""
TEST_UI_URL=""

set_backend_endpoints() {
  local port="$1"
  TEST_BACKEND_PORT="${port}"
  TEST_BACKEND_BIND="127.0.0.1:${port}"
  TEST_BACKEND_URL="http://127.0.0.1:${port}"
}

set_ui_endpoint() {
  local port="$1"
  TEST_UI_PORT="${port}"
  TEST_UI_URL="http://127.0.0.1:${port}"
}

if [[ -n "${TEST_BACKEND_PORT}" ]]; then
  set_backend_endpoints "${TEST_BACKEND_PORT}"
fi

if [[ -n "${TEST_UI_PORT}" ]]; then
  set_ui_endpoint "${TEST_UI_PORT}"
fi

color() { printf "\033[%sm%s\033[0m" "$1" "$2"; }
log() { printf "%s\n" "$*"; }
log_info() { log "$(color '36' '[info]') $*"; }
log_ok() { log "$(color '32' '[ok]') $*"; }
log_err() { log "$(color '31' '[err]') $*"; }
die() { log_err "$*"; exit 1; }

require_cmd() { command -v "$1" >/dev/null 2>&1 || die "Missing required command: $1"; }

is_macos() { [ "$(uname -s)" = "Darwin" ]; }

port_in_use() { lsof -nP -iTCP:"$1" -sTCP:LISTEN >/dev/null 2>&1; }

find_available_port() {
  local start_port="$1"
  local max_offset="${2:-100}"
  local candidate

  for ((candidate=start_port; candidate<=start_port+max_offset; candidate++)); do
    if ! port_in_use "${candidate}"; then
      printf '%s' "${candidate}"
      return 0
    fi
  done

  return 1
}

allocate_ephemeral_port() {
  node -e "const net=require('net'); const s=net.createServer(); s.listen(0,'127.0.0.1',()=>{const p=s.address().port; console.log(p); s.close();}); s.on('error', (err)=>{console.error(err.message); process.exit(1);});"
}

clear_test_port() {
  local port="$1"
  local pids=""

  pids="$(lsof -nP -iTCP:"${port}" -sTCP:LISTEN -t 2>/dev/null || true)"
  if [[ -z "${pids}" ]]; then
    return 0
  fi

  log_info "Clearing stale listener(s) on test port ${port}: ${pids}"
  kill ${pids} >/dev/null 2>&1 || true
  sleep 1

  if port_in_use "${port}"; then
    pids="$(lsof -nP -iTCP:"${port}" -sTCP:LISTEN -t 2>/dev/null || true)"
    if [[ -n "${pids}" ]]; then
      log_info "Force-killing stubborn listener(s) on test port ${port}: ${pids}"
      kill -9 ${pids} >/dev/null 2>&1 || true
      sleep 1
    fi
  fi
}

wait_http() {
  local url="$1"
  local timeout="$2"
  local start
  start="$(date +%s)"
  while true; do
    if curl -fsS "$url" >/dev/null 2>&1; then return 0; fi
    local now
    now="$(date +%s)"
    if [ $((now - start)) -ge "$timeout" ]; then return 1; fi
    sleep 0.5
  done
}

create_run_dir() {
  local ts
  ts="$(date +%Y%m%d_%H%M%S)"
  local run_dir="${REPO_ROOT}/tests/_runs/${ts}"
  mkdir -p "${run_dir}/logs" "${run_dir}/tmp" "${run_dir}/playwright"
  echo "${run_dir}"
}

absolute_fixture_path() { (cd "${REPO_ROOT}/tests/fixtures/media" && pwd); }

is_postgres_target() {
  local target="${1:-}"
  local lower
  lower="$(printf '%s' "$target" | tr '[:upper:]' '[:lower:]')"
  [[ "$lower" == postgres://* || "$lower" == postgresql://* ]]
}

resolve_test_database_target() {
  local default_postgres_url="$1"
  if [[ -n "${RUSTFIN_TEST_DATABASE_URL:-}" ]]; then
    printf '%s' "${RUSTFIN_TEST_DATABASE_URL}"
    return
  fi
  if [[ -n "${RUSTFIN_DATABASE_URL:-}" ]]; then
    printf '%s' "${RUSTFIN_DATABASE_URL}"
    return
  fi
  printf '%s' "${default_postgres_url}"
}

start_server() {
  local run_dir="$1"
  local db_path="$2"
  local picker="$3"

  if [[ -z "${RUSTFIN_TEST_BACKEND_PORT:-}" ]]; then
    set_backend_endpoints "$(allocate_ephemeral_port)"
    log_info "Using dynamic backend test port ${TEST_BACKEND_PORT}"
  fi

  if port_in_use "${TEST_BACKEND_PORT}"; then
    clear_test_port "${TEST_BACKEND_PORT}"
  fi
  if port_in_use "${TEST_BACKEND_PORT}"; then
    set_backend_endpoints "$(find_available_port "$((TEST_BACKEND_PORT + 1))")" || \
      die "Unable to find a free backend test port."
    log_info "Using alternate backend test port ${TEST_BACKEND_PORT}"
  fi

  local db_target
  db_target="$(resolve_test_database_target "${db_path}")"
  log_info "Starting backend (rustfin-server) with PostgreSQL test target ..."

  (
    cd "${REPO_ROOT}"
    export RUSTFIN_DATABASE_URL="${db_target}"
    export RUSTFIN_BIND="${TEST_BACKEND_BIND}"
    export RUSTFIN_JWT_SECRET="rustyfin_test_secret"
    export RUSTFIN_CACHE_DIR="${run_dir}/tmp/cache"
    export RUSTFIN_TRANSCODE_DIR="${run_dir}/tmp/transcode"
    export RUSTFIN_MAX_TRANSCODES="1"
    export RUSTFIN_DIRECTORY_PICKER_PATH="${picker}"
    export TMPDIR="${TMPDIR}"
    mkdir -p "${RUSTFIN_CACHE_DIR}" "${RUSTFIN_TRANSCODE_DIR}"
    cargo run -p rustfin-server
  ) >"${run_dir}/logs/backend.log" 2>&1 &
  echo $! >"${run_dir}/tmp/backend.pid"

  if ! wait_http "${TEST_BACKEND_URL}/health" 40; then
    log_err "Backend did not become healthy. Last 80 lines:"
    tail -n 80 "${run_dir}/logs/backend.log" || true
    return 1
  fi
  log_ok "Backend is up (${TEST_BACKEND_URL})"
}

start_ui() {
  local run_dir="$1"

  if [[ -z "${RUSTFIN_TEST_UI_PORT:-}" ]]; then
    set_ui_endpoint "$(allocate_ephemeral_port)"
    log_info "Using dynamic UI test port ${TEST_UI_PORT}"
  fi

  if port_in_use "${TEST_UI_PORT}"; then
    clear_test_port "${TEST_UI_PORT}"
  fi
  if port_in_use "${TEST_UI_PORT}"; then
    set_ui_endpoint "$(find_available_port "$((TEST_UI_PORT + 1))")" || \
      die "Unable to find a free UI test port."
    log_info "Using alternate UI test port ${TEST_UI_PORT}"
  fi

  local standalone_entry="${REPO_ROOT}/ui/.next/standalone/server.js"
  if [[ ! -f "${standalone_entry}" ]]; then
    log_info "Building UI standalone bundle for smoke run ..."
    (
      cd "${REPO_ROOT}"
      export TMPDIR="${TMPDIR}"
      export npm_config_cache="${npm_config_cache}"
      export RUSTYFIN_API_BASE_URL="${TEST_BACKEND_URL}"
      npm --prefix ui run build
    ) >"${run_dir}/logs/ui-build.log" 2>&1 || {
      log_err "UI build failed. Last 80 lines:"
      tail -n 80 "${run_dir}/logs/ui-build.log" || true
      return 1
    }
  fi

  log_info "Starting UI (Next standalone server) ..."
  (
    cd "${REPO_ROOT}/ui/.next/standalone"
    export TMPDIR="${TMPDIR}"
    export npm_config_cache="${npm_config_cache}"
    export PORT="${TEST_UI_PORT}"
    export HOSTNAME="127.0.0.1"
    export RUSTYFIN_API_BASE_URL="${TEST_BACKEND_URL}"
    node ./server.js
  ) >"${run_dir}/logs/ui.log" 2>&1 &
  echo $! >"${run_dir}/tmp/ui.pid"

  if ! wait_http "${TEST_UI_URL}/" 60; then
    log_err "UI did not become ready. Last 80 lines:"
    tail -n 80 "${run_dir}/logs/ui.log" || true
    return 1
  fi
  log_ok "UI is up (${TEST_UI_URL})"
}

stop_services() {
  local run_dir="$1"
  log_info "Stopping services ..."

  set +e

  if [ -f "${run_dir}/tmp/ui.pid" ]; then
    local ui_pid
    ui_pid="$(cat "${run_dir}/tmp/ui.pid")"
    kill "${ui_pid}" >/dev/null 2>&1 || true
    wait "${ui_pid}" >/dev/null 2>&1 || true
  fi

  if [ -f "${run_dir}/tmp/backend.pid" ]; then
    local backend_pid
    backend_pid="$(cat "${run_dir}/tmp/backend.pid")"
    kill "${backend_pid}" >/dev/null 2>&1 || true
    wait "${backend_pid}" >/dev/null 2>&1 || true
  fi

  # Force-clean any stragglers still listening on test ports.
  local pids=""
  pids="$(lsof -nP -iTCP:"${TEST_UI_PORT}" -sTCP:LISTEN -t 2>/dev/null || true)"
  if [ -n "${pids}" ]; then
    kill ${pids} >/dev/null 2>&1 || true
  fi
  pids="$(lsof -nP -iTCP:"${TEST_BACKEND_PORT}" -sTCP:LISTEN -t 2>/dev/null || true)"
  if [ -n "${pids}" ]; then
    kill ${pids} >/dev/null 2>&1 || true
  fi

  sleep 0.8
}

run_playwright() {
  local run_dir="$1"
  local grep_pat="$2"
  shift 2

  (
    cd "${REPO_ROOT}/tests"
    export TMPDIR="${TMPDIR}"
    export npm_config_cache="${npm_config_cache}"
    export PLAYWRIGHT_BROWSERS_PATH="${PLAYWRIGHT_BROWSERS_PATH}"
    export RUSTYFIN_TEST_RUN_DIR="${run_dir}"
    export RUSTYFIN_BASE_URL="${TEST_UI_URL}"
    if [ -n "${grep_pat}" ]; then
      npx playwright test --config=playwright.config.ts -g "${grep_pat}" "$@"
    else
      npx playwright test --config=playwright.config.ts "$@"
    fi
  )
}
