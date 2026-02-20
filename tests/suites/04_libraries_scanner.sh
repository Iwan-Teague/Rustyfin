#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "${REPO_ROOT}/tests/lib/harness.sh"

RUN_DIR="$(create_run_dir)"
trap 'stop_services "${RUN_DIR}"' EXIT

log_info "Run dir: ${RUN_DIR}"
PICKER="$(absolute_fixture_path)"
DB="${RUN_DIR}/tmp/rustfin_libraries.db"

start_server "${RUN_DIR}" "${DB}" "${PICKER}"
start_ui "${RUN_DIR}"

run_playwright "${RUN_DIR}" "@libraries|@scanner"
log_ok "Libraries/scanner E2E finished"

