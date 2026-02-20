#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SUITE="${1:-}"

if [ -z "${SUITE}" ]; then
  echo "Usage: ./tests/run-suite.sh <suite_id>"
  echo "Example: ./tests/run-suite.sh 02_auth"
  exit 2
fi

SCRIPT="${REPO_ROOT}/tests/suites/${SUITE}.sh"
if [ ! -f "${SCRIPT}" ]; then
  echo "Unknown suite: ${SUITE}"
  echo "Available suites:"
  ls -1 "${REPO_ROOT}/tests/suites" | sed 's/\.sh$//' | sort
  exit 2
fi

exec "${SCRIPT}"

