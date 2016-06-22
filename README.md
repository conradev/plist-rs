# plist-rs

[![crates.io](http://meritbadge.herokuapp.com/plist-rs)](https://crates.io/crates/plist-rs)
[![Build Status](https://travis-ci.org/conradev/plist-rs.svg?branch=master)](https://travis-ci.org/conradev/plist-rs)

[API Documentation][API documentation]

plist-rs is a property list parser written in Rust.

### Features

- Supports reading both XML and binary property lists
- Equivalent performance to Apple's `CFBinaryPlist` implementation

### To Do

- Writing property lists
- Zero-copy parsing of binary property lists

### Known Issues

- XML parsing is [slower](https://github.com/netvl/xml-rs/issues/126) than other libraries

## Getting Started

Add plist-rs as a dependency in your [`Cargo.toml`](http://crates.io/) file:

```toml
[dependencies]
plist-rs = "*"
```
[API documentation]: https://conradev.github.io/plist-rs
