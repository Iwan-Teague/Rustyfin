#!/bin/sh
# rustyfin member gate entrypoint (AQ-148).
#
# Delegates to the project's own single-source-of-truth gate harness
# (scripts/ci/debian_native_gates.sh) with EXACTLY the flags its CI passes
# (.github/workflows/ci.yml, "Run native quality gates" step):
#   --allow-non-debian   the supported-host gate SKIPs (exit 222) instead of
#                        failing off-Debian, and systemctl drops out of the
#                        required-tooling probe; runtime/systemd gates
#                        self-SKIP where the native service is absent;
#   --skip-browser-smoke the Playwright suite needs a live deployed backend;
#   --skip-judge         the AI judge gate runs in the dedicated ai-judge-*
#                        workflows; running it here too would double it.
# The harness names every gate in its output (PASS/FAIL/SKIP per gate) and
# exits 1 when any gate FAILED. This wrapper adds only the supply-chain gate
# its CI runs as a separate step, so local and CI gate the identical set.
#
# Prerequisites (r106 F10): two gates fail-closed on missing environment, so
# provision them before running locally:
#   - RUSTFIN_DATABASE_URL  required by the "Rust setup integration" gate
#                           (a live postgres; export the URL or that gate
#                           fails with "RUSTFIN_DATABASE_URL is required").
#   - ui/node_modules       required by the UI gates ("UI dependencies
#                           present", lint, typecheck, production build);
#                           populate with (cd ui && npm ci).
# Fails closed: missing host tooling (jq, curl, lsof, psql, ffprobe, node,
# npm, a populated ui/node_modules, Postgres for the setup-integration gate)
# FAILS the corresponding gate -- it is never skipped into green. Full local
# cost is CI-scale: workspace compile + clippy + the Rust test gates + UI
# lint/typecheck/production build (CI allows 90 minutes at 2 cargo jobs).
set -eu

cd "$(dirname "$0")/../.."

printf '=== 1/2 cargo deny check advisories bans sources ===\n'
cargo deny check advisories bans sources

printf '=== 2/2 native quality gates (scripts/ci/debian_native_gates.sh) ===\n'
bash scripts/ci/debian_native_gates.sh \
  --allow-non-debian \
  --skip-browser-smoke \
  --skip-judge \
  --report .tmp/gates/ci-native-gates.md

printf 'All rustyfin gates passed.\n'
