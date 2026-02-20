#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "${REPO_ROOT}/tests/lib/harness.sh"

RUN_DIR="$(create_run_dir)"
trap 'stop_services "${RUN_DIR}"' EXIT

log_info "Run dir: ${RUN_DIR}"
require_cmd cargo
require_cmd npm
require_cmd curl
require_cmd lsof

PICKER="$(absolute_fixture_path)"
DB="${RUN_DIR}/tmp/rustfin_smoke.db"

start_server "${RUN_DIR}" "${DB}" "${PICKER}"
start_ui "${RUN_DIR}"

run_playwright "${RUN_DIR}" "@smoke"
log_ok "Smoke E2E finished"

