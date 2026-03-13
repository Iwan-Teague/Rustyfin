#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

if [[ -z "${CARGO_HOME:-}" && -f "${HOME}/.cargo/env" ]]; then
  # shellcheck disable=SC1090
  source "${HOME}/.cargo/env"
fi

echo "[rustyvault-gates] Verifying runtime-unavailable isolation behavior ..."
cargo test -p rustfin-server rustyvault_unavailable_isolated_from_non_vault_routes

echo "[rustyvault-gates] Verifying backend compiles without RustyVault ..."
cargo check -p rustfin-server --no-default-features

echo "[rustyvault-gates] Verifying host UI build succeeds with Vault disabled ..."
NEXT_PUBLIC_RUSTYVAULT_ENABLED=0 npm --prefix ui run build

echo "[rustyvault-gates] All RustyVault removability gates passed."
