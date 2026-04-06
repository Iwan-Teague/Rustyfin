#!/usr/bin/env bash
set -uo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
RESET='\033[0m'

info()    { echo -e "${CYAN}[debian-gates]${RESET} $*"; }
success() { echo -e "${GREEN}[debian-gates]${RESET} $*"; }
warn()    { echo -e "${YELLOW}[debian-gates]${RESET} $*"; }
error()   { echo -e "${RED}[debian-gates]${RESET} $*" >&2; }
die()     { error "$*"; exit 1; }

SKIP_CODE=222
SKIP_RUNTIME=false
SKIP_UI=false
SKIP_CLIPPY=false
SKIP_TESTS=false
SKIP_BROWSER_SMOKE=false
ALLOW_NON_DEBIAN=false
REPORT_PATH=""
CARGO_GATE_JOBS="${RUSTFIN_GATE_CARGO_JOBS:-1}"

usage() {
  cat <<'EOF'
Usage:
  ./scripts/ci/debian_native_gates.sh [options]

Purpose:
  Run the curated supported-Debian native quality gates for Rustyfin and emit a
  Markdown report with the results.

Options:
  --skip-runtime        Skip runtime/systemd/health endpoint checks.
  --skip-ui             Skip UI lint/typecheck/build gates.
  --skip-clippy         Skip strict clippy gates.
  --skip-tests          Skip Rust test gates.
  --skip-browser-smoke  Skip the isolated Playwright browser smoke suite.
  --allow-non-debian    Run outside supported Debian hosts (runtime confidence is reduced).
  --report PATH         Write the Markdown report to PATH.
  -h, --help            Show this help.

Default report output:
  ./.tmp/gates/debian-native-gates-latest.md
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-runtime) SKIP_RUNTIME=true; shift ;;
    --skip-ui) SKIP_UI=true; shift ;;
    --skip-clippy) SKIP_CLIPPY=true; shift ;;
    --skip-tests) SKIP_TESTS=true; shift ;;
    --skip-browser-smoke) SKIP_BROWSER_SMOKE=true; shift ;;
    --allow-non-debian) ALLOW_NON_DEBIAN=true; shift ;;
    --report)
      [[ $# -ge 2 ]] || die "--report requires a path"
      REPORT_PATH="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "Unknown argument: $1"
      ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

if [[ -z "${CARGO_HOME:-}" && -f "${HOME}/.cargo/env" ]]; then
  # shellcheck disable=SC1090
  source "${HOME}/.cargo/env"
fi

if [[ -f "$REPO_ROOT/.rustyfin.runtime.env" ]]; then
  # shellcheck disable=SC1091
  source "$REPO_ROOT/.rustyfin.runtime.env"
fi

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
REPORT_ROOT="$REPO_ROOT/.tmp/gates"
LOG_DIR="$REPORT_ROOT/$RUN_ID"
mkdir -p "$LOG_DIR"

if [[ -z "$REPORT_PATH" ]]; then
  REPORT_PATH="$REPORT_ROOT/debian-native-gates-$RUN_ID.md"
fi
LATEST_REPORT="$REPORT_ROOT/debian-native-gates-latest.md"

if [[ "$(id -u)" -eq 0 ]]; then
  RUN_ROOT=()
  RUN_POSTGRES=(runuser -u postgres -- psql)
else
  RUN_ROOT=(sudo -n)
  RUN_POSTGRES=(sudo -n -u postgres psql)
fi

PASS_COUNT=0
FAIL_COUNT=0
SKIP_COUNT=0
declare -a RESULT_ROWS=()

slugify() {
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]' | sed -E 's/[^a-z0-9]+/-/g; s/^-+//; s/-+$//'
}

record_result() {
  local status="$1"
  local label="$2"
  local duration="$3"
  local logfile="$4"
  RESULT_ROWS+=("$status|$label|$duration|$logfile")
  case "$status" in
    PASS) PASS_COUNT=$((PASS_COUNT + 1)); success "PASS: $label ($duration)" ;;
    FAIL) FAIL_COUNT=$((FAIL_COUNT + 1)); error "FAIL: $label ($duration)"; error "  log: $logfile" ;;
    SKIP) SKIP_COUNT=$((SKIP_COUNT + 1)); warn "SKIP: $label ($duration)"; warn "  log: $logfile" ;;
  esac
}

run_gate() {
  local label="$1"
  shift
  local slug logfile start_ts end_ts duration rc
  slug="$(slugify "$label")"
  logfile="$LOG_DIR/${slug}.log"
  start_ts="$(date +%s)"
  if "$@" >"$logfile" 2>&1; then
    rc=0
  else
    rc=$?
  fi
  end_ts="$(date +%s)"
  duration="$((end_ts - start_ts))s"

  if [[ "$rc" -eq 0 ]]; then
    record_result "PASS" "$label" "$duration" "$logfile"
  elif [[ "$rc" -eq "$SKIP_CODE" ]]; then
    record_result "SKIP" "$label" "$duration" "$logfile"
  else
    record_result "FAIL" "$label" "$duration" "$logfile"
  fi
}

run_cargo_gate() {
  local label="$1"
  shift
  run_gate "$label" env CARGO_BUILD_JOBS="$CARGO_GATE_JOBS" "$@"
}

check_supported_debian_host() {
  [[ "$(uname -s)" == "Linux" ]] || {
    echo "Host OS is not Linux: $(uname -s)"
    [[ "$ALLOW_NON_DEBIAN" == "true" ]] && return "$SKIP_CODE"
    return 1
  }
  [[ -r /etc/os-release ]] || {
    echo "/etc/os-release not found"
    [[ "$ALLOW_NON_DEBIAN" == "true" ]] && return "$SKIP_CODE"
    return 1
  }
  # shellcheck disable=SC1091
  source /etc/os-release
  echo "ID=${ID:-unknown}"
  echo "VERSION_ID=${VERSION_ID:-unknown}"
  if [[ "${ID:-}" == "debian" && ( "${VERSION_ID:-}" == "12" || "${VERSION_ID:-}" == "13" ) ]]; then
    return 0
  fi
  [[ "$ALLOW_NON_DEBIAN" == "true" ]] && return "$SKIP_CODE"
  echo "Expected supported Debian host (12 or 13), got ${ID:-unknown} ${VERSION_ID:-unknown}"
  return 1
}

check_required_tooling() {
  local missing=0
  local tools=(cargo rustc node npm git curl jq psql)
  if [[ "$SKIP_BROWSER_SMOKE" != "true" ]]; then
    tools+=(lsof)
  fi
  if [[ "$ALLOW_NON_DEBIAN" != "true" ]]; then
    tools+=(systemctl)
  fi
  local tool
  for tool in "${tools[@]}"; do
    if command -v "$tool" >/dev/null 2>&1; then
      printf 'found %s at %s\n' "$tool" "$(command -v "$tool")"
    else
      printf 'missing %s\n' "$tool"
      missing=1
    fi
  done
  return "$missing"
}

check_no_docker_runtime_files() {
  local matches
  matches="$(find "$REPO_ROOT" -maxdepth 2 \( -name 'Dockerfile' -o -name 'docker-compose*.yml' -o -name '.dockerignore' \) -print)"
  if [[ -n "$matches" ]]; then
    echo "$matches"
    return 1
  fi
  echo "No Docker runtime files found in live repo paths."
}

check_live_runtime_docs() {
  if rg -n 'docker-compose|docker run|sqlite://' \
      "$REPO_ROOT/README.md" \
      "$REPO_ROOT/AGENTS.md" \
      "$REPO_ROOT/docs/operations/debian-12-native-runtime.md" \
      "$REPO_ROOT/scripts/start-native.sh" \
      "$REPO_ROOT/scripts/stop-native.sh" \
      "$REPO_ROOT/scripts/deploy-native.sh" \
      "$REPO_ROOT/scripts/install_native_debian.sh" \
      "$REPO_ROOT/scripts/install_native_systemd.sh" \
      "$REPO_ROOT/scripts/start.sh" \
      "$REPO_ROOT/scripts/stop.sh" \
      "$REPO_ROOT/scripts/clean_install.sh"
  then
    echo "Found unsupported runtime guidance in live docs/scripts."
    return 1
  fi
  echo "Live docs/scripts reflect native Debian runtime only."
}

check_native_script_syntax() {
  bash -n \
    "$REPO_ROOT/scripts/start-native.sh" \
    "$REPO_ROOT/scripts/stop-native.sh" \
    "$REPO_ROOT/scripts/deploy-native.sh" \
    "$REPO_ROOT/scripts/install_native_debian.sh" \
    "$REPO_ROOT/scripts/ci/debian_native_gates.sh"
}

check_ui_dependencies_present() {
  [[ -x "$REPO_ROOT/ui/node_modules/.bin/tsc" ]] || {
    echo "UI dependencies are missing. Run npm --prefix ui ci first."
    return 1
  }
}

check_media_fixtures() {
  "$REPO_ROOT/tests/check_media_fixtures.sh"
}

runtime_service_present() {
  systemctl cat "${RUSTFIN_SYSTEMD_SERVICE:-rustyfin-native.service}" >/dev/null 2>&1
}

edge_origin() {
  echo "${RUSTYFIN_BROWSER_BACKEND_ORIGIN:-https://127.0.0.1:${RUSTFIN_UI_EDGE_PORT:-3000}}"
}

edge_curl() {
  local path="$1"
  local url
  url="$(edge_origin)"
  url="${url%/}${path}"
  if [[ -n "${RUSTFIN_EDGE_HEALTH_RESOLVE:-}" ]]; then
    curl -skf --resolve "${RUSTFIN_EDGE_HEALTH_RESOLVE}" "$url"
  else
    curl -skf "$url"
  fi
}

check_runtime_services_active() {
  runtime_service_present || {
    echo "rustyfin-native.service not installed on this host."
    return "$SKIP_CODE"
  }
  systemctl is-active --quiet "${RUSTFIN_SYSTEMD_SERVICE:-rustyfin-native.service}"
  systemctl is-active --quiet "${RUSTFIN_SERVERS_AGENT_SERVICE:-rustfin-servers-agent.service}"
  systemctl is-active "${RUSTFIN_SYSTEMD_SERVICE:-rustyfin-native.service}"
  systemctl is-active "${RUSTFIN_SERVERS_AGENT_SERVICE:-rustfin-servers-agent.service}"
}

check_runtime_edge() {
  runtime_service_present || {
    echo "rustyfin-native.service not installed on this host."
    return "$SKIP_CODE"
  }
  edge_curl "/" >/dev/null
}

check_runtime_config() {
  runtime_service_present || {
    echo "rustyfin-native.service not installed on this host."
    return "$SKIP_CODE"
  }
  edge_curl "/runtime-config" | jq -e '
    (.backend_origin | type) == "string" and (.backend_origin | length) > 0
  '
}

check_runtime_health_endpoints() {
  runtime_service_present || {
    echo "rustyfin-native.service not installed on this host."
    return "$SKIP_CODE"
  }

  curl -sf "http://127.0.0.1:${RUSTFIN_BACKEND_PORT:-8096}/health" | jq -e '.status == "ok"'
  curl -sf "http://127.0.0.1:${RUSTFIN_CALENDAR_PORT:-8099}/health" | jq -e '.status == "ok"'
  curl -sf "http://127.0.0.1:${RUSTFIN_TMDB_AGENT_PORT:-8100}/health" | jq -e '.status == "ok"'
  curl -sf "http://127.0.0.1:${RUSTFIN_YOUTUBE_AGENT_PORT:-8101}/health" | jq -e '.status == "ok"'
  curl -sf "http://127.0.0.1:${RUSTFIN_TRANSCRIPTION_AGENT_PORT:-8102}/health" | jq -e '.status == "ok"'
  curl -sf "http://127.0.0.1:${RUSTFIN_SERVERS_AGENT_PORT:-8103}/health" | jq -e '.ok == true'
}

expect_http_code() {
  local method="$1"
  local url="$2"
  local expected="$3"
  local actual
  actual="$(curl -s -o /dev/null -w '%{http_code}' -X "$method" "$url")"
  echo "$method $url -> $actual"
  [[ "$actual" == "$expected" ]]
}

check_runtime_protected_api_auth() {
  runtime_service_present || {
    echo "rustyfin-native.service not installed on this host."
    return "$SKIP_CODE"
  }

  local base="http://127.0.0.1:${RUSTFIN_BACKEND_PORT:-8096}/api/v1"
  expect_http_code GET "${base}/system/info/public" 200
  expect_http_code GET "${base}/users" 401
  expect_http_code GET "${base}/libraries" 401
  expect_http_code GET "${base}/channels" 401
  expect_http_code GET "${base}/watch-party/rooms" 401
  expect_http_code GET "${base}/servers/minecraft/instances" 401
  expect_http_code GET "${base}/jobs" 401
}

check_latest_migration_applied() {
  runtime_service_present || {
    echo "rustyfin-native.service not installed on this host."
    return "$SKIP_CODE"
  }
  command -v psql >/dev/null 2>&1 || {
    echo "psql not installed"
    return 1
  }
  local latest_migration db_name applied_count db_target
  latest_migration="$(find "$REPO_ROOT/crates/db/migrations_pg" -maxdepth 1 -type f -name '*.sql' -print | sort | tail -n 1)"
  [[ -n "$latest_migration" ]] || {
    echo "No migrations found"
    return 1
  }
  latest_migration="$(basename "$latest_migration" .sql)"
  db_target="${RUSTFIN_DATABASE_URL:-}"
  if [[ -n "$db_target" ]]; then
    applied_count="$(psql "$db_target" -At -c "select count(*) from _migrations where name = '$latest_migration';")" || {
      echo "Failed to query runtime database via RUSTFIN_DATABASE_URL"
      return 1
    }
  else
    db_name="${RUSTFIN_PG_DB:-rustfin}"
    applied_count="$("${RUN_POSTGRES[@]}" -At -d "$db_name" -c "select count(*) from _migrations where name = '$latest_migration';")" || {
      echo "Failed to query postgres directly for migration state"
      return 1
    }
  fi
  echo "latest migration: $latest_migration"
  echo "applied count: ${applied_count:-0}"
  [[ "${applied_count:-0}" == "1" ]]
}

check_recent_journal_errors() {
  runtime_service_present || {
    echo "rustyfin-native.service not installed on this host."
    return "$SKIP_CODE"
  }
  local entries
  if entries="$(journalctl \
      -u "${RUSTFIN_SYSTEMD_SERVICE:-rustyfin-native.service}" \
      -u "${RUSTFIN_SERVERS_AGENT_SERVICE:-rustfin-servers-agent.service}" \
      --since '15 minutes ago' \
      -p err \
      --no-pager 2>/dev/null)"
  then
    :
  elif [[ "${#RUN_ROOT[@]}" -gt 0 ]]; then
    entries="$("${RUN_ROOT[@]}" journalctl \
      -u "${RUSTFIN_SYSTEMD_SERVICE:-rustyfin-native.service}" \
      -u "${RUSTFIN_SERVERS_AGENT_SERVICE:-rustfin-servers-agent.service}" \
      --since '15 minutes ago' \
      -p err \
      --no-pager 2>&1)" || {
      printf '%s\n' "$entries"
      echo "Failed to read journal entries"
      return 1
    }
  else
    echo "Failed to read journal entries"
    return 1
  fi
  entries="$(printf '%s' "$entries" | tr -d '\r')"
  if [[ -z "$entries" || "$entries" == "-- No entries --" ]]; then
    echo "No recent error-level journal entries."
    return 0
  fi
  if [[ -n "$entries" ]]; then
    echo "$entries"
    return 1
  fi
}

check_debian_browser_smoke() {
  "${REPO_ROOT}/scripts/ci/debian_browser_smoke.sh"
}

build_schema_db_url() {
  local base_url="$1"
  local schema_name="$2"
  local options_param="options=-c%20search_path%3D${schema_name}"

  if [[ "$base_url" == *\?* ]]; then
    printf '%s&%s' "$base_url" "$options_param"
  else
    printf '%s?%s' "$base_url" "$options_param"
  fi
}

check_setup_integration() {
  local base_db_url="${RUSTFIN_DATABASE_URL:-}"
  [[ -n "$base_db_url" ]] || {
    echo "RUSTFIN_DATABASE_URL is required for setup integration gate"
    return 1
  }

  local schema_name="rustfin_setup_gate_${RUN_ID}_$$"
  local test_db_url
  local rc=0
  test_db_url="$(build_schema_db_url "$base_db_url" "$schema_name")"

  psql "$base_db_url" -v ON_ERROR_STOP=1 -c "CREATE SCHEMA ${schema_name};" >/dev/null

  env \
    CARGO_BUILD_JOBS="$CARGO_GATE_JOBS" \
    RUSTFIN_TEST_DATABASE_URL="$test_db_url" \
    RUSTFIN_TEST_DB_ALLOW_ANY=1 \
    cargo test -p rustfin-server --test integration setup_full_wizard_flow -- --exact || rc=$?

  psql "$base_db_url" -v ON_ERROR_STOP=1 -c "DROP SCHEMA IF EXISTS ${schema_name} CASCADE;" >/dev/null 2>&1 || true
  return "$rc"
}

write_report() {
  local overall host_name current_commit
  overall="PASS"
  [[ "$FAIL_COUNT" -eq 0 ]] || overall="FAIL"
  host_name="$(hostname)"
  current_commit="$(git rev-parse HEAD)"

  {
    echo "# Supported-Debian Native Quality Gates"
    echo
    echo "- Run ID: \`$RUN_ID\`"
    echo "- Host: \`$host_name\`"
    echo "- Commit: \`$current_commit\`"
    echo "- Overall: \`$overall\`"
    echo "- Passed: \`$PASS_COUNT\`"
    echo "- Failed: \`$FAIL_COUNT\`"
    echo "- Skipped: \`$SKIP_COUNT\`"
    echo
    echo "## Results"
    echo
    echo "| Status | Gate | Duration | Log |"
    echo "| --- | --- | --- | --- |"
    local row status label duration logfile
    for row in "${RESULT_ROWS[@]}"; do
      IFS='|' read -r status label duration logfile <<<"$row"
      echo "| $status | $label | $duration | \`$logfile\` |"
    done
  } >"$REPORT_PATH"

  cp "$REPORT_PATH" "$LATEST_REPORT"
}

info "Running supported-Debian native quality gates..."
info "Logs: $LOG_DIR"

run_gate "Host is supported Debian" check_supported_debian_host
run_gate "Required host tooling present" check_required_tooling
run_gate "No Docker runtime files remain" check_no_docker_runtime_files
run_gate "Live docs and scripts reflect native runtime" check_live_runtime_docs
run_gate "Native script syntax" check_native_script_syntax
run_gate "Rust formatting" cargo fmt --all -- --check

if [[ "$SKIP_CLIPPY" == "true" ]]; then
  warn "Skipping clippy gates by request."
else
  run_cargo_gate "Rust clippy critical crates" cargo clippy -p rustfin-server -p rustfin-transcoder -p rustfin-calendar -p rustfin-servers-host -- -D warnings
fi

if [[ "$SKIP_TESTS" == "true" ]]; then
  warn "Skipping Rust test gates by request."
else
  run_cargo_gate "Rust server lib tests" cargo test -p rustfin-server --lib
  run_gate "Rust setup integration" check_setup_integration
  run_cargo_gate "Rust server integration compile" cargo test -p rustfin-server --test integration --no-run
  run_cargo_gate "Rust transcoder tests" cargo test -p rustfin-transcoder --lib
  run_cargo_gate "Rust calendar tests" cargo test -p rustfin-calendar --bin rustfin-calendar
  run_cargo_gate "Rust servers-host tests" cargo test -p rustfin-servers-host
fi

if [[ "$SKIP_UI" == "true" ]]; then
  warn "Skipping UI gates by request."
else
  run_gate "UI dependencies present" check_ui_dependencies_present
  run_gate "Media fixtures are playable" check_media_fixtures
  run_gate "UI lint" npm --prefix ui run lint
  run_gate "UI typecheck" "$REPO_ROOT/ui/node_modules/.bin/tsc" --noEmit -p "$REPO_ROOT/ui/tsconfig.json"
  run_gate "UI production build" npm --prefix ui run build
fi

if [[ "$SKIP_BROWSER_SMOKE" == "true" ]]; then
  warn "Skipping browser smoke gate by request."
else
  run_gate "Browser smoke suite" check_debian_browser_smoke
fi

if [[ "$SKIP_RUNTIME" == "true" ]]; then
  warn "Skipping runtime gates by request."
else
  run_gate "Runtime services active" check_runtime_services_active
  run_gate "Runtime edge reachable" check_runtime_edge
  run_gate "Runtime config endpoint" check_runtime_config
  run_gate "Runtime health endpoints" check_runtime_health_endpoints
  run_gate "Protected API auth gate" check_runtime_protected_api_auth
  run_gate "Latest migration applied" check_latest_migration_applied
  run_gate "No recent runtime journal errors" check_recent_journal_errors
fi

write_report

if [[ "$FAIL_COUNT" -gt 0 ]]; then
  error "Quality gates failed. Report: $REPORT_PATH"
  exit 1
fi

success "All requested quality gates passed. Report: $REPORT_PATH"
