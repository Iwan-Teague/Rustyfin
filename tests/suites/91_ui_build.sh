#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "${REPO_ROOT}/tests/lib/harness.sh"

has_eslint_config() {
  [ -f "${REPO_ROOT}/ui/.eslintrc" ] || \
  [ -f "${REPO_ROOT}/ui/.eslintrc.js" ] || \
  [ -f "${REPO_ROOT}/ui/.eslintrc.cjs" ] || \
  [ -f "${REPO_ROOT}/ui/.eslintrc.json" ] || \
  [ -f "${REPO_ROOT}/ui/eslint.config.js" ] || \
  [ -f "${REPO_ROOT}/ui/eslint.config.cjs" ] || \
  [ -f "${REPO_ROOT}/ui/eslint.config.mjs" ]
}

log_info "UI lint + build"
cd "${REPO_ROOT}"

if has_eslint_config; then
  CI=1 npm --prefix ui run lint
else
  log_info "Skipping UI lint: no ESLint config found in ui/"
fi

npm --prefix ui run build
log_ok "UI lint/build passed"
