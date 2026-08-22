# Executable Verus CoreFoundation reader port

This directory contains an executable, structure-preserving port of the
self-contained property-list reader logic in Apple's pinned CoreFoundation
snapshot. The sources use Verus, a deductive verifier for executable Rust.
They retain the original APSL-2.0 notices and are separately licensed from the
MIT `plist-rs` crate.

Verus was selected because its executable mode is ordinary compiled Rust while
specification and proof modes are erased, and because it supports unbounded
contracts, loop invariants, termination proofs, mathematical integers, and
bit-vector reasoning. Kani remains as an independent bit-precise check of the
production crate's private kernels; it is not used as a substitute for these
deductive proofs.

## Pinned toolchain

| Item | Value |
| --- | --- |
| Verus | `0.2026.08.15.7d4628a` |
| Release tag | `release/0.2026.08.15.7d4628a` |
| Rust toolchain | `1.97.1` |
| vstd | `0.0.0-2026-08-09-0044` |

`verus.lock` records the official Linux and Apple-silicon macOS artifact
hashes. CI downloads the official Linux archive and checks its SHA-256 before
running it. The proof toolchain is isolated from the crate's Rust 1.74 MSRV.

## Reproduce

Install the pinned Verus release, then run:

```sh
VERUS_BIN=/path/to/verus verification/verus/run.sh
```

`run.sh` rejects trusted shortcuts, invokes Verus with `--no-cheating`, proves
and compiles every `.rs` file, then runs every erased executable. The recorded
2026-08-22 result is:

```text
binary_graph.rs:     33 verified, 0 errors
binary_objects.rs:   43 verified, 0 errors
binary_plist.rs:     25 verified, 0 errors
xml_plist.rs:        56 verified, 0 errors
xml_structure.rs:    55 verified, 0 errors
total:              212 verified, 0 errors
```

No source uses `assume`, `admit`, an axiom, an external body/item, an assumed
specification, or a termination bypass.

## Proven components

| File | Executable proof surface |
| --- | --- |
| `binary_plist.rs` | Seven-byte `bplist0` recognition; trailer extraction and LP64 scalar validation; checked add/multiply; all offset entries; root offset/marker; `_getSizedInt`; `_readInt`; checked ranges. |
| `binary_objects.rs` | Every marker; compact/extended counts; reference resolution; integer interpretation; raw real/date bits; data/eight-bit/UTF-16/UID spans; array/set/dictionary reference regions; complete allocation-independent wire dispatch. |
| `binary_graph.rs` | Cache/start-offset identity; scalar and UID equality model; primitive keys; fuel/active-path cycle rejection; sharing; set first-occurrence deduplication; binary dictionary first-equal-key retention. |
| `xml_plist.rs` | XML whitespace, comments, PIs, quote-blind DTD scans, numeric entities, permissive Base64 quartet emission, two-digit parsing, the historical integer grammar, and exact integer UTC calendar arithmetic. |
| `xml_structure.rs` | Start/close-tag cursors, ignored attributes, immediate slash semantics, misc selection, one-child plist, arrays, alternating dictionaries, duplicate-key last-wins, and single-entry `CF$UID` rewriting. |

Every executable function has an independent mathematical specification and a
contract quantified over all finite inputs in its stated domain. Loop and
recursive proofs carry invariants and decreasing measures.

## Exactness boundary

This is a direct, source-traced port of the mapped components, not a complete
formal theorem over the original C translation units. Verus proves executable
Rust against the checked-in mathematical specifications; a human-reviewed
mapping connects each specification to pinned C lines. `PORT_MAP.md` records
that correspondence and all totalized C undefined-behavior edges.

The current component theorem does not cover:

- a single byte-input bridge composing every binary proof file;
- token production and recursive XML value dispatch across both XML files;
- CoreFoundation allocation, retains, errors, mutability, or key paths;
- the CFString encoding database and Unicode conversion runtime;
- decimal-to-binary64 scanning and final date binary64 rounding;
- old-style/OpenStep fallback from `CFOldStylePList.c`;
- compiled-C-to-Verus equivalence or unpublished Darwin Foundation.

The production MIT backend is separately tested against these behaviors. It is
not described as a formally verified backend: the Verus files are an
APSL-isolated executable reference port, Kani checks selected production
kernels, and macOS differential tests compare observable results. This split
preserves the public crate's dependency footprint, license, and MSRV without
hiding the remaining proof bridge.
