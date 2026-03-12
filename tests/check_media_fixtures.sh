#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIXTURE_ROOT="${REPO_ROOT}/tests/fixtures/media"
FFPROBE_BIN="${RUSTFIN_FFPROBE_PATH:-ffprobe}"

if [[ ! -d "${FIXTURE_ROOT}" ]]; then
  echo "Fixture root not found: ${FIXTURE_ROOT}" >&2
  exit 1
fi

if ! command -v "${FFPROBE_BIN}" >/dev/null 2>&1; then
  echo "ffprobe is required to validate media fixtures: ${FFPROBE_BIN}" >&2
  exit 1
fi

FIXTURE_FILES=()
while IFS= read -r -d '' fixture; do
  FIXTURE_FILES+=("${fixture}")
done < <(find "${FIXTURE_ROOT}" -type f \( -iname '*.mp4' -o -iname '*.m4v' -o -iname '*.mkv' -o -iname '*.webm' \) -print0 | sort -z)

if [[ "${#FIXTURE_FILES[@]}" -eq 0 ]]; then
  echo "No media fixtures found under ${FIXTURE_ROOT}" >&2
  exit 1
fi

for fixture in "${FIXTURE_FILES[@]}"; do
  echo "checking ${fixture}"
  probe_output="$("${FFPROBE_BIN}" \
    -hide_banner \
    -v error \
    -show_entries stream=codec_type,codec_name,width,height \
    -show_entries format=format_name,duration,size \
    -of default=noprint_wrappers=1 \
    "${fixture}")" || {
      echo "ffprobe failed for ${fixture}" >&2
      exit 1
    }

  if ! grep -q '^codec_type=video$' <<<"${probe_output}"; then
    echo "fixture is missing a video stream: ${fixture}" >&2
    exit 1
  fi

  if ! grep -q '^format_name=' <<<"${probe_output}"; then
    echo "fixture is missing container metadata: ${fixture}" >&2
    exit 1
  fi
done

echo "validated ${#FIXTURE_FILES[@]} media fixture(s)"
