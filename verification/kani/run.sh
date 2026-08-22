#!/usr/bin/env bash
set -euo pipefail

if ! cargo kani --version >/dev/null 2>&1; then
  echo "cargo-kani is required; see verification/kani/README.md" >&2
  exit 127
fi

exec cargo kani --features binary,backend-cf-compat "$@"
