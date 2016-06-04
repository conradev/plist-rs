use std::convert::From;
use std::error;
use std::fmt;
use std::io;
use std::num;
use std::result;
use std::str;
use std::string;
use chrono::format;
use rustc_serialize::base64;
use xml::reader;

/// The errors that can occur when parsing a property list.
#[derive(Debug)]
pub enum Error {
    /// The binary property list does not have valid magic bytes.
    InvalidMagicBytes,
    /// The binary property list trailer does not contain valid values.
    InvalidTrailer,
    /// The binary property list does not have a valid version.
    /// The only valid version is currently "00".
    VersionNotSupported(Option<String>),
    /// The binary property list has a key object that is not a string.
    InvalidKeyObject,
    /// The binary property list has an invalid nibble for a boolean.
    InvalidBoolean,
    /// The binary property list has an unsupported integer size for an offset, length,
    /// or count integer. This parser only supports u8, u16, u32 and u64 integers.
    InvalidIntegerSize,
    /// The binary property list has an unsupported object type.
    ObjectNotSupported(u8),

    /// The XML property list encountered an early end of the document.
    UnexpectedXmlEof,
    /// The XML property list contains an unexpected XML event.
    UnexpectedXmlEvent(reader::XmlEvent),
    /// The XML property list contains an unsupported object type.
    XmlObjectNotSupported(String),
    /// The XML property list contains invalid XML.
    XmlError(reader::Error),

    /// The reader experienced an I/O error.
    IoError(io::Error),
    /// The XML property list contains an invalid integer value
    IntError(num::ParseIntError),
    /// The XML property list contains an invalid float value
    FloatError(num::ParseFloatError),
    /// The XML property list contains an invalid date value
    DateError(format::ParseError),
    /// The XML property list contains an invalid data value
    Base64Error(base64::FromBase64Error),
    /// The property list contains an invalid UTF-8 string value
    Utf8Error(str::Utf8Error),
    /// The property list contains an invalid UTF-16 string value
    Utf16Error(string::FromUtf16Error),
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Error {
        Error::IoError(error)
    }
}

impl From<num::ParseIntError> for Error {
    fn from(error: num::ParseIntError) -> Error {
        Error::IntError(error)
    }
}

impl From<num::ParseFloatError> for Error {
    fn from(error: num::ParseFloatError) -> Error {
        Error::FloatError(error)
    }
}

impl From<format::ParseError> for Error {
    fn from(error: format::ParseError) -> Error {
        Error::DateError(error)
    }
}

impl From<base64::FromBase64Error> for Error {
    fn from(error: base64::FromBase64Error) -> Error {
        Error::Base64Error(error)
    }
}

impl From<str::Utf8Error> for Error {
    fn from(error: str::Utf8Error) -> Error {
        Error::Utf8Error(error)
    }
}

impl From<string::FromUtf8Error> for Error {
    fn from(error: string::FromUtf8Error) -> Error {
        Error::from(error.utf8_error())
    }
}

impl From<string::FromUtf16Error> for Error {
    fn from(error: string::FromUtf16Error) -> Error {
        Error::Utf16Error(error)
    }
}

impl From<reader::Error> for Error {
    fn from(error: reader::Error) -> Error {
        Error::XmlError(error)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::InvalidMagicBytes => write!(f, "Magic bytes are incorrect"),
            Error::InvalidTrailer => write!(f, "Trailer is invalid"),
            Error::VersionNotSupported(Some(ref s)) => write!(f, "Version {} not supported", s),
            Error::VersionNotSupported(None) => write!(f, "Version not supported"),
            Error::InvalidKeyObject => write!(f, "Key object is not a string"),
            Error::InvalidBoolean => write!(f, "Boolean object has an invalid value"),
            Error::InvalidIntegerSize => write!(f, "Integer size is not supported"),
            Error::ObjectNotSupported(ref v) => write!(f, "Object type 0x{:X} is not supported", v),
            Error::UnexpectedXmlEof => write!(f, "The XML file ends unexpectedly"),
            Error::UnexpectedXmlEvent(ref e) => write!(f, "The XML event {:?} is unexpected", e),
            Error::XmlObjectNotSupported(ref s) => {
                write!(f, "The XML object {:} is not supported", s)
            }
            Error::XmlError(ref e) => e.fmt(f),
            Error::IoError(ref e) => e.fmt(f),
            Error::IntError(ref e) => e.fmt(f),
            Error::FloatError(ref e) => e.fmt(f),
            Error::DateError(ref e) => e.fmt(f),
            Error::Base64Error(ref e) => e.fmt(f),
            Error::Utf8Error(ref e) => e.fmt(f),
            Error::Utf16Error(ref e) => e.fmt(f),
        }
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        match *self {
            Error::InvalidMagicBytes => "Magic bytes are incorrect",
            Error::InvalidTrailer => "Trailer is invalid",
            Error::VersionNotSupported(ref _s) => "Version not supported",
            Error::InvalidKeyObject => "Key object is not a string",
            Error::InvalidBoolean => "Boolean object has an invalid value",
            Error::InvalidIntegerSize => "Integer size is not supported",
            Error::ObjectNotSupported(ref _v) => "Object type is not supported",
            Error::UnexpectedXmlEof => "The XML stream ends unexpectedly",
            Error::UnexpectedXmlEvent(ref _e) => "The XML event is unexpected",
            Error::XmlObjectNotSupported(ref _s) => "The XML object is not supported",
            Error::XmlError(ref e) => e.description(),
            Error::IoError(ref e) => e.description(),
            Error::IntError(ref e) => e.description(),
            Error::FloatError(ref e) => e.description(),
            Error::DateError(ref e) => e.description(),
            Error::Base64Error(ref e) => e.description(),
            Error::Utf8Error(ref e) => e.description(),
            Error::Utf16Error(ref e) => e.description(),
        }
    }
}

/// The result type returned when parsing a property list
pub type Result<T> = result::Result<T, Error>;
