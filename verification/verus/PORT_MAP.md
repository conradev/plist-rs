# Pinned CoreFoundation reader port map

This map defines what “exact port” means for the executable Verus sources in
this directory. It prevents similarity, test parity, or a proof of a few helper
functions from being reported as whole-reader equivalence.

## Reference model

| Item | Pinned value |
| --- | --- |
| Repository | `opensource-apple/CF` |
| Commit | `3cc41a76b1491f50813e28a4ec09954ffa359e6f` |
| Binary reader | `CFBinaryPList.c` |
| XML reader | `CFPropertyList.c` |
| Calendar dependency | `CFDate.c` |
| Marker/trailer definitions | `ForFoundationOnly.h` |
| ABI | LP64: 64-bit pointers, `size_t`, `CFIndex`, and `long` |
| Normal option | immutable values, no key-path filtering |
| Allocation model | allocation succeeds |

`../reference.lock` records immutable hashes for every listed source. The
source-traced port is APSL-2.0-covered code; `LICENSE` and `NOTICE` apply to
this directory. It is excluded from the MIT crates.io package.

## Complete-reader target relation

The eventual end-to-end observable result would be one of `NotRecognized`,
`Rejected(ErrorClass)`, or `Accepted(ValueGraph)`. A normalized accepted graph
would preserve:

- nulls and booleans;
- integer value and CoreFoundation signedness class;
- `f32`, `f64`, and date width/raw bits;
- data bytes and strings as UTF-16 code units;
- UID value and source-object identity;
- shared binary references, array order, and collection kind;
- set membership after CoreFoundation equality;
- binary dictionary first-equal-key behavior;
- XML dictionary last-equal-key behavior.

Allocator addresses, retain counts, mutability, logging, localized messages,
and string-interner implementation identity are not observable.

The current files do not establish that complete byte-input theorem. They prove
the component correspondences listed below.

## Established component theorem

For every mapped executable component `f` and every finite input satisfying
its contract, Verus establishes:

```text
f_exec(input) == f_spec(input)
```

Each `f_spec` is an independent mathematical transcription of a named, pinned
C source fragment. The mapping from that C branch to the specification remains
a reviewable translation boundary: Verus does not ingest or prove the C
translation unit. There is not yet a theorem connecting every byte-input path
through allocation, the CoreFoundation runtime, and the normalized graph.

For a historical C undefined-behavior edge, the safe port instead has a total,
documented result. Those cases are not described as ISO-C equivalence.

## Binary reader correspondence

| Pinned C logic | Verus executable port | Status |
| --- | --- | --- |
| checked `u64` add/multiply helpers | `checked_add_u64`, `checked_mul_u64` | Proved for all pairs |
| `_getSizedInt` | `get_sized_int` | Proved for every 0-255-byte width and valid position |
| `__CFBinaryPlistGetTopLevelInfo` | `extract_trailer`, `validate_trailer_fields`, `header_matches`, `get_top_level_info` | Full scalar, offset-table, root-offset, and marker scan proved |
| `_readInt` | `read_int`, `decode_count` | Compact/extended counts and the historical width narrowing proved |
| `_getOffsetOfRefAt` | `resolve_reference` | Bounds, reference, and offset resolution proved |
| marker switch in `__CFBinaryPlistCreateObjectFiltered` | `decode_wire_object` and marker-specific `decode_*` functions | Allocation-independent dispatch, bits, spans, and reference regions proved |
| recursive object construction and cache | `binary_graph.rs` | Proved over abstract bounds-checked wire nodes; byte-to-node composition remains a bridge |
| `_plistIsPrimitive`, set equality, dictionary construction | `binary_graph.rs` | Proved over abstract equality classes; deriving collision-free/deep classes from runtime values remains a bridge |
| allocation, retain/release, mutable containers, key-path filtering | none | Outside the immutable/unfiltered reader theorem |
| binary writer half | none | Outside reader scope |

`binary_plist.rs` covers the top-level/trailer path. `binary_objects.rs` covers
the complete allocation-independent object-wire dispatcher. The graph layer
is separated so byte safety and object-runtime semantics cannot be conflated.

## XML reader correspondence

| Pinned C logic | Verus executable port | Status |
| --- | --- | --- |
| four-byte XML whitespace and maximal skip | `is_xml_whitespace`, `skip_xml_whitespace` | Proved for every byte/vector |
| comment and processing-instruction scans | `skip_xml_comment`, `skip_xml_processing_instruction` | Exact cursor/EOF boundaries proved |
| quote-blind DOCTYPE/declaration scans | `has_doctype`, `skip_dtd`, `scan_quote_blind_dtd_declaration` | Exact first-`[`/`>` behavior proved |
| numeric entity digit/fold | `entity_digit`, `fold_numeric_entity` | Wrapping single-`u16` result and cursor proved |
| permissive Base64 table and quartet emission | `base64_value`, `emit_base64_quad` | Table and one complete quartet proved |
| `read2DigitNumber` | `read_two_digit_number` | Value and historical cursor boundary proved |
| integer whitespace and lexical scan | `is_cf_integer_whitespace_at`, `scan_cf_integer` | Sign, `0x`, leading zero, overflow, and cursor behavior proved |
| `CFGregorianDateGetAbsoluteTime` integer UTC core | `cf_*` calendar functions | Leap/table/400-year/YMD/seconds arithmetic proved |
| tag/close-tag/collection state transitions | `xml_structure.rs` | Proved over abstract tokens/nodes; tokenizer and recursive-dispatch composition remain a bridge |
| decimal-to-IEEE conversion | none | Requires a verified `strtod`/scanner port; raw float result remains outside this theorem |
| CoreFoundation IANA encodings and UTF conversion | none | Requires the pinned encoding database/converters |
| old-style/OpenStep fallback | none | Defined in `CFOldStylePList.c`, outside XML-backend scope |
| XML writer half | none | Outside reader scope |

## Historical C undefined behavior

The equivalence domain excludes C executions that depend on undefined or
implementation-defined behavior, including:

- unaligned typed loads on architectures where they are invalid;
- signed Base64 accumulator overflow and negative signed-byte table indexes;
- negative `char` arguments to `ctype` macros;
- signed year overflow or calendar-table out-of-bounds indexing;
- out-of-range floating-to-integer conversion for `CF$UID`;
- pointer arithmetic outside a live object or through lossy integer casts;
- malformed BOM-adjusted XML pointer ranges.

The Rust port uses offsets, checked arithmetic, explicit wrapping where the
pinned machine behavior is part of the target, and deterministic rejection or
safe total values everywhere else.

## Trusted base and prohibited shortcuts

The proof trusts the pinned Verus binary, its Rust compiler, vstd, Z3, and the
human C-to-spec transcription. `run.sh` uses `--no-cheating`, compiles every
executable port, and runs the erased programs. `deny-trusted-shortcuts.sh`
separately rejects assumptions, admissions, external bodies/items, assumed
specifications, axioms, and termination bypass attributes.

This is a mechanized, source-traced executable port. It is not a theorem over
compiled Apple C or unpublished current Darwin Foundation.
