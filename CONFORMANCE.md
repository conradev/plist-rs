# Property-list conformance

`plist-rs` 0.2 distinguishes two promises which Apple itself does not treat as
the same thing.

## Profiles

- **Pure** accepts the public property-list data model: strings, data, dates,
  finite and non-finite numbers, booleans, arrays, and string-keyed
  dictionaries. It favors a small, predictable grammar and is the default.
- **CoreFoundation compatibility** follows the observable reader behavior of
  the pinned open-source CoreFoundation implementation. In addition to public
  plist values this includes binary nulls, UIDs, sets, primitive dictionary
  keys, non-power-of-two table widths, `bplist0?` headers, hexadecimal XML
  integers, permissive Base64, and other documented compatibility behavior.

The compatibility profile is a reader contract. Values accepted by it are not
necessarily values that `CFPropertyListIsValid` permits applications to write.

## Copying model

`Parser::parse` indexes a caller-owned byte slice. Binary data and compatible
UTF-8/ASCII strings and binary UTF-16 code units are spans into that slice. XML
entity expansion, Base64, and UTF-32 transcoding necessarily allocate; these
payloads are owned by the parsed document. UTF-16/32 XML decoding also uses
temporary transcoding workspace during parsing; compatible UTF-16 strings keep
source spans and the workspace is not retained. `Parser::read` reads into one
owned source buffer and uses the same span-based index.

The legacy `Plist` facade remains an owned tree. Converting a document to that
facade is a fallible traversal bounded by the parser's object limit (or the
default object limit for `TryFrom<ValueRef>`). Serde deserialization and
serialization and serde-lite's owned intermediate conversion use the same
materialization budget. Shared binary references may appear more than once in
a projected value, but cannot expand beyond that budget.

## Compatibility oracle

The compatibility origin is the historical Apple CF mirror supplied for this
work, `opensource-apple/CF` commit
`3cc41a76b1491f50813e28a4ec09954ffa359e6f`. The binary reader is in
`CFBinaryPList.c`; XML parsing is part of `CFPropertyList.c`--there is no
`CFXMLPropertyList.c` in that revision.

Security fixes and later observable behavior are cross-checked against Swift
Corelibs Foundation commit `761b621da93a856a48995efc29ed11028c283306`
(2026-08-20). The former is APSL-2.0; the latter is Apache-2.0 with the Swift
runtime exception. Their behaviors are versioned separately rather than
silently treating one as the other.

The open implementation is a non-Darwin compatibility implementation, not the
source of the current macOS Foundation binary. Verification therefore draws on:

1. the pinned historical C source for reproducible semantic tracing and
   bounded proofs;
2. the current open Corelibs descendant for hardening regressions; and
3. the installed Darwin Foundation/plutil implementation as a differential
   behavioral oracle on macOS CI.

## Verification claim

The 0.2 machine checks have three deliberately distinct targets.

SAW establishes a compiled cross-language byte theorem for the sized-integer
reader kernel:

> For every width representable by `uint8_t` and every initialized input byte
> in the complete span, pinned C `_getSizedInt`, the source-faithful Rust port,
> and the exact production Rust kernel return the same 64 output bits and equal
> the independent Cryptol recurrence.

The generic C path is alignment-free. Native x86 verification covers Apple's
typed-load fast path under an eight-byte-alignment precondition. All three
pairwise relations are checked directly in addition to the shared-spec
refinements; future compiler and adapter drift cannot rely only on transitivity.
An additional checked composition covers `_readInt`'s successful-path value and
cursor arithmetic, including its historical `uint64_t`-to-`uint8_t` width
conversion. Its C projection adapter omits the original pointer/bounds guards
and remains explicitly trusted; it is not a literal compiled theorem for the
complete `_readInt` function.

The executable Verus port establishes an unbounded component theorem:

> For every finite input satisfying a mapped component's contract, the
> executable Rust result equals that component's checked-in mathematical
> specification.

The current Verus suite reports 212 verified units spanning binary
header/trailer/offset scanning, wire-object dispatch, an abstract object graph,
XML lexical and scalar behavior, and abstract XML structure transitions. The
mapping from pinned C fragments to the mathematical specifications is
human-reviewed; Verus does not ingest or prove the original C translation
units. The graph and XML layers also retain documented byte-to-model bridge
obligations, so this is not an end-to-end proof of the production parser.

Kani independently establishes a bounded production-kernel theorem:

> Under the stated 64-bit platform assumptions, the verified scalar decoding
> kernels have the same result as their executable specifications for every
> value in each published harness domain.

Those harnesses cover the bounded integer fold, format-`00` signedness, checked
object-range arithmetic, default-limit table-layout arithmetic, address-width
capacity, and inline-count marker selection in the actual MIT crate. Complete
decoders are additionally checked by deterministic malformed-input,
regression, and macOS differential corpora. A pinned Verus binary, Rust, vstd,
Z3, and the human C-to-spec transcription form the Verus trusted boundary;
Kani and its compiler form the production-kernel proof boundary. SAW/Crucible,
Cryptol, Z3, Rust/LLVM, Clang, llvm-link, the proof adapters, and the published
source/LLVM preconditions form the cross-language proof boundary.

Allocation failure, the full CoreFoundation string/encoding and floating-point
conversion runtimes, object retain/mutability/logging behavior, exact localized
error text, old-style plist fallback, compiled-C equivalence outside the
sized-integer theorem, and unpublished current-Darwin internals remain outside
the formal claim.
`verification/README.md` records the exact domains and bridge obligations.

That boundary matters: neither deductive verification of isolated components
nor bounded model checking of production kernels is a proof of universal,
current-macOS whole-program equivalence.

## Deliberate and known deviations

Compatibility mode is bounded by the configured input, object, collection,
string, data, and nesting limits. The pinned historical C instead relied on
available address space and recursive stack depth; the default depth caps are
drawn from the hardened Corelibs descendant. Explicitly fallible parser
workspaces report allocation failure as `LimitExceeded`; process-wide allocator
exhaustion remains outside the compatibility contract.

Historical undefined behavior is not reproduced. In particular, non-ASCII
bytes in the permissive Base64 decoder and NaN-to-integer conversion for a
`CF$UID` dictionary (including out-of-range floating-point casts) are handled
deterministically and safely. Malformed dates
whose historical calculation could index outside its month table are rejected
or normalized only within the documented safe domain.

The zero-dependency XML implementation recognizes UTF-8, UTF-16, and UTF-32
encodings. It does not include CoreFoundation's platform IANA-character-set
registry, and uses deterministic XML byte signatures rather than the pinned
reader's host-dependent BOM-less UTF-16 heuristic. The compatibility profile
retains the historical wrapping, single-UTF-16-unit numeric-entity behavior. A
wrapped unpaired surrogate disappears when that one-unit value is converted to
UTF-8, matching the pinned reader; the pure profile requires Unicode scalar
values.
