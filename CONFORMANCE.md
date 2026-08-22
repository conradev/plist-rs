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

The 0.2 proof target is deliberately narrower than whole-program equivalence:

> Under the stated 64-bit platform assumptions, the verified scalar decoding
> kernels have the same result as their executable specifications for every
> value in each published harness domain.

Machine checks cover the bounded integer fold, format-`00` signedness,
checked object-range arithmetic, default-limit table-layout arithmetic,
address-width capacity, and inline-count marker selection. Complete decoders
are additionally checked by deterministic malformed-input, regression, and
macOS differential corpora. Extended-count payload parsing, allocation failure,
CoreFoundation object identity,
mutability, logging, exact localized error text, and unpublished current-Darwin
internals are outside the formal claim. `verification/README.md` records the
exact domains and trusted boundary.

That boundary matters: calling bounded model checking or fuzzing a proof of
universal, current-macOS equivalence would be inaccurate.

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
