#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "${REPO_ROOT}/tests/lib/harness.sh"

RUN_DIR="$(create_run_dir)"
trap 'stop_services "${RUN_DIR}"' EXIT

log_info "Run dir: ${RUN_DIR}"
PICKER="$(absolute_fixture_path)"
DB="${RUSTFIN_TEST_DATABASE_URL:-postgresql://rustfin:rustfin.0.0.1:5432/rustfin_test}"

start_server "${RUN_DIR}" "${DB}" "${PICKER}"
start_ui "${RUN_DIR}"

run_playwright "${RUN_DIR}" "@setup"
log_ok "Setup E2E finished"

