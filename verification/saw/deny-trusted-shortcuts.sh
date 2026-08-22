#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

forbidden_saw='admit|assume_unsat|assume_valid|unsafe_assume|llvm_unint|enable_lax|disable_alloc_sym_init_check'
if grep -En "$forbidden_saw" "$script_dir"/*.saw "$script_dir"/*.cry; then
    echo 'Forbidden SAW assumption, uninterpreted function, or lax-memory switch found.' >&2
    exit 1
fi

if grep -En 'llvm_verify .* false ' "$script_dir"/*.saw; then
    echo 'Every llvm_verify call must enable the path-satisfiability/vacuity check.' >&2
    exit 1
fi

if grep -En 'llvm_verify .* \[[^]]+\] ' "$script_dir"/*.saw; then
    echo 'Proof overrides are forbidden in the cross-language equivalence scripts.' >&2
    exit 1
fi

forbidden_code='__CPROVER_assume|kani::assume|unreachable_unchecked|core::intrinsics::assume'
if grep -En "$forbidden_code" \
    "$script_dir"/cf_port.rs \
    "$script_dir"/production_harness.rs \
    "$script_dir"/apple_harness.c \
    "$script_dir"/relation_harness.c; then
    echo 'Forbidden code-level assumption found.' >&2
    exit 1
fi

echo 'No trusted shortcut, override, or disabled vacuity check found.'
