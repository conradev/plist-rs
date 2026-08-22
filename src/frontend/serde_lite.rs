//! `serde-lite` support over the lossless document model.
//!
//! `serde-lite` deliberately uses an owned, JSON-shaped intermediate value.
//! Consequently this frontend is neither zero-copy nor lossless: property-list
//! data, dates, UIDs, sets, wide integers, and non-string dictionary keys are
//! rejected instead of being silently coerced.

use ::serde_lite::{Deserialize, Intermediate, Map, Number};

use crate::{
    Document, Error, ErrorKind, Integer, OwnedDocument, ParseOptions, Parser, Real, Result,
    ValueKind, ValueRef,
};

impl<'source> Document<'source> {
    /// Converts the root to `serde-lite`'s owned intermediate representation.
    ///
    /// This conversion allocates. Values outside `serde-lite`'s JSON-like data
    /// model return [`ErrorKind::UnsupportedValue`] or
    /// [`ErrorKind::IntegerOutOfRange`].
    pub fn to_serde_lite_intermediate(&self) -> Result<Intermediate> {
        to_intermediate(self.root())
    }

    /// Deserializes the root through `serde-lite`'s owned intermediate value.
    pub fn deserialize_lite<T>(&self) -> Result<T>
    where
        T: Deserialize,
    {
        deserialize_value(self.root())
    }
}

impl OwnedDocument {
    /// Converts the root to `serde-lite`'s owned intermediate representation.
    ///
    /// This conversion allocates. Values outside `serde-lite`'s JSON-like data
    /// model return [`ErrorKind::UnsupportedValue`] or
    /// [`ErrorKind::IntegerOutOfRange`].
    pub fn to_serde_lite_intermediate(&self) -> Result<Intermediate> {
        to_intermediate(self.root())
    }

    /// Deserializes the root through `serde-lite`'s owned intermediate value.
    pub fn deserialize_lite<T>(&self) -> Result<T>
    where
        T: Deserialize,
    {
        deserialize_value(self.root())
    }
}

impl Parser {
    /// Parses and deserializes a value using the `serde-lite` frontend.
    pub fn deserialize_lite<T>(&self, source: &[u8]) -> Result<T>
    where
        T: Deserialize,
    {
        self.parse(source)?.deserialize_lite()
    }
}

/// Parses and deserializes a value using `serde-lite` and the default parser.
pub fn from_slice_lite<T>(source: &[u8]) -> Result<T>
where
    T: Deserialize,
{
    Parser::new().deserialize_lite(source)
}

/// Parses and deserializes a value using `serde-lite` and explicit options.
pub fn from_slice_lite_with_options<T>(source: &[u8], options: ParseOptions) -> Result<T>
where
    T: Deserialize,
{
    Parser::from_options(options).deserialize_lite(source)
}

fn deserialize_value<T>(value: ValueRef<'_>) -> Result<T>
where
    T: Deserialize,
{
    let intermediate = to_intermediate(value)?;
    T::deserialize(&intermediate).map_err(|source| {
        Error::new(ErrorKind::UnsupportedValue)
            .with_format(value.format())
            .with_backend(value.backend())
            .with_message(format!("serde-lite deserialization failed: {source}"))
            .with_source(source)
    })
}

fn to_intermediate(value: ValueRef<'_>) -> Result<Intermediate> {
    let maximum = value.materialization_limit();
    let mut remaining = maximum;
    to_intermediate_with_budget(value, &mut remaining).map_err(|error| {
        let error = if error.kind() == ErrorKind::LimitExceeded {
            error.with_message(format!(
                "serde-lite materialization exceeds the configured object limit of {maximum}"
            ))
        } else {
            error
        };
        error
            .with_format(value.format())
            .with_backend(value.backend())
    })
}

fn to_intermediate_with_budget(value: ValueRef<'_>, remaining: &mut usize) -> Result<Intermediate> {
    *remaining = remaining.checked_sub(1).ok_or_else(|| {
        Error::new(ErrorKind::LimitExceeded)
            .with_message("serde-lite materialization exceeds the configured object limit")
    })?;

    match value.kind() {
        ValueKind::Null => Ok(Intermediate::None),
        ValueKind::Boolean => Ok(Intermediate::Bool(value.as_bool().expect("kind checked"))),
        ValueKind::Integer => integer(value.as_integer().expect("kind checked")),
        ValueKind::Real => Ok(Intermediate::Number(
            match value.as_real().expect("kind checked") {
                Real::F32(number) => Number::Float(f64::from(number)),
                Real::F64(number) => Number::Float(number),
            },
        )),
        ValueKind::String => {
            let string = value
                .string()
                .expect("kind checked")
                .to_string()?
                .into_owned();
            Ok(Intermediate::String(string.into()))
        }
        ValueKind::Array => {
            let array = value.as_array().expect("kind checked");
            ensure_direct_children(array.len(), *remaining)?;
            let mut output = Vec::with_capacity(array.len());
            for item in array {
                output.push(to_intermediate_with_budget(item, remaining)?);
            }
            Ok(Intermediate::Array(output))
        }
        ValueKind::Dictionary => {
            let dictionary = value.as_dictionary().expect("kind checked");
            let direct_children = dictionary.len().checked_mul(2).ok_or_else(limit_exceeded)?;
            ensure_direct_children(direct_children, *remaining)?;
            let mut output = Map::with_capacity(dictionary.len());
            for (key, item) in dictionary {
                consume_object(remaining)?;
                let key = key
                    .string()
                    .ok_or_else(|| unsupported("serde-lite requires string dictionary keys"))?
                    .to_string()?
                    .into_owned();
                output.insert_with_owned_key(key, to_intermediate_with_budget(item, remaining)?);
            }
            Ok(Intermediate::Map(output))
        }
        ValueKind::Data => Err(unsupported(
            "serde-lite cannot represent property-list data",
        )),
        ValueKind::Date => Err(unsupported(
            "serde-lite cannot represent property-list dates",
        )),
        ValueKind::Uid => Err(unsupported(
            "serde-lite cannot represent property-list UIDs",
        )),
        ValueKind::Set => Err(unsupported(
            "serde-lite cannot distinguish sets from arrays",
        )),
    }
}

fn ensure_direct_children(children: usize, remaining: usize) -> Result<()> {
    if children <= remaining {
        Ok(())
    } else {
        Err(limit_exceeded())
    }
}

fn consume_object(remaining: &mut usize) -> Result<()> {
    *remaining = remaining.checked_sub(1).ok_or_else(limit_exceeded)?;
    Ok(())
}

fn limit_exceeded() -> Error {
    Error::new(ErrorKind::LimitExceeded)
        .with_message("serde-lite materialization exceeds the configured object limit")
}

fn integer(value: Integer) -> Result<Intermediate> {
    let number = match value {
        Integer::Signed(value) => Number::SignedInt(i64::try_from(value).map_err(|_| {
            Error::new(ErrorKind::IntegerOutOfRange)
                .with_message("serde-lite only represents signed integers through i64")
        })?),
        Integer::Unsigned(value) => Number::UnsignedInt(u64::try_from(value).map_err(|_| {
            Error::new(ErrorKind::IntegerOutOfRange)
                .with_message("serde-lite only represents unsigned integers through u64")
        })?),
    };
    Ok(Intermediate::Number(number))
}

fn unsupported(message: &'static str) -> Error {
    Error::new(ErrorKind::UnsupportedValue).with_message(message)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn deserializes_json_shaped_property_lists() {
        let value: HashMap<String, i64> = from_slice_lite(
            br#"<?xml version="1.0"?><plist version="1.0"><dict><key>answer</key><integer>42</integer></dict></plist>"#,
        )
        .expect("serde-lite value");
        assert_eq!(value.get("answer"), Some(&42));
    }

    #[test]
    fn rejects_lossy_data_coercion() {
        let error = from_slice_lite::<Vec<u8>>(
            br#"<?xml version="1.0"?><plist version="1.0"><data>AQID</data></plist>"#,
        )
        .expect_err("data is outside serde-lite's intermediate model");
        assert_eq!(error.kind(), ErrorKind::UnsupportedValue);
    }
}
