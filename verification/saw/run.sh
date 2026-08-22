#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root_dir="$(cd "$script_dir/../.." && pwd)"
lock_file="$script_dir/saw.lock"
reference_lock="$root_dir/verification/reference.lock"

lock_value() {
    sed -n "s/^$1=//p" "$lock_file"
}

reference_value() {
    sed -n "s/^$1=//p" "$reference_lock"
}

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

require_hash() {
    local path="$1"
    local expected="$2"
    local actual
    actual="$(sha256_file "$path")"
    if [[ "$actual" != "$expected" ]]; then
        printf 'SHA-256 mismatch for %s\nexpected: %s\nactual:   %s\n' \
            "$path" "$expected" "$actual" >&2
        exit 1
    fi
}

resolve_executable() {
    local candidate="$1"
    if [[ "$candidate" == */* ]]; then
        [[ -x "$candidate" ]] || return 1
        printf '%s\n' "$candidate"
    else
        command -v "$candidate"
    fi
}

saw_bin="$(resolve_executable "${SAW_BIN:-saw}")" || {
    echo 'SAW_BIN must name the pinned SAW executable.' >&2
    exit 1
}
clang_bin="$(resolve_executable "${CLANG:-clang}")" || {
    echo 'CLANG must name the pinned Clang executable.' >&2
    exit 1
}
llvm_link_bin="$(resolve_executable "${LLVM_LINK:-llvm-link}")" || {
    echo 'LLVM_LINK must name the pinned llvm-link executable.' >&2
    exit 1
}

if [[ -n "${RUSTC:-}" ]]; then
    rustc_bin="$(resolve_executable "$RUSTC")" || {
        echo 'RUSTC must name an executable.' >&2
        exit 1
    }
    rustc_cmd=("$rustc_bin")
else
    rustc_bin="$(resolve_executable rustc)" || {
        echo 'rustc is required.' >&2
        exit 1
    }
    rustc_cmd=("$rustc_bin" "+$(lock_value rust_toolchain)")
fi

# The official with-solvers archive places Z3 beside SAW.
PATH="$(cd "$(dirname "$saw_bin")" && pwd):$PATH"
export PATH

saw_version="$("$saw_bin" --version 2>&1)"
clang_version="$("$clang_bin" --version 2>&1)"
llvm_link_version="$("$llvm_link_bin" --version 2>&1)"
rust_version="$("${rustc_cmd[@]}" -vV)"

grep -F "$(lock_value saw_version) ($(lock_value saw_version_commit)" <<<"$saw_version" >/dev/null || {
    printf 'SAW version does not match saw.lock:\n%s\n' "$saw_version" >&2
    exit 1
}
grep -F "clang version $(lock_value clang_version)" <<<"$clang_version" >/dev/null || {
    printf 'Clang version does not match saw.lock:\n%s\n' "$clang_version" >&2
    exit 1
}
grep -F "LLVM version $(lock_value llvm_link_version)" <<<"$llvm_link_version" >/dev/null || {
    printf 'llvm-link version does not match saw.lock:\n%s\n' "$llvm_link_version" >&2
    exit 1
}
grep -F "rustc $(lock_value rust_toolchain) " <<<"$rust_version" >/dev/null || {
    printf 'Rust version does not match saw.lock:\n%s\n' "$rust_version" >&2
    exit 1
}
grep -F "LLVM version: $(lock_value rust_llvm_version)" <<<"$rust_version" >/dev/null || {
    printf 'Rust LLVM version does not match saw.lock:\n%s\n' "$rust_version" >&2
    exit 1
}

z3_bin="$(resolve_executable z3)" || {
    echo 'The pinned Z3 solver must be on PATH (the SAW with-solvers archive includes it).' >&2
    exit 1
}
grep -F "Z3 version $(lock_value z3_version)" <<<"$("$z3_bin" --version)" >/dev/null || {
    printf 'Z3 version does not match saw.lock: %s\n' "$("$z3_bin" --version)" >&2
    exit 1
}

if [[ -n "${SAW_ARCHIVE:-}" ]]; then
    archive_hash="$(sha256_file "$SAW_ARCHIVE")"
    if [[ "$archive_hash" != "$(lock_value macos_arm64_sha256)" && \
          "$archive_hash" != "$(lock_value linux_x86_64_sha256)" ]]; then
        printf 'SAW_ARCHIVE hash is not one of the pinned official artifacts: %s\n' \
            "$archive_hash" >&2
        exit 1
    fi
fi

[[ "$(reference_value commit)" == "$(lock_value apple_commit)" ]]
[[ "$(reference_value binary_sha256)" == "$(lock_value apple_binary_sha256)" ]]
[[ "$(reference_value target_pointer_width)" == "$(lock_value target_pointer_width)" ]]

build_dir="$(mktemp -d "${TMPDIR:-/tmp}/plist-rs-saw.XXXXXX")"
trap 'rm -rf "$build_dir"' EXIT HUP INT TERM

bash "$script_dir/deny-trusted-shortcuts.sh"

apple_source="$build_dir/CFBinaryPList.c"
if [[ -n "${APPLE_CF_SOURCE:-}" ]]; then
    cp "$APPLE_CF_SOURCE" "$apple_source"
else
    apple_url="https://raw.githubusercontent.com/opensource-apple/CF/$(lock_value apple_commit)/CFBinaryPList.c"
    curl --fail --location --silent --show-error "$apple_url" --output "$apple_source"
fi
require_hash "$apple_source" "$(lock_value apple_binary_sha256)"

start_count="$(grep -c '^CF_INLINE uint64_t _getSizedInt(const uint8_t \*data, uint8_t valSize) {$' "$apple_source")"
[[ "$start_count" == 1 ]] || {
    printf 'Expected exactly one pinned _getSizedInt definition, found %s.\n' "$start_count" >&2
    exit 1
}
sed -n '/^CF_INLINE uint64_t _getSizedInt(const uint8_t \*data, uint8_t valSize) {$/,/^}$/p' \
    "$apple_source" >"$build_dir/get_sized_int.extracted.inc"
require_hash "$build_dir/get_sized_int.extracted.inc" \
    "$(lock_value apple_get_sized_int_sha256)"

production_source="$root_dir/src/backend/cf_compat/binary_kernels.rs"
require_hash "$production_source" "$(lock_value production_kernels_sha256)"

cp "$script_dir/SizedInt.cry" "$build_dir/SizedInt.cry"
cp "$script_dir/all_widths.saw" "$build_dir/all_widths.saw"
cp "$script_dir/all_widths_spec.saw" "$build_dir/all_widths_spec.saw"
cp "$script_dir/all_widths_x86.saw" "$build_dir/all_widths_x86.saw"
cp "$script_dir/byte_join.saw" "$build_dir/byte_join.saw"
cp "$script_dir/read_int_projection.saw" "$build_dir/read_int_projection.saw"
cp "$script_dir/read_int_projection_x86.saw" "$build_dir/read_int_projection_x86.saw"

# These are the complete compiler/link commands in the trusted build boundary.
# Undefining both x86 macros forces Apple's source-exact generic loop, including
# every non-power-of-two width, instead of its architecture-specific fast path.
set -x
"$clang_bin" \
    -std=c11 -O"$(lock_value c_optimization)" \
    -U__i386__ -U__x86_64__ \
    -I"$build_dir" \
    -emit-llvm -c "$script_dir/apple_harness.c" \
    -o "$build_dir/apple_harness_generic.bc"
"$clang_bin" \
    -std=c11 -O"$(lock_value c_optimization)" \
    -emit-llvm -c "$script_dir/relation_harness.c" \
    -o "$build_dir/relation_harness.bc"
"${rustc_cmd[@]}" \
    --crate-name saw_cf_port --crate-type=lib \
    -C opt-level="$(lock_value rust_optimization)" -C panic=abort \
    --emit=llvm-bc "$script_dir/cf_port.rs" \
    -o "$build_dir/cf_port.bc"
"${rustc_cmd[@]}" \
    --crate-name saw_production_kernel --crate-type=lib \
    -C opt-level="$(lock_value rust_optimization)" -C panic=abort \
    --emit=llvm-bc "$script_dir/production_harness.rs" \
    -o "$build_dir/production_harness.bc"
"$llvm_link_bin" \
    "$build_dir/apple_harness_generic.bc" \
    "$build_dir/cf_port.bc" \
    "$build_dir/production_harness.bc" \
    "$build_dir/relation_harness.bc" \
    -o "$build_dir/sized_int_equivalence_generic.bc"
set +x

(
    cd "$build_dir"
    "$saw_bin" byte_join.saw
    "$saw_bin" all_widths_spec.saw
    "$saw_bin" all_widths.saw
    "$saw_bin" read_int_projection.saw
)

rust_host="$(sed -n 's/^host: //p' <<<"$rust_version")"
if [[ "$rust_host" == x86_64-* ]]; then
    # This second build leaves Clang's native x86 macros intact, selecting the
    # exact 1/2/4/8-byte CFSwap fast branches from the extracted Apple source.
    # C -O1 avoids an LLVM -O2 dead poison phi that SAW 1.5.1 evaluates eagerly.
    set -x
    "$clang_bin" \
        -std=c11 -O"$(lock_value c_optimization)" \
        -I"$build_dir" \
        -emit-llvm -c "$script_dir/apple_harness.c" \
        -o "$build_dir/apple_harness_x86.bc"
    "$llvm_link_bin" \
        "$build_dir/apple_harness_x86.bc" \
        "$build_dir/cf_port.bc" \
        "$build_dir/production_harness.bc" \
        "$build_dir/relation_harness.bc" \
        -o "$build_dir/sized_int_equivalence_x86.bc"
    set +x
    (
        cd "$build_dir"
        "$saw_bin" all_widths_x86.saw
        "$saw_bin" read_int_projection_x86.saw
    )
else
    printf 'Skipping native x86 fast-path theorem on Rust host %s.\n' "$rust_host"
fi
