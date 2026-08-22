# Verification

This directory pins the compatibility references and carries three independent
machine-checked layers:

1. SAW/Crucible relational proofs over compiled pinned C, an APSL-2.0 Rust
   port, and the exact production Rust sized-integer kernel;
2. an APSL-2.0 executable Verus port of source-traced CoreFoundation reader
   components; and
3. Kani harnesses over selected private kernels in the production MIT parser.

The distinction matters. SAW provides a literal C-to-Rust theorem, but only for
the published sized-integer domain. The broader Verus files are direct
source-corresponding components but are not linked into the production crate.
Kani checks actual production functions but only within each harness domain.
None is presented as a whole-program parser theorem.

## Reference lock

| Item | Value |
| --- | --- |
| Primary repository | `opensource-apple/CF` |
| Primary commit | `3cc41a76b1491f50813e28a4ec09954ffa359e6f` |
| Target model | LP64; allocation succeeds; immutable/unfiltered reader path |
| `CFBinaryPList.c` SHA-256 | `8b8c72427ba60e7918f3ffa088d9f25859456eb75a0081ff5cdb7277ff62c384` |
| `CFPropertyList.c` SHA-256 | `ced92800b4651fc47e9a1f53626170c7c7324db50bf6740541fd56844b7bead3` |
| `CFDate.c` SHA-256 | `9e89926bb5569862295b02ec5b8fa0ca99c89259ca05c24ffaa3e4727d36b6fb` |
| `ForFoundationOnly.h` SHA-256 | `937efef93f9f15807659816c3a5a50ce4a27631723086748cda3ae778c7b3219` |
| `APPLE_LICENSE` SHA-256 | `212846e0145aa50fb3a5aef254a370311a93acf6c1e792e47e0068d64c8c3885` |
| Hardening descendant | `swiftlang/swift-corelibs-foundation@761b621da93a856a48995efc29ed11028c283306` |
| Descendant `CFBinaryPList.c` SHA-256 | `58741c5361b3eb7aec25b8700887ec8259bc64f31a489914ec3c44ab5d077415` |
| Descendant `CFPropertyList.c` SHA-256 | `23dbbff0c24488a65b7e61ce59f65d4c55b9e6a787c4be6300dfe28a3896de04` |

Pinned primary sources:

- <https://github.com/opensource-apple/CF/blob/3cc41a76b1491f50813e28a4ec09954ffa359e6f/CFBinaryPList.c>
- <https://github.com/opensource-apple/CF/blob/3cc41a76b1491f50813e28a4ec09954ffa359e6f/CFPropertyList.c>
- <https://github.com/opensource-apple/CF/blob/3cc41a76b1491f50813e28a4ec09954ffa359e6f/CFDate.c>
- <https://github.com/opensource-apple/CF/blob/3cc41a76b1491f50813e28a4ec09954ffa359e6f/ForFoundationOnly.h>

`verify-references.sh` downloads every immutable revision and checks the
recorded hash before the reference-backed formal CI jobs run.

## Compiled C-to-Rust byte equivalence

`saw/` uses pinned SAW 1.5.1, Crucible, Cryptol, Z3, Rust 1.74, and LLVM/Clang
20. It checks the complete pinned `CFBinaryPList.c` hash, extracts
`_getSizedInt` byte-for-byte and checks the fragment hash, then compiles:

- the extracted C function;
- a source-faithful Rust translation; and
- `src/backend/cf_compat/binary_kernels.rs`, the exact file called by the
  production parser.

One symbolic 255-byte allocation and one symbolic `u8` width cover every width
0 through 255 and every possible byte value. Nine vacuity-checked proofs with
no overrides establish three independent refinements to Cryptol (`be64` and
the full-domain `beSized`) and all three direct pairwise relations: C-to-port,
port-to-production, and C-to-production. Six more proofs cover the
successful-path `_readInt` value/cursor projection across all 16 low nibbles
and all 128 value-influencing bytes, including the historical width-to-`u8`
conversion. That projection's C composition adapter is trusted; it is not a
literal proof of `_readInt`'s pointer and bounds checks. The generic C theorem
requires no strengthened alignment. Native x86 CI adds six direct theorems for
Apple's typed-load fast path under its explicit eight-byte-alignment
precondition.

[`saw/README.md`](saw/README.md) records the exact compiler flags, hashes,
trusted boundary, reproduction command, and undefined-behavior exclusion. The
source-faithful port and license are excluded from the MIT crate package.

## Executable Verus port

`verus/` contains five executable Rust/Verus files with independent
mathematical specifications, contracts, invariants, and termination measures.
The pinned Verus `0.2026.08.15.7d4628a` run proves, compiles, and executes all
five with `--no-cheating`:

```text
212 verified, 0 errors
```

Coverage includes binary header/trailer/offset scanning, all wire object
branches, abstract graph identity/cycles/equality/collection normalization,
XML lexical/scalar quirks, and abstract XML structure/duplicate-key/CF$UID
transitions. [`verus/PORT_MAP.md`](verus/PORT_MAP.md) gives the function-level
C correspondence, defined-execution model, totalized C undefined-behavior
edges, and bridge exclusions.

The direct port is a derivative representation of APSL-covered source. Its
full license, original notices, and dated modification notice live in
`verus/`. The entire directory is excluded from the MIT crates.io package.

## Production Kani checks

Kani 0.67.0 model-checks six harnesses next to the private production kernels
in `src/backend/cf_compat/binary.rs`:

| Harness | Domain | Property |
| --- | --- | --- |
| `wide_be_u64_is_the_low_half_of_the_exact_value` | every 0-16 byte string | production fold is the low 64 bits of the exact big-endian integer |
| `format_00_integer_signedness_matches_the_specification` | every `u64` payload at widths 1, 2, 4, 8, and 16 | format-`00` integer class matches the scalar specification |
| `object_range_check_matches_exact_arithmetic` | every 64-bit start, length, and limit | range classification matches `u128` arithmetic |
| `bounded_table_layout_matches_exact_arithmetic` | default parser table/object limits and every `u8` width | checked layout matches exact arithmetic without overflow |
| `address_width_check_matches_exact_capacity` | every inclusive maximum and encoded width | capacity test matches the exact power-of-two bound |
| `inline_count_marker_matches_the_format_nibble` | all 256 marker bytes | inline/extended marker selection is exact |

`kani/run.sh` reproduces the run. These proofs complement rather than imply
the larger Verus component coverage.

## Observable contract

The intended complete-reader comparison result is either a stable rejection
class or an accepted normalized graph preserving numeric bits/classes, date
bits, strings/data, UID/object identity, collection kind/order, dictionary
keys, and shared references. Allocator addresses, retain counts, mutability,
logs, and localized error text are outside the result relation.

No current proof composes every byte-input step into that whole graph theorem.
In particular, bridge obligations remain between binary wire descriptors and
abstract graph equality classes, between XML bytes/tokens/recursive values,
and across CoreFoundation allocation, encoding, decimal-floating conversion,
and runtime equality. The installed Darwin implementation is proprietary and
is covered only by differential tests.

## Claim boundary

The machine-checked claim is therefore:

> The pinned C, source-faithful Rust, and production Rust sized-integer
> implementations are byte-equivalent throughout the published SAW domain;
> each Verus executable component refines its checked-in mathematical
> specification for every finite input in its stated contract; and each Kani
> production harness satisfies its property throughout its published domain.

Outside the SAW-sized-integer theorem, the human-reviewed mapping from pinned C
fragments to Verus specifications is not a theorem over compiled C. A green
formal workflow is not evidence about arbitrary future CoreFoundation
releases, C undefined behavior outside the defined-execution domain, or
unpublished current-Darwin internals.
