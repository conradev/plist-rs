//! Lossless, source-backed property-list documents.

use std::borrow::Cow;
use std::fmt;
use std::ops::Range;

use crate::{BackendKind, Error, ErrorKind, Format, Result};

/// An integer represented by a property list without narrowing it to `i64`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Integer {
    /// A signed integer.
    Signed(i128),
    /// An unsigned integer.
    Unsigned(u128),
}

impl Integer {
    /// Returns this value as `i128` when it fits.
    pub const fn as_i128(self) -> Option<i128> {
        match self {
            Self::Signed(value) => Some(value),
            Self::Unsigned(value) if value <= i128::MAX as u128 => Some(value as i128),
            Self::Unsigned(_) => None,
        }
    }

    /// Returns this value as `u128` when it is nonnegative.
    pub const fn as_u128(self) -> Option<u128> {
        match self {
            Self::Signed(value) if value >= 0 => Some(value as u128),
            Self::Signed(_) => None,
            Self::Unsigned(value) => Some(value),
        }
    }

    /// Returns this value as `i64` when it fits.
    pub fn as_i64(self) -> Option<i64> {
        self.as_i128().and_then(|value| i64::try_from(value).ok())
    }

    /// Returns this value as `u64` when it fits.
    pub fn as_u64(self) -> Option<u64> {
        self.as_u128().and_then(|value| u64::try_from(value).ok())
    }
}

/// A floating-point value retaining its binary property-list width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Real {
    /// An IEEE-754 single-precision value.
    F32(f32),
    /// An IEEE-754 double-precision value.
    F64(f64),
}

impl Real {
    /// Converts this value to `f64`.
    pub const fn as_f64(self) -> f64 {
        match self {
            Self::F32(value) => value as f64,
            Self::F64(value) => value,
        }
    }
}

/// A property-list date expressed as CoreFoundation absolute time.
///
/// The value is seconds since 2001-01-01 00:00:00 UTC. Keeping the original
/// `f64` avoids losing fractional seconds or values that `SystemTime` cannot
/// represent on a particular platform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Date(f64);

impl Date {
    /// The difference between the Unix and CoreFoundation epochs in seconds.
    pub const UNIX_EPOCH_OFFSET: f64 = 978_307_200.0;

    /// Creates a date from CoreFoundation absolute time.
    pub const fn from_cf_absolute_time(seconds: f64) -> Self {
        Self(seconds)
    }

    /// Returns CoreFoundation absolute time in seconds.
    pub const fn cf_absolute_time(self) -> f64 {
        self.0
    }

    /// Returns seconds since the Unix epoch.
    pub fn unix_timestamp(self) -> f64 {
        self.0 + Self::UNIX_EPOCH_OFFSET
    }
}

/// The semantic kind of a lossless property-list value.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum ValueKind {
    /// The CoreFoundation null object.
    Null,
    /// A Boolean.
    Boolean,
    /// A signed or unsigned integer.
    Integer,
    /// A floating-point number.
    Real,
    /// A date.
    Date,
    /// An opaque byte string.
    Data,
    /// A Unicode string, possibly retained as UTF-16 code units.
    String,
    /// A keyed-archiver UID.
    Uid,
    /// An ordered array.
    Array,
    /// An unordered set.
    Set,
    /// A dictionary whose keys may be any primitive accepted by the backend.
    Dictionary,
}

/// A parsed document that borrows its original input.
pub struct Document<'source> {
    source: &'source [u8],
    parsed: Parsed,
}

impl<'source> Document<'source> {
    pub(crate) fn from_parts(source: &'source [u8], parsed: Parsed) -> Self {
        Self { source, parsed }
    }

    /// Returns the original encoded bytes.
    pub const fn source(&self) -> &'source [u8] {
        self.source
    }

    /// Returns the detected or explicitly selected wire format.
    pub const fn format(&self) -> Format {
        self.parsed.format
    }

    /// Returns the backend that parsed this document.
    pub const fn backend(&self) -> BackendKind {
        self.parsed.backend
    }

    /// Returns the document's root value.
    pub fn root(&self) -> ValueRef<'_> {
        ValueRef::new(self.source, &self.parsed, self.parsed.root)
    }

    /// Copies the encoded source once into an independently owned document.
    pub fn into_owned(self) -> OwnedDocument {
        OwnedDocument {
            source: self.source.into(),
            parsed: self.parsed,
        }
    }
}

impl fmt::Debug for Document<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Document")
            .field("format", &self.format())
            .field("backend", &self.backend())
            .field("source_len", &self.source.len())
            .field("objects", &self.parsed.nodes.len())
            .finish_non_exhaustive()
    }
}

/// A parsed document that owns one copy of its encoded input.
pub struct OwnedDocument {
    source: Box<[u8]>,
    parsed: Parsed,
}

impl OwnedDocument {
    pub(crate) fn from_parts(source: Box<[u8]>, parsed: Parsed) -> Self {
        Self { source, parsed }
    }

    /// Returns the owned encoded bytes.
    pub fn source(&self) -> &[u8] {
        &self.source
    }

    /// Returns the detected or explicitly selected wire format.
    pub const fn format(&self) -> Format {
        self.parsed.format
    }

    /// Returns the backend that parsed this document.
    pub const fn backend(&self) -> BackendKind {
        self.parsed.backend
    }

    /// Returns the document's root value.
    pub fn root(&self) -> ValueRef<'_> {
        ValueRef::new(&self.source, &self.parsed, self.parsed.root)
    }

    /// Consumes the document and returns its encoded source bytes.
    pub fn into_source(self) -> Box<[u8]> {
        self.source
    }
}

impl fmt::Debug for OwnedDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OwnedDocument")
            .field("format", &self.format())
            .field("backend", &self.backend())
            .field("source_len", &self.source.len())
            .field("objects", &self.parsed.nodes.len())
            .finish_non_exhaustive()
    }
}

/// A borrowed view of one value in a [`Document`] or [`OwnedDocument`].
#[derive(Clone, Copy)]
pub struct ValueRef<'document> {
    source: &'document [u8],
    parsed: &'document Parsed,
    id: NodeId,
    #[cfg(feature = "serde")]
    materialization_checked: bool,
}

impl<'document> ValueRef<'document> {
    fn new(source: &'document [u8], parsed: &'document Parsed, id: NodeId) -> Self {
        Self {
            source,
            parsed,
            id,
            #[cfg(feature = "serde")]
            materialization_checked: false,
        }
    }

    fn node(self) -> &'document Node {
        // ParsedBuilder validates every edge before a Parsed value is created.
        &self.parsed.nodes[self.id.0]
    }

    pub(crate) const fn format(self) -> Format {
        self.parsed.format
    }

    pub(crate) const fn backend(self) -> BackendKind {
        self.parsed.backend
    }

    #[cfg(feature = "serde")]
    pub(crate) const fn materialization_checked(self) -> bool {
        self.materialization_checked
    }

    #[cfg(feature = "serde-lite")]
    pub(crate) const fn materialization_limit(self) -> usize {
        self.parsed.max_materialized_objects
    }

    #[cfg(feature = "serde")]
    pub(crate) const fn assume_materialization_checked(mut self) -> Self {
        self.materialization_checked = true;
        self
    }

    #[cfg(feature = "serde")]
    pub(crate) fn check_materialization_limit(self, frontend: &'static str) -> Result<Self> {
        if self.materialization_checked {
            return Ok(self);
        }

        let maximum = self.parsed.max_materialized_objects;
        let mut remaining = maximum;
        check_materialized_subtree(self, &mut remaining).map_err(|error| {
            error
                .with_format(self.format())
                .with_backend(self.backend())
                .with_message(format!(
                    "{frontend} materialization exceeds the configured object limit of {maximum}"
                ))
        })?;
        Ok(self.assume_materialization_checked())
    }

    /// Returns this value's semantic kind.
    pub fn kind(self) -> ValueKind {
        self.node().kind()
    }

    /// Returns whether two views refer to the same object-table node.
    pub fn is_identical_to(self, other: Self) -> bool {
        std::ptr::eq(self.parsed, other.parsed) && self.id == other.id
    }

    /// Returns the Boolean value, if this is a Boolean.
    pub fn as_bool(self) -> Option<bool> {
        match self.node() {
            Node::Boolean(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the integer value, if this is an integer.
    pub fn as_integer(self) -> Option<Integer> {
        match self.node() {
            Node::Integer(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the real value, if this is a real.
    pub fn as_real(self) -> Option<Real> {
        match self.node() {
            Node::Real(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the date value, if this is a date.
    pub fn as_date(self) -> Option<Date> {
        match self.node() {
            Node::Date(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the keyed-archiver UID, if this is a UID.
    pub fn as_uid(self) -> Option<u32> {
        match self.node() {
            Node::Uid(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the data payload, if this is a data value.
    pub fn as_data(self) -> Option<&'document [u8]> {
        match self.node() {
            Node::Data(Bytes::Source(range)) => self.source.get(range.clone()),
            Node::Data(Bytes::Owned(bytes)) => Some(bytes),
            _ => None,
        }
    }

    /// Returns a lossless string view, if this is a string.
    pub fn string(self) -> Option<StringRef<'document>> {
        let Node::String(text) = self.node() else {
            return None;
        };

        match text {
            Text::Utf8Source(range) => {
                let bytes = self.source.get(range.clone())?;
                let string = std::str::from_utf8(bytes).ok()?;
                Some(StringRef(StringRepr::Utf8(string)))
            }
            Text::Utf16BeSource(range) => Some(StringRef(StringRepr::Utf16Bytes {
                bytes: self.source.get(range.clone())?,
                endian: Endian::Big,
            })),
            Text::Utf16LeSource(range) => Some(StringRef(StringRepr::Utf16Bytes {
                bytes: self.source.get(range.clone())?,
                endian: Endian::Little,
            })),
            Text::OwnedUtf8(string) => Some(StringRef(StringRepr::Utf8(string))),
            Text::OwnedUtf16(units) => Some(StringRef(StringRepr::Utf16Units(units))),
        }
    }

    /// Returns a directly borrowed UTF-8 string when no transcoding is needed.
    pub fn as_str(self) -> Option<&'document str> {
        self.string()?.as_str()
    }

    /// Returns an array view, if this is an array.
    pub fn as_array(self) -> Option<ArrayRef<'document>> {
        match self.node() {
            Node::Array(values) => Some(ArrayRef::new(self.source, self.parsed, values)),
            _ => None,
        }
    }

    /// Returns a set view, if this is a set.
    pub fn as_set(self) -> Option<SetRef<'document>> {
        match self.node() {
            Node::Set(values) => Some(SetRef::new(self.source, self.parsed, values)),
            _ => None,
        }
    }

    /// Returns a dictionary view, if this is a dictionary.
    pub fn as_dictionary(self) -> Option<DictionaryRef<'document>> {
        match self.node() {
            Node::Dictionary(entries) => {
                Some(DictionaryRef::new(self.source, self.parsed, entries))
            }
            _ => None,
        }
    }
}

impl fmt::Debug for ValueRef<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ValueRef")
            .field("kind", &self.kind())
            .field("object", &self.id.0)
            .finish()
    }
}

#[cfg(feature = "serde")]
fn check_materialized_subtree(value: ValueRef<'_>, remaining: &mut usize) -> Result<()> {
    *remaining = remaining.checked_sub(1).ok_or_else(|| {
        Error::new(ErrorKind::LimitExceeded)
            .with_message("typed materialization exceeds the configured object limit")
    })?;

    match value.kind() {
        ValueKind::Array => {
            for child in value.as_array().expect("kind checked") {
                check_materialized_subtree(child, remaining)?;
            }
        }
        ValueKind::Set => {
            for child in value.as_set().expect("kind checked") {
                check_materialized_subtree(child, remaining)?;
            }
        }
        ValueKind::Dictionary => {
            for (key, child) in value.as_dictionary().expect("kind checked") {
                check_materialized_subtree(key, remaining)?;
                check_materialized_subtree(child, remaining)?;
            }
        }
        ValueKind::Null
        | ValueKind::Boolean
        | ValueKind::Integer
        | ValueKind::Real
        | ValueKind::Date
        | ValueKind::Data
        | ValueKind::String
        | ValueKind::Uid => {}
    }

    Ok(())
}

/// A property-list string that may still be represented as UTF-16 code units.
#[derive(Clone, Copy)]
pub struct StringRef<'document>(StringRepr<'document>);

impl<'document> StringRef<'document> {
    /// Returns a borrowed UTF-8 string when the stored representation permits it.
    pub const fn as_str(self) -> Option<&'document str> {
        match self.0 {
            StringRepr::Utf8(string) => Some(string),
            StringRepr::Utf16Bytes { .. } | StringRepr::Utf16Units(_) => None,
        }
    }

    /// Returns the original UTF-16 code units when the string is stored as UTF-16.
    ///
    /// This preserves unpaired surrogates that cannot be represented by Rust's
    /// `str`. A UTF-8-backed string returns `None` because it has no original
    /// UTF-16 representation.
    pub fn utf16_units(self) -> Option<Utf16Units<'document>> {
        match self.0 {
            StringRepr::Utf8(_) => None,
            StringRepr::Utf16Bytes { bytes, endian } => Some(Utf16Units(Utf16Repr::Bytes {
                bytes,
                endian,
                position: 0,
            })),
            StringRepr::Utf16Units(units) => {
                Some(Utf16Units(Utf16Repr::Units(units.iter().copied())))
            }
        }
    }

    /// Converts this value to a valid Rust string.
    ///
    /// UTF-8 values remain borrowed. UTF-16 values allocate and return an error
    /// if they contain unpaired surrogates.
    pub fn to_string(self) -> Result<Cow<'document, str>> {
        if let Some(string) = self.as_str() {
            return Ok(Cow::Borrowed(string));
        }

        let units: Vec<u16> = self
            .utf16_units()
            .expect("non-UTF-8 strings have UTF-16 units")
            .collect();
        String::from_utf16(&units)
            .map(Cow::Owned)
            .map_err(|source| {
                Error::new(ErrorKind::InvalidString)
                    .with_message(source.to_string())
                    .with_source(source)
            })
    }

    /// Converts this value to a Rust string, replacing unpaired surrogates.
    pub fn to_string_lossy(self) -> Cow<'document, str> {
        if let Some(string) = self.as_str() {
            return Cow::Borrowed(string);
        }

        let units: Vec<u16> = self
            .utf16_units()
            .expect("non-UTF-8 strings have UTF-16 units")
            .collect();
        Cow::Owned(String::from_utf16_lossy(&units))
    }
}

impl fmt::Debug for StringRef<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.as_str() {
            Some(string) => string.fmt(formatter),
            None => formatter
                .debug_tuple("Utf16")
                .field(&self.to_string_lossy())
                .finish(),
        }
    }
}

/// An iterator over original UTF-16 code units.
pub struct Utf16Units<'document>(Utf16Repr<'document>);

impl Iterator for Utf16Units<'_> {
    type Item = u16;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.0 {
            Utf16Repr::Bytes {
                bytes,
                endian,
                position,
            } => {
                let pair = bytes.get(*position..position.checked_add(2)?)?;
                *position += 2;
                Some(match endian {
                    Endian::Big => u16::from_be_bytes([pair[0], pair[1]]),
                    Endian::Little => u16::from_le_bytes([pair[0], pair[1]]),
                })
            }
            Utf16Repr::Units(units) => units.next(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = match &self.0 {
            Utf16Repr::Bytes {
                bytes, position, ..
            } => bytes.len().saturating_sub(*position) / 2,
            Utf16Repr::Units(units) => units.len(),
        };
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for Utf16Units<'_> {}

/// A borrowed view over an array's members.
#[derive(Clone, Copy, Debug)]
pub struct ArrayRef<'document> {
    source: &'document [u8],
    parsed: &'document Parsed,
    values: &'document [NodeId],
}

impl<'document> ArrayRef<'document> {
    fn new(
        source: &'document [u8],
        parsed: &'document Parsed,
        values: &'document [NodeId],
    ) -> Self {
        Self {
            source,
            parsed,
            values,
        }
    }

    /// Returns the number of array members.
    pub const fn len(self) -> usize {
        self.values.len()
    }

    /// Returns whether the array is empty.
    pub const fn is_empty(self) -> bool {
        self.values.is_empty()
    }

    /// Returns one array member by index.
    pub fn get(self, index: usize) -> Option<ValueRef<'document>> {
        self.values
            .get(index)
            .copied()
            .map(|id| ValueRef::new(self.source, self.parsed, id))
    }

    /// Iterates over the array members.
    pub fn iter(self) -> ArrayIter<'document> {
        ArrayIter {
            source: self.source,
            parsed: self.parsed,
            values: self.values.iter(),
        }
    }
}

impl<'document> IntoIterator for ArrayRef<'document> {
    type Item = ValueRef<'document>;
    type IntoIter = ArrayIter<'document>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// An iterator over array members.
pub struct ArrayIter<'document> {
    source: &'document [u8],
    parsed: &'document Parsed,
    values: std::slice::Iter<'document, NodeId>,
}

impl<'document> Iterator for ArrayIter<'document> {
    type Item = ValueRef<'document>;

    fn next(&mut self) -> Option<Self::Item> {
        self.values
            .next()
            .copied()
            .map(|id| ValueRef::new(self.source, self.parsed, id))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.values.size_hint()
    }
}

impl ExactSizeIterator for ArrayIter<'_> {}

/// A borrowed view over a set's members.
#[derive(Clone, Copy, Debug)]
pub struct SetRef<'document> {
    source: &'document [u8],
    parsed: &'document Parsed,
    values: &'document [NodeId],
}

impl<'document> SetRef<'document> {
    fn new(
        source: &'document [u8],
        parsed: &'document Parsed,
        values: &'document [NodeId],
    ) -> Self {
        Self {
            source,
            parsed,
            values,
        }
    }

    /// Returns the number of set members.
    pub const fn len(self) -> usize {
        self.values.len()
    }

    /// Returns whether the set is empty.
    pub const fn is_empty(self) -> bool {
        self.values.is_empty()
    }

    /// Iterates over the set members in backend-defined order.
    pub fn iter(self) -> SetIter<'document> {
        SetIter {
            source: self.source,
            parsed: self.parsed,
            values: self.values.iter(),
        }
    }
}

impl<'document> IntoIterator for SetRef<'document> {
    type Item = ValueRef<'document>;
    type IntoIter = SetIter<'document>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// An iterator over set members.
pub struct SetIter<'document> {
    source: &'document [u8],
    parsed: &'document Parsed,
    values: std::slice::Iter<'document, NodeId>,
}

impl<'document> Iterator for SetIter<'document> {
    type Item = ValueRef<'document>;

    fn next(&mut self) -> Option<Self::Item> {
        self.values
            .next()
            .copied()
            .map(|id| ValueRef::new(self.source, self.parsed, id))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.values.size_hint()
    }
}

impl ExactSizeIterator for SetIter<'_> {}

/// A borrowed view over dictionary entries.
#[derive(Clone, Copy, Debug)]
pub struct DictionaryRef<'document> {
    source: &'document [u8],
    parsed: &'document Parsed,
    entries: &'document [(NodeId, NodeId)],
}

impl<'document> DictionaryRef<'document> {
    fn new(
        source: &'document [u8],
        parsed: &'document Parsed,
        entries: &'document [(NodeId, NodeId)],
    ) -> Self {
        Self {
            source,
            parsed,
            entries,
        }
    }

    /// Returns the number of dictionary entries.
    pub const fn len(self) -> usize {
        self.entries.len()
    }

    /// Returns whether the dictionary is empty.
    pub const fn is_empty(self) -> bool {
        self.entries.is_empty()
    }

    /// Iterates over key-value pairs without requiring string keys.
    pub fn iter(self) -> DictionaryIter<'document> {
        DictionaryIter {
            source: self.source,
            parsed: self.parsed,
            entries: self.entries.iter(),
        }
    }

    /// Looks up a UTF-8 dictionary key.
    pub fn get(self, wanted: &str) -> Option<ValueRef<'document>> {
        self.iter().find_map(|(key, value)| {
            let key = key.string()?.to_string().ok()?;
            (key.as_ref() == wanted).then_some(value)
        })
    }
}

impl<'document> IntoIterator for DictionaryRef<'document> {
    type Item = (ValueRef<'document>, ValueRef<'document>);
    type IntoIter = DictionaryIter<'document>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// An iterator over dictionary key-value pairs.
pub struct DictionaryIter<'document> {
    source: &'document [u8],
    parsed: &'document Parsed,
    entries: std::slice::Iter<'document, (NodeId, NodeId)>,
}

impl<'document> Iterator for DictionaryIter<'document> {
    type Item = (ValueRef<'document>, ValueRef<'document>);

    fn next(&mut self) -> Option<Self::Item> {
        self.entries.next().map(|(key, value)| {
            (
                ValueRef::new(self.source, self.parsed, *key),
                ValueRef::new(self.source, self.parsed, *value),
            )
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.entries.size_hint()
    }
}

impl ExactSizeIterator for DictionaryIter<'_> {}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct NodeId(pub(crate) usize);

#[derive(Debug)]
pub(crate) struct Parsed {
    format: Format,
    backend: BackendKind,
    #[allow(dead_code)] // Read by optional typed frontends.
    max_materialized_objects: usize,
    root: NodeId,
    nodes: Vec<Node>,
}

#[derive(Debug)]
enum Node {
    Null,
    Boolean(bool),
    Integer(Integer),
    Real(Real),
    Date(Date),
    Data(Bytes),
    String(Text),
    Uid(u32),
    Array(Box<[NodeId]>),
    Set(Box<[NodeId]>),
    Dictionary(Box<[(NodeId, NodeId)]>),
}

impl Node {
    const fn kind(&self) -> ValueKind {
        match self {
            Self::Null => ValueKind::Null,
            Self::Boolean(_) => ValueKind::Boolean,
            Self::Integer(_) => ValueKind::Integer,
            Self::Real(_) => ValueKind::Real,
            Self::Date(_) => ValueKind::Date,
            Self::Data(_) => ValueKind::Data,
            Self::String(_) => ValueKind::String,
            Self::Uid(_) => ValueKind::Uid,
            Self::Array(_) => ValueKind::Array,
            Self::Set(_) => ValueKind::Set,
            Self::Dictionary(_) => ValueKind::Dictionary,
        }
    }
}

#[derive(Debug)]
enum Bytes {
    Source(Range<usize>),
    Owned(Box<[u8]>),
}

#[derive(Debug)]
enum Text {
    Utf8Source(Range<usize>),
    Utf16BeSource(Range<usize>),
    Utf16LeSource(Range<usize>),
    OwnedUtf8(Box<str>),
    #[allow(dead_code)] // Reserved for backends that normalize into UTF-16.
    OwnedUtf16(Box<[u16]>),
}

#[derive(Clone, Copy)]
enum StringRepr<'document> {
    Utf8(&'document str),
    Utf16Bytes {
        bytes: &'document [u8],
        endian: Endian,
    },
    Utf16Units(&'document [u16]),
}

enum Utf16Repr<'document> {
    Bytes {
        bytes: &'document [u8],
        endian: Endian,
        position: usize,
    },
    Units(std::iter::Copied<std::slice::Iter<'document, u16>>),
}

#[derive(Clone, Copy)]
enum Endian {
    Big,
    Little,
}

/// Crate-private construction API shared by parser backends.
#[derive(Debug)]
pub(crate) struct ParsedBuilder {
    source_len: usize,
    format: Format,
    backend: BackendKind,
    max_materialized_objects: usize,
    nodes: Vec<Node>,
}

impl ParsedBuilder {
    pub(crate) fn new(
        source_len: usize,
        format: Format,
        backend: BackendKind,
        max_materialized_objects: usize,
    ) -> Self {
        Self {
            source_len,
            format,
            backend,
            max_materialized_objects,
            nodes: Vec::new(),
        }
    }

    fn push(&mut self, node: Node) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(node);
        id
    }

    pub(crate) fn null(&mut self) -> NodeId {
        self.push(Node::Null)
    }

    pub(crate) fn boolean(&mut self, value: bool) -> NodeId {
        self.push(Node::Boolean(value))
    }

    pub(crate) fn integer(&mut self, value: Integer) -> NodeId {
        self.push(Node::Integer(value))
    }

    pub(crate) fn real(&mut self, value: Real) -> NodeId {
        self.push(Node::Real(value))
    }

    pub(crate) fn date(&mut self, value: Date) -> NodeId {
        self.push(Node::Date(value))
    }

    pub(crate) fn uid(&mut self, value: u32) -> NodeId {
        self.push(Node::Uid(value))
    }

    /// Converts a number node using `CFNumberGetValue(...,
    /// kCFNumberSInt32Type, ...)`'s legacy cast semantics.
    ///
    /// The historical XML reader ignores the conversion's loss flag when it
    /// recognizes a `CF$UID` dictionary. Integer values are therefore reduced
    /// to their low 32 bits before that two's-complement bit pattern becomes
    /// the keyed-archiver UID payload.
    pub(crate) fn cf_number_sint32_bits(&self, id: NodeId) -> Option<u32> {
        let value = match self.nodes.get(id.0)? {
            Node::Integer(Integer::Signed(value)) => *value as i32,
            Node::Integer(Integer::Unsigned(value)) => *value as i32,
            Node::Real(value) => {
                // Rust defines this cast for NaN and out-of-range values,
                // avoiding the historical implementation's undefined domain
                // while retaining in-range truncation toward zero.
                value.as_f64() as i32
            }
            _ => return None,
        };
        Some(value as u32)
    }

    pub(crate) fn source_data(&mut self, range: Range<usize>) -> NodeId {
        self.push(Node::Data(Bytes::Source(range)))
    }

    pub(crate) fn owned_data(&mut self, value: Vec<u8>) -> NodeId {
        self.push(Node::Data(Bytes::Owned(value.into_boxed_slice())))
    }

    pub(crate) fn source_string(&mut self, range: Range<usize>) -> NodeId {
        self.push(Node::String(Text::Utf8Source(range)))
    }

    pub(crate) fn source_utf16be_string(&mut self, range: Range<usize>) -> NodeId {
        self.push(Node::String(Text::Utf16BeSource(range)))
    }

    pub(crate) fn source_utf16le_string(&mut self, range: Range<usize>) -> NodeId {
        self.push(Node::String(Text::Utf16LeSource(range)))
    }

    pub(crate) fn owned_string(&mut self, value: String) -> NodeId {
        self.push(Node::String(Text::OwnedUtf8(value.into_boxed_str())))
    }

    #[allow(dead_code)] // Part of the shared backend construction contract.
    pub(crate) fn owned_utf16_string(&mut self, value: Vec<u16>) -> NodeId {
        self.push(Node::String(Text::OwnedUtf16(value.into_boxed_slice())))
    }

    pub(crate) fn array(&mut self, values: Vec<NodeId>) -> NodeId {
        self.push(Node::Array(values.into_boxed_slice()))
    }

    pub(crate) fn set(&mut self, values: Vec<NodeId>) -> NodeId {
        self.push(Node::Set(values.into_boxed_slice()))
    }

    pub(crate) fn dictionary(&mut self, entries: Vec<(NodeId, NodeId)>) -> NodeId {
        self.push(Node::Dictionary(entries.into_boxed_slice()))
    }

    pub(crate) fn finish(self, root: NodeId) -> Result<Parsed> {
        if root.0 >= self.nodes.len() {
            return Err(self.invariant("root object does not exist"));
        }

        for node in &self.nodes {
            match node {
                Node::Data(Bytes::Source(range)) | Node::String(Text::Utf8Source(range)) => {
                    self.validate_range(range)?;
                }
                Node::String(Text::Utf16BeSource(range))
                | Node::String(Text::Utf16LeSource(range)) => {
                    self.validate_range(range)?;
                    if range.len() % 2 != 0 {
                        return Err(self.invariant("UTF-16 source range has an odd length"));
                    }
                }
                Node::Array(values) | Node::Set(values) => {
                    for id in values.iter() {
                        self.validate_id(*id)?;
                    }
                }
                Node::Dictionary(entries) => {
                    for (key, value) in entries.iter() {
                        self.validate_id(*key)?;
                        self.validate_id(*value)?;
                    }
                }
                Node::Null
                | Node::Boolean(_)
                | Node::Integer(_)
                | Node::Real(_)
                | Node::Date(_)
                | Node::Data(Bytes::Owned(_))
                | Node::String(Text::OwnedUtf8(_) | Text::OwnedUtf16(_))
                | Node::Uid(_) => {}
            }
        }

        Ok(Parsed {
            format: self.format,
            backend: self.backend,
            max_materialized_objects: self.max_materialized_objects,
            root,
            nodes: self.nodes,
        })
    }

    fn validate_id(&self, id: NodeId) -> Result<()> {
        if id.0 < self.nodes.len() {
            Ok(())
        } else {
            Err(self.invariant("container references an object that does not exist"))
        }
    }

    fn validate_range(&self, range: &Range<usize>) -> Result<()> {
        if range.start <= range.end && range.end <= self.source_len {
            Ok(())
        } else {
            Err(self.invariant("payload range is outside the source input"))
        }
    }

    fn invariant(&self, message: &'static str) -> Error {
        Error::new(ErrorKind::Internal)
            .with_format(self.format)
            .with_backend(self.backend)
            .with_message(message)
    }
}
