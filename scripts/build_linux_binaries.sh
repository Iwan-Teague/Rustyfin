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
  - This script uses `cargo zigbuild` when cross-compiling or when GNU libc
    compatibility mode is enabled (default for Linux GNU targets).
  - `cargo zigbuild` requires:
    - `zig`
    - `cargo-zigbuild`
  - To force host-glibc builds on Linux GNU targets, set:
      RUSTFIN_NATIVE_GNU_COMPAT_BUILD=0
EOF
}

PROFILE=""
OUTPUT_DIR=""
TARGET_TRIPLE=""
CACHE_DIR=""
declare -a BINS=()
RUST_TOOLCHAIN="${RUSTFIN_NATIVE_RUST_TOOLCHAIN:-stable}"
RUSTFIN_NATIVE_GNU_COMPAT_BUILD="${RUSTFIN_NATIVE_GNU_COMPAT_BUILD:-1}"
RUSTFIN_NATIVE_GNU_GLIBC_VERSION="${RUSTFIN_NATIVE_GNU_GLIBC_VERSION:-2.36}"
RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES="${RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES:-}"
RUSTFIN_SERVER_CARGO_FEATURES="${RUSTFIN_SERVER_CARGO_FEATURES:-}"

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

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo is required for native Linux binary build." >&2
  exit 1
fi
if ! command -v rustc >/dev/null 2>&1; then
  echo "rustc is required for native Linux binary build." >&2
  exit 1
fi

declare -a RUSTC_CMD=("rustc")
declare -a CARGO_CMD=("cargo")
RUSTC_BIN=""
RUSTDOC_BIN=""
if command -v rustup >/dev/null 2>&1; then
  if rustup run "$RUST_TOOLCHAIN" rustc -vV >/dev/null 2>&1 && rustup run "$RUST_TOOLCHAIN" cargo -V >/dev/null 2>&1; then
    RUSTC_BIN="$(rustup which --toolchain "$RUST_TOOLCHAIN" rustc)"
    RUSTDOC_BIN="$(rustup which --toolchain "$RUST_TOOLCHAIN" rustdoc)"
    CARGO_BIN="$(rustup which --toolchain "$RUST_TOOLCHAIN" cargo)"
    if [[ -x "$RUSTC_BIN" && -x "$RUSTDOC_BIN" && -x "$CARGO_BIN" ]]; then
      RUSTC_CMD=("$RUSTC_BIN")
      CARGO_CMD=("$CARGO_BIN")
    else
      RUSTC_CMD=("rustup" "run" "$RUST_TOOLCHAIN" "rustc")
      CARGO_CMD=("rustup" "run" "$RUST_TOOLCHAIN" "cargo")
    fi
  fi
fi

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
OUTPUT_DIR="$(cd "$OUTPUT_DIR" && pwd -P)"
CACHE_DIR="$(cd "$CACHE_DIR" && pwd -P)"

artifact_profile_dir="$PROFILE"
if [[ "$PROFILE" == "dev" || "$PROFILE" == "debug" ]]; then
  artifact_profile_dir="debug"
fi

rust_host_info="$("${RUSTC_CMD[@]}" -vV)"
rust_host_triple="$(printf '%s\n' "$rust_host_info" | awk '/^host: / {print $2}')"
use_zigbuild=false
target_is_gnu_linux=false
if [[ "$TARGET_TRIPLE" == *-unknown-linux-gnu* ]]; then
  target_is_gnu_linux=true
fi

force_gnu_compat_zig=false
if [[ "$host_os" == "linux" && "$target_is_gnu_linux" == "true" && "$RUSTFIN_NATIVE_GNU_COMPAT_BUILD" == "1" ]]; then
  force_gnu_compat_zig=true
fi

if [[ "$host_os" != "linux" || "$rust_host_triple" != "$TARGET_TRIPLE" || "$force_gnu_compat_zig" == "true" ]]; then
  use_zigbuild=true
fi

zig_target="$TARGET_TRIPLE"
if [[ "$use_zigbuild" == "true" && "$target_is_gnu_linux" == "true" && "$RUSTFIN_NATIVE_GNU_COMPAT_BUILD" == "1" ]]; then
  # Produce binaries linked against a Debian-12-compatible glibc baseline.
  if [[ "$TARGET_TRIPLE" != *.* ]]; then
    zig_target="${TARGET_TRIPLE}.${RUSTFIN_NATIVE_GNU_GLIBC_VERSION}"
  fi
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
  if ! command -v cargo-zigbuild >/dev/null 2>&1; then
    cat >&2 <<'EOF'
Native Linux cross-build requires cargo-zigbuild.
Install and retry:
  cargo install cargo-zigbuild --locked
EOF
    exit 1
  fi
fi

if command -v rustup >/dev/null 2>&1; then
  if ! rustup target list --toolchain "$RUST_TOOLCHAIN" --installed | grep -Fxq "$TARGET_TRIPLE"; then
    rustup target add --toolchain "$RUST_TOOLCHAIN" "$TARGET_TRIPLE"
  fi
fi

if [[ "$use_zigbuild" == "true" ]]; then
  export ZIG_LOCAL_CACHE_DIR="${CACHE_DIR}/zig-local"
  export ZIG_GLOBAL_CACHE_DIR="${CACHE_DIR}/zig-global"
  mkdir -p "$ZIG_LOCAL_CACHE_DIR" "$ZIG_GLOBAL_CACHE_DIR"
fi

build_one() {
  local bin="$1"
  local -a cmd=()

  if [[ "$use_zigbuild" == "true" ]]; then
    cmd=("${CARGO_CMD[@]}" zigbuild --target "$zig_target")
  else
    cmd=("${CARGO_CMD[@]}" build --target "$TARGET_TRIPLE")
  fi

  cmd+=(--locked)
  if [[ "$PROFILE" == "release" ]]; then
    cmd+=(--release)
  elif [[ "$PROFILE" != "dev" && "$PROFILE" != "debug" ]]; then
    cmd+=(--profile "$PROFILE")
  fi
  cmd+=(--bin "$bin")
  if [[ "$bin" == "rustfin-server" && -n "$RUSTFIN_SERVER_CARGO_FEATURES" ]]; then
    cmd+=(--features "$RUSTFIN_SERVER_CARGO_FEATURES")
  fi
  if [[ "$bin" == "rustfin-transcription-agent" && -n "$RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES" ]]; then
    cmd+=(--features "$RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES")
  fi

  echo "[native-build] building ${bin} (${TARGET_TRIPLE}, profile=${PROFILE})"
  if [[ "$use_zigbuild" == "true" ]]; then
    echo "[native-build]   zig target: ${zig_target}"
  fi
  if [[ -n "$RUSTC_BIN" && -n "$RUSTDOC_BIN" ]]; then
    CARGO_TARGET_DIR="$CACHE_DIR" \
    RUSTC="$RUSTC_BIN" \
    RUSTDOC="$RUSTDOC_BIN" \
    "${cmd[@]}"
  else
    CARGO_TARGET_DIR="$CACHE_DIR" "${cmd[@]}"
  fi

  local artifact_target_dir="$TARGET_TRIPLE"
  if [[ "$use_zigbuild" == "true" ]]; then
    artifact_target_dir="$zig_target"
  fi
  local artifact="${CACHE_DIR}/${artifact_target_dir}/${artifact_profile_dir}/${bin}"
  if [[ ! -f "$artifact" && "$use_zigbuild" == "true" && "$artifact_target_dir" != "$TARGET_TRIPLE" ]]; then
    # cargo-zigbuild may still emit into the canonical Rust target dir.
    local fallback_artifact="${CACHE_DIR}/${TARGET_TRIPLE}/${artifact_profile_dir}/${bin}"
    if [[ -f "$fallback_artifact" ]]; then
      artifact="$fallback_artifact"
    fi
  fi
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
