#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

SUITES=(
  "90_rust_unit"
  "91_ui_build"
  "00_smoke"
  "01_setup"
  "02_auth"
  "03_users_permissions"
  "04_libraries_scanner"
  "05_directory_picker"
  "06_accessibility"
  "07_api_contract"
)

echo "[test-all] Running suites:"
printf ' - %s\n' "${SUITES[@]}"

FAIL=0
for s in "${SUITES[@]}"; do
  echo
  echo "==================== SUITE ${s} ===================="
  if "${REPO_ROOT}/tests/suites/${s}.sh"; then
    echo "[test-all] ✅ ${s} PASS"
  else
    echo "[test-all] ❌ ${s} FAIL"
    FAIL=1
  fi
done

echo
if [ "${FAIL}" -eq 0 ]; then
  echo "[test-all] ✅ ALL SUITES PASSED"
  exit 0
else
  echo "[test-all] ❌ SOME SUITES FAILED"
  exit 1
fi

