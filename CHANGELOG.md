# Changelog

## 0.2.0 — 2026-08-22

### Added

- Borrowed `Document` and source-owning `OwnedDocument` APIs with a lossless
  DAG value model.
- Pure and CoreFoundation-compatible parsing policies for binary and XML
  property lists.
- Explicit format, backend, and defensive resource-limit configuration through
  `Parser`, `ParseOptions`, and `Limits`.
- Optional full Serde and serde-lite typed frontends.
- Pinned compatibility references, an executable Verus port reporting 212
  verified component units, bounded Kani production harnesses,
  differential tests, stable benchmarks, and feature-matrix CI workflows.

### Preserved

- The original `Plist` enum variants and owned `Array`/FNV `Dictionary` model.
- `Plist::from_binary_reader`, `Plist::from_xml_reader`, and
  `Plist::from_reader`, including their starting-position behavior.

### Changed

- The public error is now a dependency-independent structured `Error` with a
  stable `ErrorKind`, format/backend/offset context, and optional source. Code
  that pattern-matched the old chrono/xml-rs/rustc-serialize payload variants
  must migrate to `Error::kind()`.
- The minimum supported Rust version is 1.74.
- Obsolete chrono 0.2, rustc-serialize, and xml-rs runtime dependencies were
  removed; the default parser's only runtime dependency is `fnv`, retained for
  exact legacy dictionary compatibility.
- Obsolete Travis CI publishing configuration and its encrypted deploy key were
  removed in favor of least-privilege GitHub Actions workflows.
