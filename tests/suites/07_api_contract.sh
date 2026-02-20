#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "${REPO_ROOT}/tests/lib/harness.sh"

RUN_DIR="$(create_run_dir)"
trap 'stop_services "${RUN_DIR}"' EXIT

log_info "Run dir: ${RUN_DIR}"
PICKER="$(absolute_fixture_path)"
DB="${RUN_DIR}/tmp/rustfin_api.db"

start_server "${RUN_DIR}" "${DB}" "${PICKER}"

log_info "API contract checks (curl)"

curl -fsS "${TEST_BACKEND_URL}/health" >/dev/null
log_ok "/health OK"

code="$(curl -s -o /dev/null -w '%{http_code}' "${TEST_BACKEND_URL}/api/v1/users")"
[ "${code}" = "401" ] || die "Expected /api/v1/users to be 401 without token, got ${code}"
log_ok "Unauthed /users -> 401"

code="$(curl -s -o /dev/null -w '%{http_code}' "${TEST_BACKEND_URL}/api/v1/system/info/public")"
[ "${code}" = "200" ] || die "Expected /system/info/public 200, got ${code}"
log_ok "Public system info -> 200"

log_ok "API contract checks finished"
