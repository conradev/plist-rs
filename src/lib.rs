#![deny(unsafe_code,
        dead_code,
        unused_extern_crates,
        unused_imports,
        unused_features,
        unused_assignments,
        unused_variables,
        unused_import_braces,
        unused_qualifications,
        warnings,
        missing_debug_implementations,
        missing_docs,
        trivial_casts,
        trivial_numeric_casts)]
//! Property list parsing
//!
//! # Examples of use
//!
//! ## Parsing a `Plist` from a file
//!
//! ```rust
//! extern crate plist;
//!
//! use std::fs::File;
//! use std::io::BufReader;
//!
//! use plist::Plist;
//!
//! fn main() {
//!     let mut f = File::open("tests/types-xml.plist").unwrap();
//!     let mut reader = BufReader::new(f);
//!
//!     let plist = Plist::from_reader(&mut reader).unwrap();
//!
//!     println!("Property list {:?}", plist);
//! }
//! ```

extern crate chrono;
extern crate fnv;
extern crate rustc_serialize;
extern crate xml;

mod result;
mod plist;
mod reader;

pub use result::{Result, Error};
pub use plist::Plist;
