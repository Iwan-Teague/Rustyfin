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
# Environment semantics (r106 F10, revised by AQ-233): on GitHub CI
# (GITHUB_ACTIONS=true, which provisions Postgres, psql and node) every gate
# is fail-closed exactly as before -- missing host tooling or a missing
# RUSTFIN_DATABASE_URL FAILS the corresponding gate and is never skipped
# into green. On any other host, only the DB-/UI-dependent gates (setup
# integration, migration query, UI lint/typecheck/build) SKIP (exit 222,
# the protocol --allow-non-debian already uses) when their prerequisites
# are absent, each with a message naming the missing piece and pointing at
# CI, which runs them fail-closed on every push; every gate that can still
# make an assertion (fmt, clippy, the Rust test gates, docs, syntax,
# runtime/docker checks) still runs and still fails here. Full local cost
# without UI/node is: workspace compile + clippy + the Rust test gates.
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
