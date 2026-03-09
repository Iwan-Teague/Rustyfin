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

start_server "${RUN_DIR}" "${SMOKE_DB_URL}" "${PICKER}"
start_ui "${RUN_DIR}"

run_playwright "${RUN_DIR}" "@debian-native-smoke"
log_ok "Debian native browser smoke finished"
