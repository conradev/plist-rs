#![forbid(unsafe_code)]
#![warn(missing_docs)]
//! Fast, lossless parsing of binary and XML property lists.
//!
//! The original owned `Plist` API remains available with the default
//! `legacy-api` feature. Version 0.2 also
//! provides [`Document`] for source-backed parsing from a byte slice and
//! [`OwnedDocument`] for parsing readers into one owned source buffer. Parser
//! backends are selected independently through [`Parser`] and [`ParseOptions`].

use std::io::Read;

mod backend;
pub mod document;
mod error;
mod format;
mod frontend;
mod options;
#[cfg(feature = "legacy-api")]
mod plist;

pub use document::{
    ArrayIter, ArrayRef, Date, DictionaryIter, DictionaryRef, Document, Integer, OwnedDocument,
    Real, SetIter, SetRef, StringRef, Utf16Units, ValueKind, ValueRef,
};
pub use error::{Error, ErrorKind, Result};
pub use format::Format;
#[cfg(feature = "serde")]
pub use frontend::serde::{from_slice, from_slice_with_options};
#[cfg(feature = "serde-lite")]
pub use frontend::serde_lite::{from_slice_lite, from_slice_lite_with_options};
pub use options::{BackendKind, Limits, ParseOptions};
#[cfg(feature = "legacy-api")]
pub use plist::{Array, Dictionary, Plist};

/// A configured entry point to every parsing frontend.
#[derive(Clone, Debug, Default)]
pub struct Parser {
    options: ParseOptions,
}

impl Parser {
    /// Creates a parser with automatic format detection and the pure backend.
    pub const fn new() -> Self {
        Self {
            options: ParseOptions::new(),
        }
    }

    /// Creates a parser from a complete options value.
    pub const fn with_options(options: ParseOptions) -> Self {
        Self { options }
    }

    /// Creates a parser from a complete options value.
    ///
    /// This is an alias for [`Parser::with_options`] retained for callers that
    /// prefer constructor-style naming.
    pub const fn from_options(options: ParseOptions) -> Self {
        Self::with_options(options)
    }

    /// Returns this parser's options.
    pub const fn options(&self) -> &ParseOptions {
        &self.options
    }

    /// Selects an input format.
    pub const fn format(mut self, format: Format) -> Self {
        self.options = self.options.with_format(format);
        self
    }

    /// Selects a parser backend.
    pub const fn backend(mut self, backend: BackendKind) -> Self {
        self.options = self.options.with_backend(backend);
        self
    }

    /// Replaces the parser's resource limits.
    pub const fn limits(mut self, limits: Limits) -> Self {
        self.options = self.options.with_limits(limits);
        self
    }

    /// Parses a document while borrowing payloads from `source` when possible.
    ///
    /// XML entities, base64 data, and transcoded strings allocate only the
    /// normalized payloads required by their wire representation. UTF-16 and
    /// UTF-32 XML parsing may also use temporary transcoding workspace that is
    /// released before this method returns.
    pub fn parse<'source>(&self, source: &'source [u8]) -> Result<Document<'source>> {
        self.check_input_len(source.len())?;
        let parsed = backend::parse(source, &self.options)?;
        Ok(Document::from_parts(source, parsed))
    }

    /// Parses an already-owned source buffer without copying it.
    ///
    /// The returned document owns the exact `Box<[u8]>` allocation supplied by
    /// the caller. Payloads that can be represented as source spans continue to
    /// refer to this buffer.
    pub fn parse_owned(&self, source: Box<[u8]>) -> Result<OwnedDocument> {
        self.check_input_len(source.len())?;
        let parsed = backend::parse(&source, &self.options)?;
        Ok(OwnedDocument::from_parts(source, parsed))
    }

    /// Reads a document into one owned source buffer and parses spans over it.
    pub fn read<R: Read>(&self, input: R) -> Result<OwnedDocument> {
        let maximum = self.options.limits().max_input_bytes();
        let read_limit = maximum.saturating_add(1) as u64;
        let mut source = Vec::new();
        input
            .take(read_limit)
            .read_to_end(&mut source)
            .map_err(|source| {
                Error::from(source)
                    .with_format(self.options.format())
                    .with_backend(self.options.backend())
            })?;
        self.check_input_len(source.len())?;

        self.parse_owned(source.into_boxed_slice())
    }

    /// Parses a byte slice and projects it into the original owned value model.
    #[cfg(feature = "legacy-api")]
    pub fn parse_plist(&self, source: &[u8]) -> Result<Plist> {
        let document = self.parse(source)?;
        Plist::try_from_with_limit(document.root(), self.options.limits().max_objects())
    }

    /// Reads and projects a property list into the original owned value model.
    #[cfg(feature = "legacy-api")]
    pub fn read_plist<R: Read>(&self, input: R) -> Result<Plist> {
        let document = self.read(input)?;
        Plist::try_from_with_limit(document.root(), self.options.limits().max_objects())
    }

    fn check_input_len(&self, actual: usize) -> Result<()> {
        let maximum = self.options.limits().max_input_bytes();
        if actual <= maximum {
            Ok(())
        } else {
            Err(Error::new(ErrorKind::LimitExceeded)
                .with_format(self.options.format())
                .with_backend(self.options.backend())
                .with_message(format!(
                    "input contains {actual} bytes; configured maximum is {maximum}"
                )))
        }
    }
}
