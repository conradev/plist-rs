# plist-rs

[![CI](https://github.com/conradev/plist-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/conradev/plist-rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/plist-rs.svg)](https://crates.io/crates/plist-rs)
[![docs.rs](https://docs.rs/plist-rs/badge.svg)](https://docs.rs/plist-rs)

A safe, dependency-light Rust reader for binary and XML property lists.

Version 0.2 keeps the original owned `Plist` API and adds a lossless,
source-backed document model, selectable parsing semantics, defensive resource
limits, Serde, and serde-lite.

## Compatibility API

Existing 0.1 parsing calls remain available:

```rust
use std::fs::File;
use plist::Plist;

let mut input = File::open("tests/types-xml.plist")?;
let value = Plist::from_reader(&mut input)?;
# Ok::<(), plist::Error>(())
```

The eight original `Plist` variants, its FNV-backed dictionary type, and
`from_binary_reader`, `from_xml_reader`, and `from_reader` signatures are
preserved. The structured 0.2 `Error` replaces the dependency-specific 0.1
error variants; see [the changelog](CHANGELOG.md).

## Lossless documents

`Parser::parse` builds a compact node index while borrowing compatible string
and data payloads directly from the caller's bytes:

```rust
use plist::{BackendKind, Parser, ValueKind};

let source = br#"<?xml version="1.0"?>
<plist version="1.0"><string>hello</string></plist>"#;
let document = Parser::new()
    .backend(BackendKind::Pure)
    .parse(source)?;

assert_eq!(document.root().kind(), ValueKind::String);
assert_eq!(document.root().string().unwrap().to_string()?, "hello");
# Ok::<(), plist::Error>(())
```

`Parser::parse_owned` consumes a `Box<[u8]>` without copying the source.
`Parser::read` reads into one owned source buffer. XML entity expansion,
Base64 decoding, and other representations whose semantic bytes do not exist
contiguously on the wire allocate the normalized payload they require.
UTF-16/32 XML decoding also uses temporary transcoding workspace while the
index is built; compatible UTF-16 string values still retain spans into the
original source, and that workspace is released before parsing returns.

The document model preserves binary nulls, UIDs, sets, shared object identity,
signed and unsigned integers through 128 bits, `f32` versus `f64`, raw
CoreFoundation date seconds, and primitive compatibility-profile dictionary
keys. Projection to the legacy `Plist` tree is fallible when that older model
cannot represent a value.

## Backends and formats

Format, parsing semantics, ownership, and typed frontend are independent axes:

| Axis | Choices |
| --- | --- |
| Format | automatic detection, binary, XML |
| Backend | `Pure` (default), `CoreFoundation` compatibility |
| Ownership | borrowed `Document`, source-owning `OwnedDocument`, owned `Plist` |
| Frontend | direct document views, legacy `Plist`, Serde, serde-lite |

Both backends are safe Rust and share hardened bounds-checked kernels. The pure
policy accepts the public plist grammar and favors canonical encodings. The
compatibility policy tracks the observable reader behavior of the pinned Apple
CoreFoundation sources, including accepted legacy and private encodings.

Enable compatibility semantics explicitly:

```toml
[dependencies]
plist-rs = { version = "0.2", features = ["backend-cf-compat"] }
```

```rust,no_run
use plist::{BackendKind, Parser};

# let bytes: &[u8] = br#"<plist version="1.0"><string>value</string></plist>"#;
let document = Parser::new()
    .backend(BackendKind::CoreFoundation)
    .parse(bytes)?;
# let _ = document;
# Ok::<(), plist::Error>(())
```

## Typed frontends

With the `serde` feature, a parsed document can deserialize types containing
borrowed fields:

```rust
# #[cfg(feature = "serde")]
# {
use serde::Deserialize;

#[derive(Deserialize)]
struct Config<'a> {
    name: &'a str,
}

# let xml_bytes: &[u8] = br#"<plist version="1.0"><dict><key>name</key><string>example</string></dict></plist>"#;
let document = plist::Parser::new().parse(xml_bytes)?;
let config: Config<'_> = document.deserialize()?;
# let _ = config.name;
# }
# Ok::<(), plist::Error>(())
```

The `serde-lite` feature provides `from_slice_lite`,
`Parser::deserialize_lite`, and `Document::deserialize_lite`. Its intermediate
model is owned and JSON-shaped, so it is not a zero-copy frontend. Data, dates,
UIDs, sets, integers wider than 64 bits, and non-string dictionary keys return
an error instead of being silently converted.

## Features

Default features are `binary`, `xml`, `backend-pure`, and `legacy-api`.
Optional features are:

- `backend-cf-compat`: CoreFoundation-compatible reader semantics.
- `serde`: full Serde deserialization and serialization of document views.
- `serde-lite`: smaller-code typed deserialization through an owned
  intermediate representation.

For a zero-dependency document parser without the legacy FNV dictionary facade:

```toml
[dependencies]
plist-rs = { version = "0.2", default-features = false, features = ["binary", "xml", "backend-pure"] }
```

There are no runtime C or XML-parser dependencies. The crate currently requires
Rust 1.74 or newer.

## Conformance and verification

[CONFORMANCE.md](CONFORMANCE.md) documents the profile differences, pinned
reference revisions, copying contract, and compatibility-oracle strategy.
[verification/README.md](verification/README.md) documents three complementary
machine-checked layers. SAW proves the pinned C `_getSizedInt`, a
source-faithful Rust port, and the exact production Rust kernel byte-equivalent
for every `u8` width and every possible byte buffer in the published
compiled-LLVM memory domain, both directly and against an independent Cryptol
specification. An executable Verus port proves broader source-traced reader
components, currently
212 verified units without trusted proof shortcuts. Kani checks selected
production parsing kernels throughout their published domains. These are real
component theorems, not an end-to-end theorem for either complete parser;
whole-program identity with unpublished current Darwin Foundation is not
claimed.

## License

The published Rust crate is MIT licensed. The source-faithful ports in
`verification/verus` and `verification/saw` are APSL-2.0 covered, retain their
own licenses and notices, and are excluded from the crates.io package. Other
Apple and Swift reference sources are not vendored; their licenses and
immutable revision hashes are recorded as compatibility provenance.
