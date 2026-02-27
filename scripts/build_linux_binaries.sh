#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  ./scripts/build_linux_binaries.sh --profile <dev|release|name> --output-dir <dir> [--target <triple>] --bin <name> [--bin <name>...]

Options:
  --profile     Cargo profile (dev, release, or custom profile name).
  --output-dir  Directory where built binaries are copied.
  --target      Linux target triple (default: auto based on host arch).
  --cache-dir   Cargo target cache directory (default: <output-dir>/../.build-cache).
  --bin         Binary name to build (repeatable).
  -h, --help    Show this help.

Notes:
  - On non-Linux hosts this script uses `cargo zigbuild` and requires:
    - `zig`
    - `cargo-zigbuild`
  - On Linux, if target matches rust host triple, plain `cargo build` is used.
EOF
}

PROFILE=""
OUTPUT_DIR=""
TARGET_TRIPLE=""
CACHE_DIR=""
declare -a BINS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile)
      [[ $# -ge 2 ]] || { echo "Missing value for --profile" >&2; exit 1; }
      PROFILE="$2"
      shift 2
      ;;
    --output-dir)
      [[ $# -ge 2 ]] || { echo "Missing value for --output-dir" >&2; exit 1; }
      OUTPUT_DIR="$2"
      shift 2
      ;;
    --target)
      [[ $# -ge 2 ]] || { echo "Missing value for --target" >&2; exit 1; }
      TARGET_TRIPLE="$2"
      shift 2
      ;;
    --cache-dir)
      [[ $# -ge 2 ]] || { echo "Missing value for --cache-dir" >&2; exit 1; }
      CACHE_DIR="$2"
      shift 2
      ;;
    --bin)
      [[ $# -ge 2 ]] || { echo "Missing value for --bin" >&2; exit 1; }
      BINS+=("$2")
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

[[ -n "$PROFILE" ]] || { echo "--profile is required" >&2; exit 1; }
[[ -n "$OUTPUT_DIR" ]] || { echo "--output-dir is required" >&2; exit 1; }
[[ ${#BINS[@]} -gt 0 ]] || { echo "At least one --bin is required" >&2; exit 1; }

host_os="$(uname -s | tr '[:upper:]' '[:lower:]')"
host_arch="$(uname -m)"

if [[ -z "$TARGET_TRIPLE" ]]; then
  case "$host_arch" in
    arm64|aarch64)
      TARGET_TRIPLE="aarch64-unknown-linux-gnu"
      ;;
    x86_64|amd64)
      TARGET_TRIPLE="x86_64-unknown-linux-gnu"
      ;;
    *)
      echo "Unsupported host arch '$host_arch'; set --target explicitly." >&2
      exit 1
      ;;
  esac
fi

if [[ -z "$CACHE_DIR" ]]; then
  CACHE_DIR="$(cd "$(dirname "$OUTPUT_DIR")" && pwd)/.build-cache"
fi

mkdir -p "$OUTPUT_DIR" "$CACHE_DIR"

artifact_profile_dir="$PROFILE"
if [[ "$PROFILE" == "dev" || "$PROFILE" == "debug" ]]; then
  artifact_profile_dir="debug"
fi

rust_host_info="$(rustc -vV)"
rust_host_triple="$(printf '%s\n' "$rust_host_info" | awk '/^host: / {print $2}')"
use_zigbuild=false
if [[ "$host_os" != "linux" || "$rust_host_triple" != "$TARGET_TRIPLE" ]]; then
  use_zigbuild=true
fi

if [[ "$use_zigbuild" == "true" ]]; then
  if ! command -v zig >/dev/null 2>&1; then
    cat >&2 <<EOF
Native Linux cross-build requires 'zig' on ${host_os}/${host_arch}.
Install and retry.
macOS example: brew install zig
EOF
    exit 1
  fi
  if ! cargo zigbuild --version >/dev/null 2>&1; then
    cat >&2 <<'EOF'
Native Linux cross-build requires cargo-zigbuild.
Install and retry:
  cargo install cargo-zigbuild --locked
EOF
    exit 1
  fi
fi

if ! rustup target list --installed | grep -Fxq "$TARGET_TRIPLE"; then
  rustup target add "$TARGET_TRIPLE"
fi

build_one() {
  local bin="$1"
  local -a cmd=()

  if [[ "$use_zigbuild" == "true" ]]; then
    cmd=(cargo zigbuild --target "$TARGET_TRIPLE")
  else
    cmd=(cargo build --target "$TARGET_TRIPLE")
  fi

  if [[ "$PROFILE" == "release" ]]; then
    cmd+=(--release)
  elif [[ "$PROFILE" != "dev" && "$PROFILE" != "debug" ]]; then
    cmd+=(--profile "$PROFILE")
  fi
  cmd+=(--bin "$bin")

  echo "[native-build] building ${bin} (${TARGET_TRIPLE}, profile=${PROFILE})"
  CARGO_TARGET_DIR="$CACHE_DIR" "${cmd[@]}"

  local artifact="${CACHE_DIR}/${TARGET_TRIPLE}/${artifact_profile_dir}/${bin}"
  if [[ ! -f "$artifact" ]]; then
    echo "Expected artifact missing: $artifact" >&2
    exit 1
  fi
  cp "$artifact" "${OUTPUT_DIR}/${bin}"
  chmod 755 "${OUTPUT_DIR}/${bin}"
}

for bin in "${BINS[@]}"; do
  build_one "$bin"
done

echo "[native-build] output dir: ${OUTPUT_DIR}"
echo "[native-build] target: ${TARGET_TRIPLE}"
