//! Property-list formats.

use std::fmt;

/// The wire format to parse.
///
/// [`Format::Auto`] asks the selected backend to inspect the input. The
/// explicit variants are useful when the caller already knows the format and
/// wants malformed input to fail without trying another parser.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Format {
    /// Detect binary or XML input.
    #[default]
    Auto,
    /// Apple's binary property-list format.
    Binary,
    /// Apple's XML property-list format.
    Xml,
}

impl fmt::Display for Format {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Auto => "auto",
            Self::Binary => "binary",
            Self::Xml => "XML",
        })
    }
}
