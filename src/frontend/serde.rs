//! Serde support over [`ValueRef`](crate::ValueRef).

use std::borrow::Cow;
use std::fmt;

use ::serde::de::{
    self,
    value::{StrDeserializer, U8Deserializer},
    DeserializeOwned, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor,
};
use ::serde::ser::{SerializeMap, SerializeSeq};
use ::serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::document::{ArrayIter, DictionaryIter, Integer, Real, SetIter};
#[cfg(feature = "legacy-api")]
use crate::Plist;
use crate::{
    Document, Error, ErrorKind, OwnedDocument, ParseOptions, Parser, Result, ValueKind, ValueRef,
};

const DATE_NEWTYPE: &str = "$plist::Date";
const UID_NEWTYPE: &str = "$plist::Uid";
const SET_NEWTYPE: &str = "$plist::Set";

impl de::Error for Error {
    fn custom<T>(message: T) -> Self
    where
        T: fmt::Display,
    {
        Error::new(ErrorKind::UnsupportedValue).with_message(message.to_string())
    }
}

impl<'source> Document<'source> {
    /// Deserializes a typed value while allowing fields to borrow this document.
    pub fn deserialize<'document, T>(&'document self) -> Result<T>
    where
        T: Deserialize<'document>,
    {
        deserialize_value(self.root())
    }
}

impl OwnedDocument {
    /// Deserializes a typed value while allowing fields to borrow this document.
    pub fn deserialize<'document, T>(&'document self) -> Result<T>
    where
        T: Deserialize<'document>,
    {
        deserialize_value(self.root())
    }
}

impl Parser {
    /// Parses and deserializes an owned value using the Serde frontend.
    ///
    /// Parse a [`Document`] first and call [`Document::deserialize`] when the
    /// target contains fields borrowed from the input.
    pub fn deserialize<T>(&self, source: &[u8]) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let document = self.parse(source)?;
        document.deserialize()
    }
}

/// Deserializes an owned value using the default parser.
///
/// Use [`Document::deserialize`] when the target contains borrowed fields.
pub fn from_slice<T>(source: &[u8]) -> Result<T>
where
    T: DeserializeOwned,
{
    from_slice_with_options(source, ParseOptions::default())
}

/// Deserializes an owned value using explicit parser options.
///
/// Use [`Document::deserialize`] when the target contains borrowed fields.
pub fn from_slice_with_options<T>(source: &[u8], options: ParseOptions) -> Result<T>
where
    T: DeserializeOwned,
{
    Parser::from_options(options).deserialize(source)
}

fn mismatch(value: ValueRef<'_>, expected: &'static str) -> Error {
    Error::new(ErrorKind::UnsupportedValue)
        .with_format(value.format())
        .with_backend(value.backend())
        .with_message(format!("expected {expected}, found {:?}", value.kind()))
}

fn deserialize_value<'de, T>(value: ValueRef<'de>) -> Result<T>
where
    T: Deserialize<'de>,
{
    T::deserialize(value).map_err(|error| {
        error
            .with_format(value.format())
            .with_backend(value.backend())
    })
}

impl<'de> Deserializer<'de> for ValueRef<'de> {
    type Error = Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.kind() {
            ValueKind::Null => visitor.visit_unit(),
            ValueKind::Boolean => visitor.visit_bool(self.as_bool().expect("kind checked")),
            ValueKind::Integer => visit_integer(self.as_integer().expect("kind checked"), visitor),
            ValueKind::Real => visit_real(self.as_real().expect("kind checked"), visitor),
            ValueKind::Date => {
                visitor.visit_f64(self.as_date().expect("kind checked").cf_absolute_time())
            }
            ValueKind::Data => visitor.visit_borrowed_bytes(self.as_data().expect("kind checked")),
            ValueKind::String => visit_string(self, visitor),
            ValueKind::Uid => visitor.visit_u32(self.as_uid().expect("kind checked")),
            ValueKind::Array => {
                let value = self.check_materialization_limit("Serde")?;
                visitor.visit_seq(Values::array(value))
            }
            ValueKind::Set => {
                let value = self.check_materialization_limit("Serde")?;
                visitor.visit_seq(Values::set(value))
            }
            ValueKind::Dictionary => {
                let value = self.check_materialization_limit("Serde")?;
                visitor.visit_map(Entries::new(value))
            }
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.as_bool()
            .ok_or_else(|| mismatch(self, "a Boolean"))
            .and_then(|value| visitor.visit_bool(value))
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visit_signed(
            self,
            visitor,
            "i8",
            |value| i8::try_from(value).ok(),
            Visitor::visit_i8,
        )
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visit_signed(
            self,
            visitor,
            "i16",
            |value| i16::try_from(value).ok(),
            Visitor::visit_i16,
        )
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visit_signed(
            self,
            visitor,
            "i32",
            |value| i32::try_from(value).ok(),
            Visitor::visit_i32,
        )
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visit_signed(
            self,
            visitor,
            "i64",
            |value| i64::try_from(value).ok(),
            Visitor::visit_i64,
        )
    }

    fn deserialize_i128<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let integer = self
            .as_integer()
            .ok_or_else(|| mismatch(self, "a signed integer"))?;
        let value = integer
            .as_i128()
            .ok_or_else(|| integer_out_of_range(self, "i128"))?;
        visitor.visit_i128(value)
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visit_unsigned(
            self,
            visitor,
            "u8",
            |value| u8::try_from(value).ok(),
            Visitor::visit_u8,
        )
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visit_unsigned(
            self,
            visitor,
            "u16",
            |value| u16::try_from(value).ok(),
            Visitor::visit_u16,
        )
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        if let Some(uid) = self.as_uid() {
            return visitor.visit_u32(uid);
        }
        visit_unsigned(
            self,
            visitor,
            "u32",
            |value| u32::try_from(value).ok(),
            Visitor::visit_u32,
        )
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        if let Some(uid) = self.as_uid() {
            return visitor.visit_u64(u64::from(uid));
        }
        visit_unsigned(
            self,
            visitor,
            "u64",
            |value| u64::try_from(value).ok(),
            Visitor::visit_u64,
        )
    }

    fn deserialize_u128<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        if let Some(uid) = self.as_uid() {
            return visitor.visit_u128(u128::from(uid));
        }
        let integer = self
            .as_integer()
            .ok_or_else(|| mismatch(self, "an unsigned integer"))?;
        let value = integer
            .as_u128()
            .ok_or_else(|| integer_out_of_range(self, "u128"))?;
        visitor.visit_u128(value)
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let value = self
            .as_real()
            .ok_or_else(|| mismatch(self, "a real number"))?
            .as_f64() as f32;
        visitor.visit_f32(value)
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        if let Some(date) = self.as_date() {
            return visitor.visit_f64(date.cf_absolute_time());
        }
        let value = self
            .as_real()
            .ok_or_else(|| mismatch(self, "a real number"))?
            .as_f64();
        visitor.visit_f64(value)
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let value = self
            .string()
            .ok_or_else(|| mismatch(self, "a character"))?
            .to_string()?;
        let mut chars = value.chars();
        let character = chars
            .next()
            .filter(|_| chars.next().is_none())
            .ok_or_else(|| <Error as de::Error>::custom("expected one Unicode scalar value"))?;
        visitor.visit_char(character)
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visit_string(self, visitor)
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visit_string(self, visitor)
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.as_data()
            .ok_or_else(|| mismatch(self, "data"))
            .and_then(|value| visitor.visit_borrowed_bytes(value))
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        if self.kind() == ValueKind::Null {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        if self.kind() == ValueKind::Null {
            visitor.visit_unit()
        } else {
            Err(mismatch(self, "null"))
        }
    }

    fn deserialize_unit_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V>(self, name: &'static str, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match name {
            DATE_NEWTYPE if self.kind() != ValueKind::Date => Err(mismatch(self, "a date")),
            UID_NEWTYPE if self.kind() != ValueKind::Uid => Err(mismatch(self, "a UID")),
            SET_NEWTYPE if self.kind() != ValueKind::Set => Err(mismatch(self, "a set")),
            _ => visitor.visit_newtype_struct(self),
        }
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.kind() {
            ValueKind::Array => {
                let value = self.check_materialization_limit("Serde")?;
                visitor.visit_seq(Values::array(value))
            }
            ValueKind::Set => {
                let value = self.check_materialization_limit("Serde")?;
                visitor.visit_seq(Values::set(value))
            }
            ValueKind::Data => visitor.visit_seq(Bytes::new(self)),
            _ => Err(mismatch(self, "an array, set, or data value")),
        }
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        if self.kind() == ValueKind::Dictionary {
            let value = self.check_materialization_limit("Serde")?;
            visitor.visit_map(Entries::new(value))
        } else {
            Err(mismatch(self, "a dictionary"))
        }
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        if let Some(string) = self.string() {
            return visitor.visit_enum(EnumValue {
                variant: string.to_string()?,
                value: None,
            });
        }

        let value = self.check_materialization_limit("Serde")?;
        let dictionary = value
            .as_dictionary()
            .filter(|dictionary| dictionary.len() == 1)
            .ok_or_else(|| mismatch(value, "a string or single-entry enum dictionary"))?;
        let (key, value) = dictionary.iter().next().expect("length checked");
        let variant = key
            .string()
            .ok_or_else(|| mismatch(key, "a string enum variant"))?
            .to_string()?;
        visitor.visit_enum(EnumValue {
            variant,
            value: Some(value.assume_materialization_checked()),
        })
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }
}

fn visit_integer<'de, V>(value: Integer, visitor: V) -> Result<V::Value>
where
    V: Visitor<'de>,
{
    match value {
        Integer::Signed(value) => visitor.visit_i128(value),
        Integer::Unsigned(value) => visitor.visit_u128(value),
    }
}

fn visit_real<'de, V>(value: Real, visitor: V) -> Result<V::Value>
where
    V: Visitor<'de>,
{
    match value {
        Real::F32(value) => visitor.visit_f32(value),
        Real::F64(value) => visitor.visit_f64(value),
    }
}

fn visit_string<'de, V>(value: ValueRef<'de>, visitor: V) -> Result<V::Value>
where
    V: Visitor<'de>,
{
    let string = value
        .string()
        .ok_or_else(|| mismatch(value, "a string"))?
        .to_string()?;
    match string {
        Cow::Borrowed(value) => visitor.visit_borrowed_str(value),
        Cow::Owned(value) => visitor.visit_string(value),
    }
}

fn visit_signed<'de, V, T, Convert, Visit>(
    input: ValueRef<'de>,
    visitor: V,
    target: &'static str,
    convert: Convert,
    visit: Visit,
) -> Result<V::Value>
where
    V: Visitor<'de>,
    Convert: FnOnce(i128) -> Option<T>,
    Visit: FnOnce(V, T) -> Result<V::Value>,
{
    let integer = input
        .as_integer()
        .ok_or_else(|| mismatch(input, "an integer"))?;
    let value = integer
        .as_i128()
        .and_then(convert)
        .ok_or_else(|| integer_out_of_range(input, target))?;
    visit(visitor, value)
}

fn visit_unsigned<'de, V, T, Convert, Visit>(
    input: ValueRef<'de>,
    visitor: V,
    target: &'static str,
    convert: Convert,
    visit: Visit,
) -> Result<V::Value>
where
    V: Visitor<'de>,
    Convert: FnOnce(u128) -> Option<T>,
    Visit: FnOnce(V, T) -> Result<V::Value>,
{
    let integer = input
        .as_integer()
        .ok_or_else(|| mismatch(input, "an integer"))?;
    let value = integer
        .as_u128()
        .and_then(convert)
        .ok_or_else(|| integer_out_of_range(input, target))?;
    visit(visitor, value)
}

fn integer_out_of_range(value: ValueRef<'_>, target: &'static str) -> Error {
    Error::new(ErrorKind::IntegerOutOfRange)
        .with_format(value.format())
        .with_backend(value.backend())
        .with_message(format!("integer cannot be represented as {target}"))
}

enum Values<'de> {
    Array(ArrayIter<'de>, bool),
    Set(SetIter<'de>, bool),
}

impl<'de> Values<'de> {
    fn array(value: ValueRef<'de>) -> Self {
        Self::Array(
            value.as_array().expect("kind checked").iter(),
            value.materialization_checked(),
        )
    }

    fn set(value: ValueRef<'de>) -> Self {
        Self::Set(
            value.as_set().expect("kind checked").iter(),
            value.materialization_checked(),
        )
    }

    fn next(&mut self) -> Option<ValueRef<'de>> {
        let (value, checked) = match self {
            Self::Array(values, checked) => (values.next(), *checked),
            Self::Set(values, checked) => (values.next(), *checked),
        };
        value.map(|value| {
            if checked {
                value.assume_materialization_checked()
            } else {
                value
            }
        })
    }

    fn remaining(&self) -> usize {
        match self {
            Self::Array(values, _) => values.len(),
            Self::Set(values, _) => values.len(),
        }
    }
}

impl<'de> SeqAccess<'de> for Values<'de> {
    type Error = Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>>
    where
        T: DeserializeSeed<'de>,
    {
        self.next().map(|value| seed.deserialize(value)).transpose()
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.remaining())
    }
}

struct Bytes<'de> {
    values: std::slice::Iter<'de, u8>,
}

impl<'de> Bytes<'de> {
    fn new(value: ValueRef<'de>) -> Self {
        Self {
            values: value.as_data().expect("kind checked").iter(),
        }
    }
}

impl<'de> SeqAccess<'de> for Bytes<'de> {
    type Error = Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>>
    where
        T: DeserializeSeed<'de>,
    {
        self.values
            .next()
            .copied()
            .map(|value| seed.deserialize(U8Deserializer::<Error>::new(value)))
            .transpose()
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.values.len())
    }
}

struct Entries<'de> {
    entries: DictionaryIter<'de>,
    value: Option<ValueRef<'de>>,
    materialization_checked: bool,
}

impl<'de> Entries<'de> {
    fn new(value: ValueRef<'de>) -> Self {
        Self {
            entries: value.as_dictionary().expect("kind checked").iter(),
            value: None,
            materialization_checked: value.materialization_checked(),
        }
    }
}

impl<'de> MapAccess<'de> for Entries<'de> {
    type Error = Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>>
    where
        K: DeserializeSeed<'de>,
    {
        let Some((key, value)) = self.entries.next() else {
            return Ok(None);
        };
        if self.materialization_checked {
            self.value = Some(value.assume_materialization_checked());
            seed.deserialize(key.assume_materialization_checked())
                .map(Some)
        } else {
            self.value = Some(value);
            seed.deserialize(key).map(Some)
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value>
    where
        V: DeserializeSeed<'de>,
    {
        let value = self
            .value
            .take()
            .ok_or_else(|| <Error as de::Error>::custom("dictionary value requested before key"))?;
        seed.deserialize(value)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.entries.len())
    }
}

struct EnumValue<'de> {
    variant: Cow<'de, str>,
    value: Option<ValueRef<'de>>,
}

impl<'de> EnumAccess<'de> for EnumValue<'de> {
    type Error = Error;
    type Variant = Self;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant)>
    where
        V: DeserializeSeed<'de>,
    {
        let variant = match &self.variant {
            Cow::Borrowed(value) => seed.deserialize(StrDeserializer::<Error>::new(value))?,
            Cow::Owned(value) => seed.deserialize(StrDeserializer::<Error>::new(value.as_str()))?,
        };
        Ok((variant, self))
    }
}

impl<'de> VariantAccess<'de> for EnumValue<'de> {
    type Error = Error;

    fn unit_variant(self) -> Result<()> {
        match self.value {
            None => Ok(()),
            Some(value) if value.kind() == ValueKind::Null => Ok(()),
            Some(value) => Err(mismatch(value, "a unit enum variant")),
        }
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value>
    where
        T: DeserializeSeed<'de>,
    {
        seed.deserialize(
            self.value
                .ok_or_else(|| <Error as de::Error>::custom("enum variant has no value"))?,
        )
    }

    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.value
            .ok_or_else(|| <Error as de::Error>::custom("enum variant has no value"))?
            .deserialize_seq(visitor)
    }

    fn struct_variant<V>(self, _fields: &'static [&'static str], visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.value
            .ok_or_else(|| <Error as de::Error>::custom("enum variant has no value"))?
            .deserialize_map(visitor)
    }
}

impl Serialize for ValueRef<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let input = (*self)
            .check_materialization_limit("Serde serialization")
            .map_err(::serde::ser::Error::custom)?;
        match input.kind() {
            ValueKind::Null => serializer.serialize_unit(),
            ValueKind::Boolean => serializer.serialize_bool(input.as_bool().expect("kind checked")),
            ValueKind::Integer => match input.as_integer().expect("kind checked") {
                Integer::Signed(value) => match i64::try_from(value) {
                    Ok(value) => serializer.serialize_i64(value),
                    Err(_) => serializer.serialize_i128(value),
                },
                Integer::Unsigned(value) => match u64::try_from(value) {
                    Ok(value) => serializer.serialize_u64(value),
                    Err(_) => serializer.serialize_u128(value),
                },
            },
            ValueKind::Real => match input.as_real().expect("kind checked") {
                Real::F32(value) => serializer.serialize_f32(value),
                Real::F64(value) => serializer.serialize_f64(value),
            },
            ValueKind::Date => serializer.serialize_newtype_struct(
                DATE_NEWTYPE,
                &input.as_date().expect("kind checked").cf_absolute_time(),
            ),
            ValueKind::Data => serializer.serialize_bytes(input.as_data().expect("kind checked")),
            ValueKind::String => input
                .string()
                .expect("kind checked")
                .to_string()
                .map_err(::serde::ser::Error::custom)?
                .serialize(serializer),
            ValueKind::Uid => serializer
                .serialize_newtype_struct(UID_NEWTYPE, &input.as_uid().expect("kind checked")),
            ValueKind::Array => {
                let array = input.as_array().expect("kind checked");
                let mut sequence = serializer.serialize_seq(Some(array.len()))?;
                for value in array {
                    sequence.serialize_element(&value.assume_materialization_checked())?;
                }
                sequence.end()
            }
            ValueKind::Set => serializer.serialize_newtype_struct(SET_NEWTYPE, &SetValue(input)),
            ValueKind::Dictionary => {
                let dictionary = input.as_dictionary().expect("kind checked");
                let mut map = serializer.serialize_map(Some(dictionary.len()))?;
                for (key, value) in dictionary {
                    map.serialize_entry(
                        &key.assume_materialization_checked(),
                        &value.assume_materialization_checked(),
                    )?;
                }
                map.end()
            }
        }
    }
}

struct SetValue<'document>(ValueRef<'document>);

impl Serialize for SetValue<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let set = self.0.as_set().expect("set wrapper only contains a set");
        let mut sequence = serializer.serialize_seq(Some(set.len()))?;
        for value in set {
            sequence.serialize_element(&value.assume_materialization_checked())?;
        }
        sequence.end()
    }
}

#[cfg(feature = "legacy-api")]
impl Serialize for Plist {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Plist::Array(values) => values.serialize(serializer),
            Plist::Dict(values) => values.serialize(serializer),
            Plist::Boolean(value) => serializer.serialize_bool(*value),
            Plist::Data(value) => serializer.serialize_bytes(value),
            Plist::DateTime(value) => {
                let unix = match value.duration_since(std::time::UNIX_EPOCH) {
                    Ok(value) => value.as_secs_f64(),
                    Err(value) => -value.duration().as_secs_f64(),
                };
                serializer.serialize_newtype_struct(
                    DATE_NEWTYPE,
                    &(unix - crate::Date::UNIX_EPOCH_OFFSET),
                )
            }
            Plist::Real(value) => serializer.serialize_f64(*value),
            Plist::Integer(value) => serializer.serialize_i64(*value),
            Plist::String(value) => serializer.serialize_str(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_deserializes_borrowed_xml_strings() {
        let source =
            br#"<?xml version="1.0"?><plist version="1.0"><string>borrowed</string></plist>"#;
        let document = Parser::new().parse(source).expect("XML document");
        let value: &str = document.deserialize().expect("borrowed string");
        assert_eq!(value, "borrowed");
        assert!(value.as_ptr() >= source.as_ptr());
        assert!(value.as_ptr() < source[source.len()..].as_ptr());
    }

    #[test]
    fn from_slice_deserializes_owned_collections() {
        let source = br#"<?xml version="1.0"?><plist version="1.0"><array><integer>1</integer><integer>2</integer></array></plist>"#;
        let value: Vec<i64> = from_slice(source).expect("owned vector");
        assert_eq!(value, [1, 2]);
    }

    #[test]
    fn data_deserializes_into_a_byte_vector() {
        let source = br#"<?xml version="1.0"?><plist version="1.0"><data>AQID</data></plist>"#;
        let value: Vec<u8> = from_slice(source).expect("byte vector");
        assert_eq!(value, [1, 2, 3]);
    }

    #[test]
    fn narrow_integer_failures_use_the_range_error_category() {
        let too_large = br#"<plist version="1.0"><integer>256</integer></plist>"#;
        let error = Parser::new().deserialize::<u8>(too_large).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::IntegerOutOfRange);

        let negative = br#"<plist version="1.0"><integer>-1</integer></plist>"#;
        let error = Parser::new().deserialize::<u8>(negative).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::IntegerOutOfRange);

        let wrong_kind = br#"<plist version="1.0"><string>1</string></plist>"#;
        let error = Parser::new().deserialize::<u8>(wrong_kind).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::UnsupportedValue);
    }
}
