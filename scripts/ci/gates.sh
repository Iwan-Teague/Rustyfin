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

# ── Toolchain pin preamble (AQ-233) ────────────────────────────────────
# The gate must run the exact toolchain rust-toolchain.toml pins — never
# the ambient one. An ambient cargo that is not a rustup proxy (Homebrew's
# on macOS) ignores rust-toolchain.toml entirely, so this preamble
# resolves the pinned toolchain's bin dir via `rustup which` and puts it
# FIRST on PATH, then refuses (exit 1) unless the rustc and clippy that
# PATH now finds match the pin's minor. A missing or malformed
# rust-toolchain.toml, a missing rustup, or an uninstalled pin are hard
# refusals — the gate never silently starts on the ambient toolchain.
# Parsing is grep + parameter expansion only (no sed/awk/read: BSD/GNU
# divergence).
toolchain_fail() {
  printf 'GATE REFUSED (toolchain pin): %s\n' "$1" >&2
  exit 1
}
[ -f rust-toolchain.toml ] \
  || toolchain_fail 'rust-toolchain.toml missing — refusing to gate on the ambient toolchain'
channel_count=$(grep -c '^channel[[:space:]]*=' rust-toolchain.toml) || channel_count=0
[ "$channel_count" -eq 1 ] \
  || toolchain_fail "expected exactly one 'channel =' line in rust-toolchain.toml, found $channel_count"
channel_line=$(grep '^channel[[:space:]]*=' rust-toolchain.toml)
channel_value=${channel_line#*=}
channel_value=${channel_value#"${channel_value%%[![:space:]]*}"}
channel_value=${channel_value%"${channel_value##*[![:space:]]}"}
case $channel_value in
  \"*\") channel=${channel_value#\"} ;;
  *) toolchain_fail "channel value is not a double-quoted string: $channel_value" ;;
esac
channel=${channel%\"}
case $channel in
  [0-9]*.[0-9]*|[0-9]*.[0-9]*.[0-9]*) ;;
  *) toolchain_fail "unsupported channel '$channel' — this gate requires a concrete X.Y or X.Y.Z pin so the clippy-minor assertion is well-defined" ;;
esac
command -v rustup >/dev/null 2>&1 \
  || toolchain_fail 'rustup not on PATH — cannot resolve the pinned toolchain (an ambient-only install refuses here by design)'
pinned_cargo=$(rustup which --toolchain "$channel" cargo 2>/dev/null) \
  || toolchain_fail "toolchain '$channel' is not installed — run: rustup toolchain install $channel"
[ -x "$pinned_cargo" ] || toolchain_fail "resolved cargo is not executable: $pinned_cargo"
PATH=${pinned_cargo%/*}:$PATH
export PATH
# Assert the toolchain PATH now resolves really is the pin: rustc minor
# must equal the pinned minor, and clippy (0.1.N) must track that rustc.
rustc_banner=$(rustc --version 2>/dev/null) \
  || toolchain_fail 'rustc --version failed under the pinned PATH'
rustc_ver=${rustc_banner#* }
rustc_ver=${rustc_ver%% *}
case $rustc_ver in
  [0-9]*.[0-9]*) ;;
  *) toolchain_fail "unparsable 'rustc --version' output: $rustc_banner" ;;
esac
rustc_minor=${rustc_ver#*.}
rustc_minor=${rustc_minor%%.*}
clippy_banner=$(cargo clippy --version 2>/dev/null) \
  || toolchain_fail 'cargo clippy --version failed under the pinned PATH'
clippy_ver=${clippy_banner#* }
clippy_ver=${clippy_ver%% *}
case $clippy_ver in
  0.*.*) ;;
  *) toolchain_fail "unparsable 'cargo clippy --version' output: $clippy_banner" ;;
esac
clippy_minor=${clippy_ver#*.}
clippy_minor=${clippy_minor#*.}
clippy_minor=${clippy_minor%%[!0-9]*}
pin_minor=${channel#*.}
pin_minor=${pin_minor%%.*}
if [ "$rustc_minor" -ne "$pin_minor" ] || [ "$clippy_minor" -ne "$rustc_minor" ]; then
  toolchain_fail "PATH resolves rustc $rustc_ver / clippy $clippy_ver, not the pinned $channel"
fi
printf 'gate toolchain: pin %s — rustc %s, clippy %s (pinned bin dir first on PATH)\n' \
  "$channel" "$rustc_ver" "$clippy_ver"

printf '=== 1/2 cargo deny check advisories bans sources ===\n'
cargo deny check advisories bans sources

printf '=== 2/2 native quality gates (scripts/ci/debian_native_gates.sh) ===\n'
bash scripts/ci/debian_native_gates.sh \
  --allow-non-debian \
  --skip-browser-smoke \
  --skip-judge \
  --report .tmp/gates/ci-native-gates.md

printf 'All rustyfin gates passed.\n'
