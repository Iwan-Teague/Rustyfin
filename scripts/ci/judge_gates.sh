#!/usr/bin/env bash
set -euo pipefail

MODE="smoke"
ARTIFACTS_DIR=""

usage() {
  cat <<'EOF'
Usage:
  ./scripts/ci/judge_gates.sh [options]

Purpose:
  Run the Rust-backed AI judge gate in smoke or release mode and publish stable
  artifacts under ./.tmp/gates/.

Options:
  --mode MODE          Gate mode: smoke or release. Default: smoke.
  --artifacts-dir DIR  Artifact output directory. Defaults to a timestamped
                       path under ./.tmp/gates/.
  -h, --help           Show this help.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode)
      [[ $# -ge 2 ]] || { echo "--mode requires a value" >&2; exit 1; }
      MODE="$2"
      shift 2
      ;;
    --artifacts-dir)
      [[ $# -ge 2 ]] || { echo "--artifacts-dir requires a path" >&2; exit 1; }
      ARTIFACTS_DIR="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

case "$MODE" in
  smoke|release) ;;
  *)
    echo "Unsupported judge gate mode: $MODE" >&2
    exit 1
    ;;
esac

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

if [[ -z "${CARGO_HOME:-}" && -f "${HOME}/.cargo/env" ]]; then
  # shellcheck disable=SC1090
  source "${HOME}/.cargo/env"
fi

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
GENERATED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
REPORT_ROOT="$REPO_ROOT/.tmp/gates"
mkdir -p "$REPORT_ROOT"

if [[ -z "$ARTIFACTS_DIR" ]]; then
  ARTIFACTS_DIR="$REPORT_ROOT/ai-judge-${MODE}-${RUN_ID}"
fi
mkdir -p "$ARTIFACTS_DIR"

GIT_SHA="$(git rev-parse HEAD 2>/dev/null || printf 'unknown')"
BASE_SHA="$(git merge-base HEAD main 2>/dev/null || git merge-base HEAD origin/main 2>/dev/null || printf '%s' "$GIT_SHA")"
RUN_LABEL="judge-${MODE}-${RUN_ID}"
TIMEZONE_VALUE="${RUSTFIN_AI_EVAL_TIMEZONE:-${TZ:-UTC}}"
LOCALE_VALUE="${RUSTFIN_AI_EVAL_LOCALE:-${LC_ALL:-${LANG:-en-IE}}}"

set +e
cargo run -p ai-evals -- gate \
  --mode "$MODE" \
  --artifacts-dir "$ARTIFACTS_DIR" \
  --generated-at "$GENERATED_AT" \
  --run-id "$RUN_LABEL" \
  --git-sha "$GIT_SHA" \
  --base-sha "$BASE_SHA" \
  --timezone "$TIMEZONE_VALUE" \
  --locale "$LOCALE_VALUE"
RC=$?
set -e

LATEST_DIR="$REPORT_ROOT/ai-judge-${MODE}-latest"
mkdir -p "$LATEST_DIR"
cp -f "$ARTIFACTS_DIR"/* "$LATEST_DIR"/ 2>/dev/null || true

echo "[judge-gates] mode: $MODE"
echo "[judge-gates] artifacts: $ARTIFACTS_DIR"
echo "[judge-gates] latest copy: $LATEST_DIR"

exit "$RC"
