//! Parser selection and resource limits.

use crate::Format;

/// The parser implementation to use.
///
/// Backend selection is independent of format and frontend selection. Both
/// backends can therefore power the legacy value API, documents, and optional
/// typed frontends.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum BackendKind {
    /// The dependency-light, performance-oriented Rust implementation.
    #[default]
    Pure,
    /// The Rust port that tracks CoreFoundation parsing semantics.
    CoreFoundation,
}

/// Resource limits applied while parsing untrusted input.
///
/// Limits are deliberately explicit. Compatibility semantics apply within
/// these safety boundaries, and applications can choose tighter values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    max_input_bytes: usize,
    max_depth: usize,
    max_objects: usize,
    max_container_len: usize,
    max_string_bytes: usize,
    max_data_bytes: usize,
}

impl Limits {
    /// Creates the default set of defensive limits.
    pub const fn new() -> Self {
        Self {
            max_input_bytes: 256 * 1024 * 1024,
            max_depth: 512,
            max_objects: 1_000_000,
            max_container_len: 1_000_000,
            max_string_bytes: 128 * 1024 * 1024,
            max_data_bytes: 256 * 1024 * 1024,
        }
    }

    /// Returns the maximum accepted input size in bytes.
    pub const fn max_input_bytes(&self) -> usize {
        self.max_input_bytes
    }

    /// Returns the maximum container nesting depth.
    pub const fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// Returns the maximum number of parsed objects.
    ///
    /// The same value bounds nodes materialized by the legacy `Plist` facade
    /// and optional typed frontends, preventing shared binary DAGs from
    /// expanding without limit.
    pub const fn max_objects(&self) -> usize {
        self.max_objects
    }

    /// Returns the maximum number of direct members in one container.
    pub const fn max_container_len(&self) -> usize {
        self.max_container_len
    }

    /// Returns the maximum byte storage used by one string payload.
    ///
    /// Source-backed strings are measured in their wire byte span (including
    /// UTF-16 code units); normalized strings are measured in UTF-8 bytes.
    pub const fn max_string_bytes(&self) -> usize {
        self.max_string_bytes
    }

    /// Returns the maximum decoded size of one data value.
    pub const fn max_data_bytes(&self) -> usize {
        self.max_data_bytes
    }

    /// Sets the maximum accepted input size in bytes.
    pub const fn with_max_input_bytes(mut self, value: usize) -> Self {
        self.max_input_bytes = value;
        self
    }

    /// Sets the maximum container nesting depth.
    pub const fn with_max_depth(mut self, value: usize) -> Self {
        self.max_depth = value;
        self
    }

    /// Sets the maximum number of parsed or frontend-materialized objects.
    pub const fn with_max_objects(mut self, value: usize) -> Self {
        self.max_objects = value;
        self
    }

    /// Sets the maximum number of direct members in one container.
    pub const fn with_max_container_len(mut self, value: usize) -> Self {
        self.max_container_len = value;
        self
    }

    /// Sets the maximum byte storage used by one string payload.
    pub const fn with_max_string_bytes(mut self, value: usize) -> Self {
        self.max_string_bytes = value;
        self
    }

    /// Sets the maximum decoded size of one data value.
    pub const fn with_max_data_bytes(mut self, value: usize) -> Self {
        self.max_data_bytes = value;
        self
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration shared by all parsing frontends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseOptions {
    format: Format,
    backend: BackendKind,
    limits: Limits,
}

impl ParseOptions {
    /// Creates options using automatic format detection and the pure backend.
    pub const fn new() -> Self {
        Self {
            format: Format::Auto,
            backend: BackendKind::Pure,
            limits: Limits::new(),
        }
    }

    /// Returns the requested input format.
    pub const fn format(&self) -> Format {
        self.format
    }

    /// Returns the selected parser backend.
    pub const fn backend(&self) -> BackendKind {
        self.backend
    }

    /// Returns the active resource limits.
    pub const fn limits(&self) -> &Limits {
        &self.limits
    }

    /// Selects an input format.
    pub const fn with_format(mut self, format: Format) -> Self {
        self.format = format;
        self
    }

    /// Selects a parser backend.
    pub const fn with_backend(mut self, backend: BackendKind) -> Self {
        self.backend = backend;
        self
    }

    /// Replaces the parser resource limits.
    pub const fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self::new()
    }
}
