#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  ./scripts/db/validate_sqlite_postgres_counts.sh --sqlite /path/to/rustfin.db --postgres-url postgresql://user:pass@host:5432/db

Compares row counts for all non-internal SQLite tables against PostgreSQL.
Exits non-zero when any mismatch is detected.
EOF
}

SQLITE_PATH=""
POSTGRES_URL=""

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

command -v sqlite3 >/dev/null 2>&1 || {
  echo "sqlite3 is required" >&2
  exit 1
}
command -v psql >/dev/null 2>&1 || {
  echo "psql is required" >&2
  exit 1
}

if [[ ! -f "$SQLITE_PATH" ]]; then
  echo "SQLite DB not found: $SQLITE_PATH" >&2
  exit 1
fi

readarray -t tables < <(
  sqlite3 "$SQLITE_PATH" \
    "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name;"
)

if [[ ${#tables[@]} -eq 0 ]]; then
  echo "No tables found in SQLite source database."
  exit 1
fi

printf '%-45s %-12s %-12s %-8s\n' "table" "sqlite" "postgres" "status"
printf '%-45s %-12s %-12s %-8s\n' "---------------------------------------------" "------------" "------------" "--------"

mismatch=0
for table in "${tables[@]}"; do
  [[ -n "$table" ]] || continue
  sqlite_count="$(
    sqlite3 "$SQLITE_PATH" "SELECT COUNT(*) FROM \"$table\";"
  )"
  postgres_count="$(
    psql "$POSTGRES_URL" -Atqc "SELECT COUNT(*) FROM \"$table\";" 2>/dev/null || echo "__missing__"
  )"

  status="ok"
  if [[ "$postgres_count" == "__missing__" ]]; then
    status="missing"
    mismatch=1
  elif [[ "$sqlite_count" != "$postgres_count" ]]; then
    status="mismatch"
    mismatch=1
  fi

  printf '%-45s %-12s %-12s %-8s\n' "$table" "$sqlite_count" "$postgres_count" "$status"
done

if [[ $mismatch -ne 0 ]]; then
  echo "Validation failed: table count mismatches detected." >&2
  exit 1
fi

echo "Validation passed: SQLite and PostgreSQL row counts match."
