\
    #!/usr/bin/env bash
    set -euo pipefail
    assert_file() { [ -f "$1" ] || { echo "[assert] expected file: $1" >&2; exit 1; }; }

