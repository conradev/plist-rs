# SAW cross-language sized-integer equivalence

This directory contains actual relational proofs connecting the pinned Apple C
source, a source-faithful Rust port, and the allocation-free Rust kernel used by
the production parser. It uses SAW's LLVM/Crucible frontend and Cryptol rather
than treating a manually written specification as proof of C correspondence.

## Proved statements

`all_widths.saw` allocates one 255-byte symbolic buffer and one symbolic `u8`
width for each theorem. Both implementations in a comparison receive the same
pointer and width. SAW proves, without sampling:

- pinned C `_getSizedInt` equals the APSL Rust port;
- the APSL Rust port equals the production Rust kernel; and
- pinned C `_getSizedInt` equals the production Rust kernel;
- each equality holds for every width from 0 through 255 and every possible
  byte value, including unsigned 64-bit wraparound above eight bytes.

`byte_join.saw` independently proves that all three width-eight functions equal
Cryptol `be64`, whose definition is `join` over eight symbolic bytes. Thus this
is both a direct relational proof and an independent byte-order/value proof.

`all_widths_spec.saw` goes further: it proves each of the three implementations
against Cryptol `beSized`, an independent 255-step recurrence over a symbolic
buffer and symbolic `u8` width. The pairwise miters are therefore backed by a
common mathematical result over their complete input domain.

`read_int_projection.saw` checks the defined successful-path value/cursor
composition that exposed a real compatibility bug during this work. For every
low marker nibble and every one of the 128 bytes that can influence the value,
it proves a C projection adapter, the source-faithful Rust composition, and the
actual production value kernel equal the independent Cryptol record and one
another. Declared widths are 1 through 32768; widths of 256 and above convert
to zero through `_getSizedInt`'s `uint8_t` parameter while the cursor still
advances over the complete declared width. The production decoder regression
uses a nonzero 256th byte to lock this behavior down.

This projection is intentionally not described as compiled equivalence of the
complete C `_readInt` function. The C adapter transcribes its successful-path
width conversion, value call, and cursor arithmetic, but omits its marker,
pointer-overflow, and extent checks. Those adapter statements are part of the
trusted boundary. `read_int_projection_x86.saw` adds the three direct
relations through Apple's aligned typed-load path.

The C compile command explicitly undefines `__i386__` and `__x86_64__`. The
theorem therefore covers the source's generic loop—the path used on non-x86
targets and the compatibility path for every non-power-of-two width. This
generic theorem uses an allocation without strengthened alignment.

On a native x86_64 host, the runner also compiles the extracted function
without undefining the architecture macros and proves all three pairwise
relations through Apple's exact 1/2/4/8-byte CFSwap fast branches. That theorem
uses an explicitly eight-byte-aligned symbolic allocation, matching the LLVM
typed-load precondition. It does not claim the C source has defined behavior
for unaligned typed loads, nor does it verify the complete plist parser.

## Exact-source boundary

The runner never trusts a hand-copied C body. It downloads the file at Apple
commit `3cc41a76b1491f50813e28a4ec09954ffa359e6f`, checks the full-file hash,
extracts `_getSizedInt` byte-for-byte into a temporary include, and checks the
extracted-function hash before compiling it. Both hashes are pinned in
`saw.lock` and agree with `../reference.lock`.

`production_harness.rs` uses `#[path]` to compile
`src/backend/cf_compat/binary_kernels.rs` directly. The complete production
file hash is pinned; there is no proof-only copy of its implementation.

`cf_port.rs` is a source-faithful APSL modification and remains separately
licensed. `apple_harness.c`, `relation_harness.c`, and the ABI wrappers are
small proof adapters whose behavior is itself included in symbolic execution.

## Reproduce on arm64 macOS

The recorded toolchain is SAW 1.5.1 with bundled Z3 4.8.14, Rust 1.74.0
(LLVM 17.0.4), and Clang/llvm-link 20.1.8. Official SAW archive hashes are in
`saw.lock`.

Download and verify the official `with-solvers` archive, then use LLVM 20 from
the `nixpkgs_commit` recorded in `saw.lock` (or supply equivalent pinned
binaries):

```sh
SAW_ROOT=/private/tmp/saw-1.5.1-macos-15-ARM64-with-solvers
NIXPKGS_REV=6b316287bae2ee04c9b93c8c858d930fd07d7338
CLANG_ROOT="$(nix --option substituters https://cache.nixos.org \
  build --no-link --print-out-paths \
  "github:NixOS/nixpkgs/$NIXPKGS_REV#llvmPackages_20.clang")"
LLVM_ROOT="$(nix --option substituters https://cache.nixos.org \
  build --no-link --print-out-paths \
  "github:NixOS/nixpkgs/$NIXPKGS_REV#llvmPackages_20.llvm")"

SAW_BIN="$SAW_ROOT/bin/saw" \
CLANG="$CLANG_ROOT/bin/clang" \
LLVM_LINK="$LLVM_ROOT/bin/llvm-link" \
verification/saw/run.sh
```

Set `SAW_ARCHIVE` as well to make the runner verify the downloaded archive.
Set `APPLE_CF_SOURCE` to an existing pinned source file for an offline run; its
hash is still mandatory. `RUSTC` may point to a wrapper for the pinned Rust
compiler, otherwise the runner invokes `rustc +1.74.0`.

The generic run ends with fifteen `Proof succeeded!` lines: three width-eight
Cryptol proofs, three full-width Cryptol proofs, three direct sized-integer
relations, three successful-count projection refinements, and three direct
projection relations. A native x86_64 run adds six aligned fast-path relational
proofs.

## Trusted boundary

The proof trusts the pinned SAW/Crucible, Z3, Rust/LLVM, Clang, and llvm-link
tools; the checked ABI adapters; SHA-256; and the 64-bit target model. It proves
compiled LLVM behavior for the checked source and flags. It does not prove
compiler correctness, allocator failure behavior, undefined C behavior outside
the explicit valid 255-byte allocation, or any parser logic beyond this sized
integer kernel.
