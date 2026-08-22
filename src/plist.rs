//! The compatibility value frontend.

use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::io::{Read, Seek, SeekFrom};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fnv::FnvHasher;

use crate::{Error, ErrorKind, Format, Limits, Parser, Result, ValueKind, ValueRef};

/// Represents a property list value using the original 0.1 data model.
///
/// The lossless document API supports additional CoreFoundation values. A
/// conversion to `Plist` fails rather than silently narrowing those values.
#[derive(Debug, Clone, PartialEq)]
pub enum Plist {
    /// An array or vector of plist objects.
    Array(Array),
    /// A dictionary or hash map of plist objects, keyed by string.
    Dict(Dictionary),
    /// A Boolean value.
    Boolean(bool),
    /// A data value.
    Data(Vec<u8>),
    /// A date value.
    DateTime(SystemTime),
    /// A floating-point value.
    Real(f64),
    /// A signed 64-bit integer value.
    Integer(i64),
    /// A UTF-8 string value.
    String(String),
}

/// The array representation used by [`Plist`].
pub type Array = Vec<Plist>;

/// The FNV-backed dictionary representation used by [`Plist`].
pub type Dictionary = HashMap<String, Plist, BuildHasherDefault<FnvHasher>>;

impl Plist {
    /// Decodes a binary property list value from a reader.
    ///
    /// As in 0.1, parsing starts at byte zero rather than the reader's current
    /// position.
    pub fn from_binary_reader<R: Read + Seek>(input: &mut R) -> Result<Self> {
        input.seek(SeekFrom::Start(0))?;
        let document = Parser::new().format(Format::Binary).read(input)?;
        Self::try_from(document.root())
    }

    /// Decodes an XML property list value from a reader.
    ///
    /// As in 0.1, XML parsing begins at the reader's current position.
    pub fn from_xml_reader<R: Read>(input: &mut R) -> Result<Self> {
        let document = Parser::new().format(Format::Xml).read(input)?;
        Self::try_from(document.root())
    }

    /// Decodes a binary or XML property list value from a reader.
    ///
    /// As in 0.1, automatic parsing starts at byte zero.
    pub fn from_reader<R: Read + Seek>(input: &mut R) -> Result<Self> {
        input.seek(SeekFrom::Start(0))?;
        let document = Parser::new().format(Format::Auto).read(input)?;
        Self::try_from(document.root())
    }
}

impl TryFrom<ValueRef<'_>> for Plist {
    type Error = Error;

    fn try_from(value: ValueRef<'_>) -> Result<Self> {
        Self::try_from_with_limit(value, Limits::new().max_objects())
    }
}

impl Plist {
    pub(crate) fn try_from_with_limit(value: ValueRef<'_>, maximum: usize) -> Result<Self> {
        let format = value.format();
        let backend = value.backend();
        let mut remaining = maximum;
        project(value, &mut Vec::new(), &mut remaining)
            .map_err(|error| error.with_format(format).with_backend(backend))
    }
}

fn project<'document>(
    value: ValueRef<'document>,
    path: &mut Vec<ValueRef<'document>>,
    remaining: &mut usize,
) -> Result<Plist> {
    *remaining = remaining.checked_sub(1).ok_or_else(|| {
        Error::new(ErrorKind::LimitExceeded)
            .with_message("legacy Plist projection exceeds the configured object limit")
    })?;

    match value.kind() {
        ValueKind::Null | ValueKind::Uid | ValueKind::Set => Err(unsupported(value.kind())),
        ValueKind::Boolean => value
            .as_bool()
            .map(Plist::Boolean)
            .ok_or_else(internal_projection_error),
        ValueKind::Integer => {
            let integer = value
                .as_integer()
                .ok_or_else(internal_projection_error)?
                .as_i64()
                .ok_or_else(|| Error::new(ErrorKind::IntegerOutOfRange))?;
            Ok(Plist::Integer(integer))
        }
        ValueKind::Real => value
            .as_real()
            .map(|real| Plist::Real(real.as_f64()))
            .ok_or_else(internal_projection_error),
        ValueKind::Date => {
            let date = value.as_date().ok_or_else(internal_projection_error)?;
            Ok(Plist::DateTime(system_time(date.unix_timestamp())?))
        }
        ValueKind::Data => value
            .as_data()
            .map(|data| Plist::Data(data.to_vec()))
            .ok_or_else(internal_projection_error),
        ValueKind::String => {
            let string = value
                .string()
                .ok_or_else(internal_projection_error)?
                .to_string()?
                .into_owned();
            Ok(Plist::String(string))
        }
        ValueKind::Array => with_container(value, path, |path| {
            let array = value.as_array().ok_or_else(internal_projection_error)?;
            ensure_direct_children(array.len(), *remaining)?;
            let mut projected = Vec::with_capacity(array.len());
            for member in array {
                projected.push(project(member, path, remaining)?);
            }
            Ok(Plist::Array(projected))
        }),
        ValueKind::Dictionary => with_container(value, path, |path| {
            let dictionary = value
                .as_dictionary()
                .ok_or_else(internal_projection_error)?;
            ensure_direct_children(dictionary.len(), *remaining)?;
            let hasher = BuildHasherDefault::<FnvHasher>::default();
            let mut projected = HashMap::with_capacity_and_hasher(dictionary.len(), hasher);
            for (key, member) in dictionary {
                let key = key
                    .string()
                    .ok_or_else(|| Error::new(ErrorKind::InvalidKey))?
                    .to_string()?
                    .into_owned();
                projected.insert(key, project(member, path, remaining)?);
            }
            Ok(Plist::Dict(projected))
        }),
    }
}

fn ensure_direct_children(children: usize, remaining: usize) -> Result<()> {
    if children <= remaining {
        Ok(())
    } else {
        Err(Error::new(ErrorKind::LimitExceeded)
            .with_message("legacy Plist projection exceeds the configured object limit"))
    }
}

fn with_container<'document>(
    value: ValueRef<'document>,
    path: &mut Vec<ValueRef<'document>>,
    project_container: impl FnOnce(&mut Vec<ValueRef<'document>>) -> Result<Plist>,
) -> Result<Plist> {
    if path.iter().any(|ancestor| ancestor.is_identical_to(value)) {
        return Err(Error::new(ErrorKind::InvalidReference)
            .with_message("cyclic object graph cannot be represented as Plist"));
    }

    path.push(value);
    let result = project_container(path);
    path.pop();
    result
}

fn system_time(unix_seconds: f64) -> Result<SystemTime> {
    if !unix_seconds.is_finite() {
        return Err(Error::new(ErrorKind::DateOutOfRange));
    }

    let duration = Duration::try_from_secs_f64(unix_seconds.abs())
        .map_err(|_| Error::new(ErrorKind::DateOutOfRange))?;
    let value = if unix_seconds.is_sign_negative() {
        UNIX_EPOCH.checked_sub(duration)
    } else {
        UNIX_EPOCH.checked_add(duration)
    };
    value.ok_or_else(|| Error::new(ErrorKind::DateOutOfRange))
}

fn unsupported(kind: ValueKind) -> Error {
    Error::new(ErrorKind::UnsupportedValue).with_message(format!(
        "{kind:?} cannot be represented by the legacy Plist enum"
    ))
}

fn internal_projection_error() -> Error {
    Error::new(ErrorKind::Internal).with_message("value kind and payload disagree")
}
