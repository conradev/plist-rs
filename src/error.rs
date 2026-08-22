//! Error and result types.

use std::borrow::Cow;
use std::error;
use std::fmt;
use std::io;

use crate::{BackendKind, Format};

/// A broad, stable category for a parsing or projection failure.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The input could not be read.
    Io,
    /// Binary property-list magic bytes were absent or invalid.
    InvalidMagic,
    /// The binary trailer was invalid.
    InvalidTrailer,
    /// The property-list version is not supported.
    UnsupportedVersion,
    /// The input is malformed for the selected format.
    Malformed,
    /// An object reference was invalid or outside the object table.
    InvalidReference,
    /// A dictionary key is invalid for the requested frontend.
    InvalidKey,
    /// An integer encoding or value was invalid.
    InvalidInteger,
    /// A floating-point encoding or value was invalid.
    InvalidReal,
    /// A date encoding or value was invalid.
    InvalidDate,
    /// A data encoding was invalid.
    InvalidData,
    /// A string encoding was invalid for the requested frontend.
    InvalidString,
    /// The wire format contains an unsupported object marker.
    UnsupportedObject,
    /// A lossless value cannot be represented by the requested frontend.
    UnsupportedValue,
    /// An integer cannot be represented by the requested frontend.
    IntegerOutOfRange,
    /// A date cannot be represented by the requested frontend.
    DateOutOfRange,
    /// A configured parser resource limit was exceeded.
    LimitExceeded,
    /// The selected backend was not compiled into the crate.
    BackendUnavailable,
    /// The selected format was not compiled into the crate.
    FormatUnavailable,
    /// An internal parser invariant was violated.
    Internal,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Io => "I/O error",
            Self::InvalidMagic => "invalid binary property-list magic bytes",
            Self::InvalidTrailer => "invalid binary property-list trailer",
            Self::UnsupportedVersion => "unsupported property-list version",
            Self::Malformed => "malformed property list",
            Self::InvalidReference => "invalid property-list object reference",
            Self::InvalidKey => "invalid property-list dictionary key",
            Self::InvalidInteger => "invalid property-list integer",
            Self::InvalidReal => "invalid property-list real",
            Self::InvalidDate => "invalid property-list date",
            Self::InvalidData => "invalid property-list data",
            Self::InvalidString => "invalid property-list string",
            Self::UnsupportedObject => "unsupported property-list object",
            Self::UnsupportedValue => "value is unsupported by this frontend",
            Self::IntegerOutOfRange => "integer is outside the frontend's range",
            Self::DateOutOfRange => "date is outside the frontend's range",
            Self::LimitExceeded => "property-list resource limit exceeded",
            Self::BackendUnavailable => "property-list backend is unavailable",
            Self::FormatUnavailable => "property-list format is unavailable",
            Self::Internal => "internal property-list parser error",
        })
    }
}

/// An error produced while parsing or projecting a property list.
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
    format: Option<Format>,
    backend: Option<BackendKind>,
    offset: Option<usize>,
    message: Option<Cow<'static, str>>,
    source: Option<Box<dyn error::Error + Send + Sync + 'static>>,
}

impl Error {
    /// Creates an error with the supplied category.
    pub fn new(kind: ErrorKind) -> Self {
        Self {
            kind,
            format: None,
            backend: None,
            offset: None,
            message: None,
            source: None,
        }
    }

    /// Creates an error located at a byte offset in a known format.
    pub fn at(kind: ErrorKind, format: Format, offset: usize) -> Self {
        Self::new(kind).with_format(format).with_offset(offset)
    }

    /// Creates a malformed-input error with explanatory context.
    pub fn parse(format: Format, offset: usize, message: impl Into<Cow<'static, str>>) -> Self {
        Self::at(ErrorKind::Malformed, format, offset).with_message(message)
    }

    /// Returns the stable category of this error.
    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Returns the input format associated with this error, when known.
    pub const fn format(&self) -> Option<Format> {
        self.format
    }

    /// Returns the backend associated with this error, when known.
    pub const fn backend(&self) -> Option<BackendKind> {
        self.backend
    }

    /// Returns the byte offset associated with this error, when known.
    pub const fn offset(&self) -> Option<usize> {
        self.offset
    }

    /// Returns additional human-readable context, when available.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Adds input-format context.
    pub fn with_format(mut self, format: Format) -> Self {
        self.format = Some(format);
        self
    }

    /// Adds parser-backend context.
    pub fn with_backend(mut self, backend: BackendKind) -> Self {
        self.backend = Some(backend);
        self
    }

    /// Adds a byte offset.
    pub fn with_offset(mut self, offset: usize) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Adds explanatory context.
    pub fn with_message(mut self, message: impl Into<Cow<'static, str>>) -> Self {
        self.message = Some(message.into());
        self
    }

    pub(crate) fn with_source(mut self, source: impl error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.kind)?;
        if let Some(message) = &self.message {
            write!(formatter, ": {message}")?;
        }
        if let Some(offset) = self.offset {
            write!(formatter, " at byte {offset}")?;
        }
        if let Some(format) = self.format {
            write!(formatter, " ({format})")?;
        }
        Ok(())
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn error::Error + 'static))
    }
}

impl From<io::Error> for Error {
    fn from(source: io::Error) -> Self {
        Self::new(ErrorKind::Io)
            .with_message(source.to_string())
            .with_source(source)
    }
}

/// The result type returned by property-list operations.
pub type Result<T> = std::result::Result<T, Error>;
