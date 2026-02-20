#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "${REPO_ROOT}/tests/lib/harness.sh"

log_info "Rust unit/integration tests"
cd "${REPO_ROOT}"
cargo test
log_ok "cargo test passed"

