#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  ./scripts/db/migrate_sqlite_to_postgres.sh --sqlite /path/to/rustfin.db --postgres-url postgresql://user:pass@host:5432/db

Options:
  --sqlite        Path to source SQLite database file.
  --postgres-url  Destination PostgreSQL URL.
  --skip-validate Skip row-count validation step.
  -h, --help      Show this help.

Notes:
  - Uses pgloader in Docker to perform the bulk migration.
  - Intended for one-off cutover and dry-run validation.
EOF
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

SQLITE_PATH=""
POSTGRES_URL=""
SKIP_VALIDATE=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --sqlite)
      [[ $# -ge 2 ]] || { echo "Missing value for --sqlite" >&2; exit 1; }
      SQLITE_PATH="$2"
      shift 2
      ;;
    --postgres-url)
      [[ $# -ge 2 ]] || { echo "Missing value for --postgres-url" >&2; exit 1; }
      POSTGRES_URL="$2"
      shift 2
      ;;
    --skip-validate)
      SKIP_VALIDATE=true
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

[[ -n "$SQLITE_PATH" ]] || { echo "--sqlite is required" >&2; exit 1; }
[[ -n "$POSTGRES_URL" ]] || { echo "--postgres-url is required" >&2; exit 1; }

if [[ ! -f "$SQLITE_PATH" ]]; then
  echo "SQLite DB not found: $SQLITE_PATH" >&2
  exit 1
fi

command -v docker >/dev/null 2>&1 || {
  echo "docker is required" >&2
  exit 1
}

sqlite_dir="$(cd "$(dirname "$SQLITE_PATH")" && pwd)"
sqlite_file="$(basename "$SQLITE_PATH")"
sqlite_in_container="/sqlite/${sqlite_file}"

echo "[migrate] source sqlite: ${SQLITE_PATH}"
echo "[migrate] target postgres: ${POSTGRES_URL}"
echo "[migrate] running pgloader..."

docker run --rm \
  -v "${sqlite_dir}:/sqlite:ro" \
  dimitri/pgloader:latest \
  pgloader "sqlite://${sqlite_in_container}" "${POSTGRES_URL}"

echo "[migrate] pgloader migration complete."

if [[ "$SKIP_VALIDATE" == "true" ]]; then
  echo "[migrate] validation skipped (--skip-validate)."
  exit 0
fi

echo "[migrate] validating row counts..."
"${REPO_ROOT}/scripts/db/validate_sqlite_postgres_counts.sh" \
  --sqlite "${SQLITE_PATH}" \
  --postgres-url "${POSTGRES_URL}"

echo "[migrate] done."
