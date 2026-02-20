#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TESTS_DIR="${REPO_ROOT}/tests"
UI_DIR="${REPO_ROOT}/ui"

# Stable writable temp/cache dirs (macOS-safe)
SAFE_TMP_DIR="${RUSTFIN_TMPDIR:-${REPO_ROOT}/.tmp}"
SAFE_NPM_CACHE="${RUSTFIN_NPM_CACHE:-${REPO_ROOT}/.npm-cache}"
SAFE_PW_BROWSERS="${RUSTFIN_PLAYWRIGHT_BROWSERS:-${REPO_ROOT}/.playwright-browsers}"

mkdir -p "${SAFE_TMP_DIR}" "${SAFE_NPM_CACHE}" "${SAFE_PW_BROWSERS}"
chmod 700 "${SAFE_TMP_DIR}" 2>/dev/null || true

export TMPDIR="${SAFE_TMP_DIR}"
export npm_config_cache="${SAFE_NPM_CACHE}"
export PLAYWRIGHT_BROWSERS_PATH="${SAFE_PW_BROWSERS}"

echo "[bootstrap] Using TMPDIR: ${TMPDIR}"
echo "[bootstrap] Using npm cache: ${npm_config_cache}"
echo "[bootstrap] Using Playwright browsers path: ${PLAYWRIGHT_BROWSERS_PATH}"

echo "[bootstrap] Installing JS deps for tests (Playwright, wait-on, axe)..."
cd "${TESTS_DIR}"
if [[ -f package-lock.json ]]; then
  npm ci
else
  npm install
fi

echo "[bootstrap] Installing Playwright browsers..."
npx playwright install

echo "[bootstrap] Installing UI deps..."
cd "${REPO_ROOT}"
if [[ -f "${UI_DIR}/package-lock.json" ]]; then
  npm --prefix "${UI_DIR}" ci
else
  npm --prefix "${UI_DIR}" install
fi

echo "[bootstrap] Done."
