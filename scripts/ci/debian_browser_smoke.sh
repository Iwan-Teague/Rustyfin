#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "${REPO_ROOT}/tests/lib/harness.sh"

if [[ -f "${REPO_ROOT}/.rustyfin.runtime.env" ]]; then
  # shellcheck disable=SC1091
  source "${REPO_ROOT}/.rustyfin.runtime.env"
fi

require_cmd cargo
require_cmd npm
require_cmd curl
require_cmd lsof
require_cmd psql

ensure_tests_dependencies() {
  if [[ ! -x "${REPO_ROOT}/tests/node_modules/.bin/playwright" ]]; then
    log_info "Installing test harness JS dependencies ..."
    npm --prefix "${REPO_ROOT}/tests" ci
  fi
}

ensure_playwright_browser() {
  if ! find "${PLAYWRIGHT_BROWSERS_PATH}" -maxdepth 1 -type d -name 'chromium-*' | grep -q .; then
    log_info "Installing Playwright Chromium browser ..."
    (
      cd "${REPO_ROOT}/tests"
      export PLAYWRIGHT_BROWSERS_PATH="${PLAYWRIGHT_BROWSERS_PATH}"
      npx playwright install chromium
    )
  fi
}

bootstrap_setup_for_smoke() {
  local admin_username="${RUSTFIN_ADMIN_USERNAME:-admin}"
  local admin_password="${RUSTFIN_ADMIN_PASSWORD:-AdminPassword123!}"
  local owner_token
  local claim_response

  log_info "Bootstrapping setup state for browser smoke ..."

  claim_response="$(curl -fsS \
    -X POST "${TEST_BACKEND_URL}/api/v1/setup/session/claim" \
    -H 'Content-Type: application/json' \
    -d '{"client_name":"DebianSmoke","force":false,"confirm_takeover":false}')"
  owner_token="$(printf '%s' "${claim_response}" | jq -r '.owner_token')"
  [[ -n "${owner_token}" && "${owner_token}" != "null" ]] || die "Failed to claim setup session for smoke run."

  curl -fsS \
    -X PUT "${TEST_BACKEND_URL}/api/v1/setup/config" \
    -H 'Content-Type: application/json' \
    -H "X-Setup-Owner-Token: ${owner_token}" \
    -H "X-Setup-Remote-Token: ${owner_token}" \
    -d '{"server_name":"Rustyfin Smoke","default_ui_locale":"en-GB","default_region":"GB","default_time_zone":"Europe/London"}' \
    >/dev/null

  curl -fsS \
    -X POST "${TEST_BACKEND_URL}/api/v1/setup/admin" \
    -H 'Content-Type: application/json' \
    -H "X-Setup-Owner-Token: ${owner_token}" \
    -H "X-Setup-Remote-Token: ${owner_token}" \
    -H "Idempotency-Key: smoke-admin-bootstrap-0001" \
    -d "{\"username\":\"${admin_username}\",\"password\":\"${admin_password}\"}" \
    >/dev/null

  curl -fsS \
    -X PUT "${TEST_BACKEND_URL}/api/v1/setup/metadata" \
    -H 'Content-Type: application/json' \
    -H "X-Setup-Owner-Token: ${owner_token}" \
    -H "X-Setup-Remote-Token: ${owner_token}" \
    -d '{"metadata_language":"en","metadata_region":"GB"}' \
    >/dev/null

  curl -fsS \
    -X PUT "${TEST_BACKEND_URL}/api/v1/setup/network" \
    -H 'Content-Type: application/json' \
    -H "X-Setup-Owner-Token: ${owner_token}" \
    -H "X-Setup-Remote-Token: ${owner_token}" \
    -d '{"allow_remote_access":false,"trusted_proxies":[]}' \
    >/dev/null

  curl -fsS \
    -X POST "${TEST_BACKEND_URL}/api/v1/setup/complete" \
    -H 'Content-Type: application/json' \
    -H "X-Setup-Owner-Token: ${owner_token}" \
    -H "X-Setup-Remote-Token: ${owner_token}" \
    -d '{"confirm":true}' \
    >/dev/null

  log_ok "Smoke setup bootstrap completed"
}

build_smoke_db_url() {
  local base_url="$1"
  local schema_name="$2"
  local options_param="options=-c%20search_path%3D${schema_name}"

  if [[ "${base_url}" == *\?* ]]; then
    printf '%s&%s' "${base_url}" "${options_param}"
  else
    printf '%s?%s' "${base_url}" "${options_param}"
  fi
}

cleanup_schema() {
  local base_url="$1"
  local schema_name="$2"
  psql "${base_url}" -v ON_ERROR_STOP=1 -c "DROP SCHEMA IF EXISTS ${schema_name} CASCADE;" >/dev/null
}

find_native_server_bin() {
  find "${REPO_ROOT}/.native-bins" -type f -name rustfin-server | sort | head -n 1
}

start_native_test_backend() {
  local run_dir="$1"
  local db_url="$2"
  local picker="$3"
  local server_bin="$4"

  [[ -x "${server_bin}" ]] || die "Native rustfin-server binary not found: ${server_bin}"

  if [[ -z "${TEST_BACKEND_PORT}" || -z "${TEST_BACKEND_BIND}" || -z "${TEST_BACKEND_URL}" ]]; then
    if [[ -n "${RUSTFIN_TEST_BACKEND_PORT:-}" ]]; then
      set_backend_endpoints "${RUSTFIN_TEST_BACKEND_PORT}"
    else
      set_backend_endpoints "$(allocate_ephemeral_port)"
      log_info "Using dynamic backend test port ${TEST_BACKEND_PORT}"
    fi
  fi

  if port_in_use "${TEST_BACKEND_PORT}"; then
    clear_test_port "${TEST_BACKEND_PORT}"
  fi

  if port_in_use "${TEST_BACKEND_PORT}"; then
    set_backend_endpoints "$(find_available_port "$((TEST_BACKEND_PORT + 1))")" || \
      die "Unable to find a free backend test port."
    log_info "Using alternate backend test port ${TEST_BACKEND_PORT}"
  fi

  log_info "Starting backend from native rustfin-server binary ..."
  (
    cd "${REPO_ROOT}"
    export RUSTFIN_DATABASE_URL="${db_url}"
    export RUSTFIN_BIND="${TEST_BACKEND_BIND}"
    export RUSTFIN_JWT_SECRET="rustyfin_test_secret"
    export RUSTFIN_CACHE_DIR="${run_dir}/tmp/cache"
    export RUSTFIN_TRANSCODE_DIR="${run_dir}/tmp/transcode"
    export RUSTFIN_MAX_TRANSCODES="1"
    export RUSTFIN_DIRECTORY_PICKER_PATH="${picker}"
    export TMPDIR="${TMPDIR}"
    mkdir -p "${RUSTFIN_CACHE_DIR}" "${RUSTFIN_TRANSCODE_DIR}"
    "${server_bin}"
  ) >"${run_dir}/logs/backend.log" 2>&1 &
  echo $! >"${run_dir}/tmp/backend.pid"

  if ! wait_http "${TEST_BACKEND_URL}/health" 60; then
    log_err "Backend did not become healthy. Last 80 lines:"
    tail -n 80 "${run_dir}/logs/backend.log" || true
    return 1
  fi

  log_ok "Backend is up (${TEST_BACKEND_URL})"
}

RUN_DIR="$(create_run_dir)"
SMOKE_SCHEMA="rustfin_smoke_$(date +%s)_$$"
BASE_DB_URL="${RUSTFIN_TEST_DATABASE_URL:-${RUSTFIN_DATABASE_URL:-}}"
[[ -n "${BASE_DB_URL}" ]] || die "RUSTFIN_DATABASE_URL is required to run the Debian browser smoke suite."

trap 'stop_services "${RUN_DIR}"; cleanup_schema "${BASE_DB_URL}" "${SMOKE_SCHEMA}"' EXIT

log_info "Run dir: ${RUN_DIR}"
log_info "Smoke schema: ${SMOKE_SCHEMA}"

ensure_tests_dependencies
ensure_playwright_browser

psql "${BASE_DB_URL}" -v ON_ERROR_STOP=1 -c "CREATE SCHEMA ${SMOKE_SCHEMA};" >/dev/null

PICKER="$(absolute_fixture_path)"
SMOKE_DB_URL="$(build_smoke_db_url "${BASE_DB_URL}" "${SMOKE_SCHEMA}")"
SERVER_BIN="$(find_native_server_bin)"
export RUSTYFIN_TEST_LIBRARY_PATH="${PICKER}/movies"

start_native_test_backend "${RUN_DIR}" "${SMOKE_DB_URL}" "${PICKER}" "${SERVER_BIN}"
bootstrap_setup_for_smoke
start_ui "${RUN_DIR}"

run_playwright "${RUN_DIR}" "@debian-native-smoke"
log_ok "Debian native browser smoke finished"
