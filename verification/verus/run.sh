#!/bin/sh
set -eu

pin='0.2026.08.15.7d4628a'
default_verus="/private/tmp/verus-plist-rs-${pin}/verus-arm64-macos/verus"
verus_bin="${VERUS_BIN:-${default_verus}}"
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

if [ ! -x "$verus_bin" ]; then
    echo "Verus ${pin} not found at ${verus_bin}" >&2
    echo 'Set VERUS_BIN to the pinned verifier executable.' >&2
    exit 1
fi

if ! "$verus_bin" --version | grep -F "Version: ${pin}" >/dev/null; then
    echo "VERUS_BIN is not the pinned Verus ${pin} release" >&2
    exit 1
fi

output_dir=$(mktemp -d "${TMPDIR:-/tmp}/plist-rs-verus.XXXXXX")
trap 'rm -rf "$output_dir"' EXIT HUP INT TERM

"$script_dir/deny-trusted-shortcuts.sh"

for source in "$script_dir"/*.rs; do
    source_name=$(basename "$source" .rs)
    executable="$output_dir/$source_name"
    "$verus_bin" \
        --no-cheating \
        --compile \
        --triggers-mode silent \
        --multiple-errors 20 \
        -o "$executable" \
        "$source"
    "$executable"
done
