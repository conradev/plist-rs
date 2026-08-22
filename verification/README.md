# Verification

This directory records the reproducible reference and the machine-checked
contract for the CoreFoundation-compatible backend.

## Reference lock

| Item | Value |
| --- | --- |
| Primary repository | `opensource-apple/CF` |
| Primary commit | `3cc41a76b1491f50813e28a4ec09954ffa359e6f` |
| Target model | 64-bit `size_t`, `long`, and pointers; allocation succeeds |
| Primary `CFBinaryPList.c` SHA-256 | `8b8c72427ba60e7918f3ffa088d9f25859456eb75a0081ff5cdb7277ff62c384` |
| Primary `CFPropertyList.c` SHA-256 | `ced92800b4651fc47e9a1f53626170c7c7324db50bf6740541fd56844b7bead3` |
| Primary `APPLE_LICENSE` SHA-256 | `212846e0145aa50fb3a5aef254a370311a93acf6c1e792e47e0068d64c8c3885` |
| Hardening descendant | `swiftlang/swift-corelibs-foundation@761b621da93a856a48995efc29ed11028c283306` |
| Descendant `CFBinaryPList.c` SHA-256 | `58741c5361b3eb7aec25b8700887ec8259bc64f31a489914ec3c44ab5d077415` |
| Descendant `CFPropertyList.c` SHA-256 | `23dbbff0c24488a65b7e61ce59f65d4c55b9e6a787c4be6300dfe28a3896de04` |

Pinned sources:

- <https://github.com/opensource-apple/CF/blob/3cc41a76b1491f50813e28a4ec09954ffa359e6f/CFBinaryPList.c>
- <https://github.com/opensource-apple/CF/blob/3cc41a76b1491f50813e28a4ec09954ffa359e6f/CFPropertyList.c>
- <https://github.com/swiftlang/swift-corelibs-foundation/blob/761b621da93a856a48995efc29ed11028c283306/Sources/CoreFoundation/CFBinaryPList.c>
- <https://github.com/swiftlang/swift-corelibs-foundation/blob/761b621da93a856a48995efc29ed11028c283306/Sources/CoreFoundation/CFPropertyList.c>

The Rust code is a new safe implementation of the observable contract and is
MIT licensed. The upstream C files are not copied or vendored into the crate;
their APSL-2.0 and Apache-2.0 licenses apply to those separately linked
references. Commit pins and hashes make the behavioral provenance auditable
without presenting the reference sources as part of this package.
`verification/verify-references.sh` downloads those immutable revisions and
checks every recorded hash; the formal-verification workflow runs it before
the model checker.

## Observable contract

The comparison result is one of:

- rejection in a stable error category; or
- an accepted typed graph preserving integer value, float width and bits, date
  seconds, collection kind, object references, and dictionary keys. The 0.2
  document does not retain an integer's original byte width.

Addresses, allocator calls, retain counts, mutability, logs, and localized error
messages are not compared.

## Machine-checked layer shipped in 0.2

Kani harnesses live next to the private production kernels in
`src/backend/cf_compat/binary.rs`; they are not duplicate test-only
implementations. `verification/kani/run.sh` runs all of them. The independent
specifications use wider exact arithmetic or direct format equations instead
of the checked/wrapping operations under test.

| Harness | Complete input domain | Property |
| --- | --- | --- |
| `wide_be_u64_is_the_low_half_of_the_exact_value` | every 0-16 byte string | the production fold is the low 64 bits of the exact big-endian integer |
| `format_00_integer_signedness_matches_the_specification` | every `u64` payload at widths 1, 2, 4, 8, and 16 | the production integer-class rule matches its executable scalar specification |
| `object_range_check_matches_exact_arithmetic` | every 64-bit `start`, `length`, and `limit` | overflow and out-of-object-table results match `u128` arithmetic |
| `bounded_table_layout_matches_exact_arithmetic` | every table position through 256 MiB, object count through 1,000,000, and every `u8` width | under the default parser limits, table multiplication and final-file addition match `u128` arithmetic without overflow |
| `address_width_check_matches_exact_capacity` | every 64-bit inclusive maximum and every `u8` width | the field-width capacity test matches the exact power-of-two bound |
| `inline_count_marker_matches_the_format_nibble` | all 256 marker bytes | nibbles 0-14 are inline and nibble 15 selects an extended count |

The first two properties model the pinned source's `_getSizedInt` and format
`00` integer branches. The range and layout properties model the scalar
preconditions around them. Except for the explicitly bounded byte-fold and
default-limit table-layout harnesses, they cover all scalar values in the
stated 64-bit model. The 16-byte fold bound matches the maximum integer payload
accepted by this decoder.

## What this does not prove

No SAW proof of the complete pinned C translation unit is shipped. The C file
depends on the rest of CoreFoundation, compiler/ABI behavior, allocation, and
pointer validity. The Kani result therefore proves the production Rust kernels
against executable specifications traced to the pinned C source; treating that
trace as a direct, whole-parser C-to-Rust theorem would overstate the result.

Container graph construction, Unicode and XML behavior, allocation failure,
object identity, mutability, logging, localized error strings, and unpublished
current-Darwin internals remain outside this formal layer. They require corpus,
fuzzing, and macOS differential testing; deterministic corpora and the macOS
differential suite ship today, while a sustained fuzz campaign is future work.
A green Kani run is neither an unbounded whole-parser proof nor evidence about
arbitrary future CoreFoundation releases.
