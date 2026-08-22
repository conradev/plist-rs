//! Safe binary-property-list decoder with selectable compatibility policy.
//!
//! This is a new safe Rust implementation of an observable behavior contract.
//! Its historical reference is [`opensource-apple/CF`'s `CFBinaryPList.c`][apple-c]
//! at commit `3cc41a76b1491f50813e28a4ec09954ffa359e6f` (SHA-256
//! `8b8c72427ba60e7918f3ffa088d9f25859456eb75a0081ff5cdb7277ff62c384`,
//! APSL-2.0). Bounds, depth handling, and malformed-input behavior were also
//! checked against Swift Corelibs Foundation commit
//! `761b621da93a856a48995efc29ed11028c283306`. No C source is copied or
//! vendored; keeping both pins here makes compatibility changes deliberate.
//!
//! [apple-c]: https://github.com/opensource-apple/CF/blob/3cc41a76b1491f50813e28a4ec09954ffa359e6f/CFBinaryPList.c

use std::borrow::Cow;
use std::collections::hash_map::RandomState;
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::hash::{BuildHasher, Hash, Hasher};
use std::marker::PhantomData;
use std::ops::Range;

use crate::document::{NodeId, Parsed, ParsedBuilder};
use crate::{BackendKind, Date, Error, ErrorKind, Format, Integer, ParseOptions, Real, Result};

const HEADER_LEN: usize = 8;
const TRAILER_LEN: usize = 32;

#[cfg(any(
    target_os = "android",
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos"
))]
const CF_MAX_DEPTH: usize = 128;

#[cfg(not(any(
    target_os = "android",
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos"
)))]
const CF_MAX_DEPTH: usize = 512;

/// Returns whether `source` has the permissive Core Foundation binary header.
#[cfg(feature = "backend-cf-compat")]
pub(crate) fn is_binary_cf(source: &[u8]) -> bool {
    source.get(..7) == Some(&b"bplist0"[..])
}

/// Returns whether `source` has the canonical public binary-plist header.
#[cfg(feature = "backend-pure")]
pub(crate) fn is_binary_pure(source: &[u8]) -> bool {
    source.get(..HEADER_LEN) == Some(&b"bplist00"[..])
}

/// Decodes using the pinned Core Foundation compatibility policy.
#[cfg(feature = "backend-cf-compat")]
pub(crate) fn parse_cf(source: &[u8], options: &ParseOptions) -> Result<Parsed> {
    Decoder::<CoreFoundationProfile>::parse(source, options)
        .map_err(|error| error.with_backend(BackendKind::CoreFoundation))
}

/// Decodes using the stricter, public-format pure-Rust policy.
#[cfg(feature = "backend-pure")]
pub(crate) fn parse_pure(source: &[u8], options: &ParseOptions) -> Result<Parsed> {
    Decoder::<PureProfile>::parse(source, options)
        .map_err(|error| error.with_backend(BackendKind::Pure))
}

trait Profile {
    fn backend() -> BackendKind;
    fn valid_header(source: &[u8]) -> bool;
    fn valid_table_width(width: usize) -> bool;
    fn valid_count_width(width: usize) -> bool;
    fn permits_null() -> bool;
    fn permits_uid() -> bool;
    fn permits_set() -> bool;
    fn permits_dictionary_key(node: &SemanticNode) -> bool;
}

#[cfg(feature = "backend-cf-compat")]
struct CoreFoundationProfile;

#[cfg(feature = "backend-cf-compat")]
impl Profile for CoreFoundationProfile {
    #[inline]
    fn backend() -> BackendKind {
        BackendKind::CoreFoundation
    }

    #[inline]
    fn valid_header(source: &[u8]) -> bool {
        is_binary_cf(source)
    }

    #[inline]
    fn valid_table_width(width: usize) -> bool {
        width != 0
    }

    #[inline]
    fn valid_count_width(_width: usize) -> bool {
        true
    }

    #[inline]
    fn permits_null() -> bool {
        true
    }

    #[inline]
    fn permits_uid() -> bool {
        true
    }

    #[inline]
    fn permits_set() -> bool {
        true
    }

    #[inline]
    fn permits_dictionary_key(node: &SemanticNode) -> bool {
        node.is_primitive()
    }
}

#[cfg(feature = "backend-pure")]
struct PureProfile;

#[cfg(feature = "backend-pure")]
impl Profile for PureProfile {
    #[inline]
    fn backend() -> BackendKind {
        BackendKind::Pure
    }

    #[inline]
    fn valid_header(source: &[u8]) -> bool {
        is_binary_pure(source)
    }

    #[inline]
    fn valid_table_width(width: usize) -> bool {
        matches!(width, 1 | 2 | 4 | 8)
    }

    #[inline]
    fn valid_count_width(width: usize) -> bool {
        matches!(width, 1 | 2 | 4 | 8)
    }

    #[inline]
    fn permits_null() -> bool {
        false
    }

    #[inline]
    fn permits_uid() -> bool {
        false
    }

    #[inline]
    fn permits_set() -> bool {
        false
    }

    #[inline]
    fn permits_dictionary_key(node: &SemanticNode) -> bool {
        matches!(node, SemanticNode::String(_))
    }
}

#[derive(Debug)]
struct Trailer {
    reference_width: usize,
    object_count: usize,
    root_index: usize,
    object_table_end: usize,
    offsets: Vec<usize>,
}

impl Trailer {
    fn parse<P: Profile>(source: &[u8], options: &ParseOptions) -> Result<Self> {
        if source.len() > options.limits().max_input_bytes() {
            return Err(error_at(
                ErrorKind::LimitExceeded,
                0,
                "input exceeds the configured byte limit",
            ));
        }
        if source.get(..6) != Some(&b"bplist"[..]) {
            return Err(error_at(
                ErrorKind::InvalidMagic,
                0,
                "binary property-list magic bytes are absent or truncated",
            ));
        }
        if source.len() < HEADER_LEN {
            return Err(error_at(
                ErrorKind::UnsupportedVersion,
                6,
                "binary property-list version is truncated",
            ));
        }
        if !P::valid_header(source) {
            return Err(error_at(
                ErrorKind::UnsupportedVersion,
                6,
                "binary property-list version is not supported by this backend",
            ));
        }
        if source.len() < HEADER_LEN + 1 + TRAILER_LEN {
            return Err(error_at(
                ErrorKind::InvalidTrailer,
                source.len(),
                "binary property-list object table or trailer is truncated",
            ));
        }

        let trailer_start = source.len() - TRAILER_LEN;
        let trailer = &source[trailer_start..];
        let offset_width = usize::from(trailer[6]);
        let reference_width = usize::from(trailer[7]);
        let object_count_u64 = be_u64(&trailer[8..16]);
        let root_index_u64 = be_u64(&trailer[16..24]);
        let object_table_end_u64 = be_u64(&trailer[24..32]);

        if object_count_u64 > isize::MAX as u64 || object_table_end_u64 > isize::MAX as u64 {
            return Err(error_at(
                ErrorKind::InvalidTrailer,
                trailer_start,
                "trailer value exceeds CFIndex",
            ));
        }
        if object_count_u64 == 0 {
            return Err(error_at(
                ErrorKind::InvalidTrailer,
                trailer_start + 8,
                "object table is empty",
            ));
        }
        if root_index_u64 >= object_count_u64 {
            return Err(error_at(
                ErrorKind::InvalidReference,
                trailer_start + 16,
                "root object reference is out of range",
            ));
        }
        if object_table_end_u64 < 9 {
            return Err(error_at(
                ErrorKind::InvalidTrailer,
                trailer_start + 24,
                "offset table begins before the object table",
            ));
        }
        if object_table_end_u64 >= trailer_start as u64 {
            return Err(error_at(
                ErrorKind::InvalidTrailer,
                trailer_start + 24,
                "offset table overlaps the trailer",
            ));
        }
        if !P::valid_table_width(offset_width) || !P::valid_table_width(reference_width) {
            return Err(error_at(
                ErrorKind::InvalidTrailer,
                trailer_start + 6,
                "invalid offset or object-reference width",
            ));
        }

        let (table_size_u64, expected_len_u64) =
            match checked_table_layout(object_table_end_u64, object_count_u64, offset_width) {
                Ok(layout) => layout,
                Err(TableLayoutError::TableSizeOverflow) => {
                    return Err(error_at(
                        ErrorKind::InvalidTrailer,
                        trailer_start + 8,
                        "offset table size overflows",
                    ));
                }
                Err(TableLayoutError::FileSizeOverflow) => {
                    return Err(error_at(
                        ErrorKind::InvalidTrailer,
                        trailer_start + 24,
                        "binary property-list size overflows",
                    ));
                }
            };
        if table_size_u64 == 0 {
            return Err(error_at(
                ErrorKind::InvalidTrailer,
                trailer_start + 8,
                "offset table is empty",
            ));
        }
        if expected_len_u64 != source.len() as u64 {
            return Err(error_at(
                ErrorKind::InvalidTrailer,
                trailer_start,
                "binary property-list sections do not exactly fill the input",
            ));
        }

        if !width_can_address(reference_width, object_count_u64) {
            return Err(error_at(
                ErrorKind::InvalidTrailer,
                trailer_start + 7,
                "object references cannot address every object",
            ));
        }
        if !width_can_address(offset_width, object_table_end_u64) {
            return Err(error_at(
                ErrorKind::InvalidTrailer,
                trailer_start + 6,
                "offset integers cannot address the object table",
            ));
        }

        let object_count = usize::try_from(object_count_u64).map_err(|_| {
            error_at(
                ErrorKind::InvalidTrailer,
                trailer_start + 8,
                "object count does not fit usize",
            )
        })?;
        if object_count > options.limits().max_objects() {
            return Err(error_at(
                ErrorKind::LimitExceeded,
                trailer_start + 8,
                "object count exceeds the configured limit",
            ));
        }
        let root_index = usize::try_from(root_index_u64).map_err(|_| {
            error_at(
                ErrorKind::InvalidReference,
                trailer_start + 16,
                "root index does not fit usize",
            )
        })?;
        let object_table_end = usize::try_from(object_table_end_u64).map_err(|_| {
            error_at(
                ErrorKind::InvalidTrailer,
                trailer_start + 24,
                "offset table does not fit usize",
            )
        })?;
        let table_size = usize::try_from(table_size_u64).map_err(|_| {
            error_at(
                ErrorKind::InvalidTrailer,
                object_table_end,
                "offset table does not fit usize",
            )
        })?;
        let table_end = object_table_end.checked_add(table_size).ok_or_else(|| {
            error_at(
                ErrorKind::InvalidTrailer,
                object_table_end,
                "offset table range overflows",
            )
        })?;
        let table = source.get(object_table_end..table_end).ok_or_else(|| {
            error_at(
                ErrorKind::InvalidTrailer,
                object_table_end,
                "truncated offset table",
            )
        })?;

        let mut offsets = Vec::new();
        offsets
            .try_reserve_exact(object_count)
            .map_err(|_| allocation_error(object_table_end, "offset table"))?;
        let maximum_offset = object_table_end - 1;
        for (index, encoded) in table.chunks_exact(offset_width).enumerate() {
            let offset_u64 = wide_be_u64(encoded);
            if offset_u64 > maximum_offset as u64 {
                let location = object_table_end + index * offset_width;
                return Err(error_at(
                    ErrorKind::InvalidReference,
                    location,
                    "object offset is outside the object table",
                ));
            }
            offsets.push(offset_u64 as usize);
        }

        let root_offset = offsets[root_index];
        if !(HEADER_LEN..object_table_end).contains(&root_offset) {
            return Err(error_at(
                ErrorKind::InvalidReference,
                object_table_end + root_index * offset_width,
                "root offset is outside the object table",
            ));
        }

        Ok(Self {
            reference_width,
            object_count,
            root_index,
            object_table_end,
            offsets,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct Decoded {
    node: NodeId,
    semantic: SemanticId,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct SemanticId(usize);

/// A per-decode keyed summary of the pinned CF equality relation.
///
/// Equal values are guaranteed to have equal fingerprints; the converse is
/// deliberately not trusted. Normalization uses these only to select a short
/// candidate chain and always resolves collisions with `cf_equal`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct SemanticFingerprint(u64);

#[derive(Clone, Debug)]
enum StringStorage {
    Ascii(Range<usize>),
    Utf16Be(Range<usize>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NumericValue {
    Signed(i128),
    Unsigned(u128),
    F32(u32),
    F64(u64),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum CanonicalNumber {
    Integer(i128),
    Float(u64),
    Nan,
}

#[derive(Clone, Debug)]
enum SemanticNode {
    Null,
    Boolean(bool),
    Number(NumericValue),
    Date(u64),
    Data(Range<usize>),
    String(StringStorage),
    // Retain the payload for semantic diagnostics even though the historical
    // runtime's UID equality callback uses object identity, not this value.
    #[allow(dead_code)]
    Uid(u32),
    Array(Vec<SemanticId>),
    Set(Vec<SemanticId>),
    Dictionary(Vec<(SemanticId, SemanticId)>),
}

impl SemanticNode {
    #[inline]
    #[cfg(feature = "backend-cf-compat")]
    fn is_primitive(&self) -> bool {
        !matches!(
            self,
            SemanticNode::Array(_) | SemanticNode::Set(_) | SemanticNode::Dictionary(_)
        )
    }
}

struct Decoder<'a, P: Profile> {
    source: &'a [u8],
    trailer: Trailer,
    builder: ParsedBuilder,
    cache: HashMap<usize, Decoded>,
    visiting: HashSet<usize>,
    semantics: Vec<SemanticNode>,
    fingerprints: Vec<SemanticFingerprint>,
    fingerprint_state: RandomState,
    max_depth: usize,
    max_container_len: usize,
    max_string_bytes: usize,
    max_data_bytes: usize,
    profile: PhantomData<P>,
}

impl<'a, P: Profile> Decoder<'a, P> {
    fn parse(source: &'a [u8], options: &ParseOptions) -> Result<Parsed> {
        let trailer = Trailer::parse::<P>(source, options)?;
        let root_offset = trailer.offsets[trailer.root_index];
        let mut decoder = Self {
            source,
            trailer,
            builder: ParsedBuilder::new(
                source.len(),
                Format::Binary,
                P::backend(),
                options.limits().max_objects(),
            ),
            cache: HashMap::new(),
            visiting: HashSet::new(),
            semantics: Vec::new(),
            fingerprints: Vec::new(),
            fingerprint_state: RandomState::new(),
            max_depth: options.limits().max_depth().min(CF_MAX_DEPTH),
            max_container_len: options.limits().max_container_len(),
            max_string_bytes: options.limits().max_string_bytes(),
            max_data_bytes: options.limits().max_data_bytes(),
            profile: PhantomData,
        };
        let root = decoder.decode_object(root_offset, 0)?;
        decoder.builder.finish(root.node)
    }

    fn decode_object(&mut self, offset: usize, depth: usize) -> Result<Decoded> {
        if depth > self.max_depth {
            return Err(error_at(
                ErrorKind::LimitExceeded,
                offset,
                "maximum property-list depth exceeded",
            ));
        }
        if let Some(decoded) = self.cache.get(&offset) {
            return Ok(*decoded);
        }
        if !(HEADER_LEN..self.trailer.object_table_end).contains(&offset) {
            return Err(error_at(
                ErrorKind::InvalidReference,
                offset,
                "object offset is outside the object table",
            ));
        }

        let marker = self.source[offset];
        match marker >> 4 {
            0x0 => self.decode_simple(offset, marker),
            0x1 => self.decode_integer(offset, marker),
            0x2 => self.decode_real(offset, marker),
            0x3 => self.decode_date(offset, marker),
            0x4 => self.decode_data(offset, marker),
            0x5 => self.decode_ascii_string(offset, marker),
            0x6 => self.decode_utf16_string(offset, marker),
            0x8 => self.decode_uid(offset, marker),
            0xA => self.decode_sequence(offset, marker, depth, false),
            0xC => self.decode_sequence(offset, marker, depth, true),
            0xD => self.decode_dictionary(offset, marker, depth),
            _ => Err(error_at(
                ErrorKind::UnsupportedObject,
                offset,
                "unsupported binary property-list marker",
            )),
        }
    }

    fn decode_simple(&mut self, offset: usize, marker: u8) -> Result<Decoded> {
        let (node, semantic) = match marker {
            0x00 if P::permits_null() => (self.builder.null(), SemanticNode::Null),
            0x08 => (self.builder.boolean(false), SemanticNode::Boolean(false)),
            0x09 => (self.builder.boolean(true), SemanticNode::Boolean(true)),
            _ => {
                return Err(error_at(
                    ErrorKind::UnsupportedObject,
                    offset,
                    "invalid or unsupported null/boolean marker",
                ));
            }
        };
        Ok(self.register(offset, node, semantic))
    }

    fn decode_integer(&mut self, offset: usize, marker: u8) -> Result<Decoded> {
        let width = marker_power_width(marker);
        if width > 16 {
            return Err(error_at(
                ErrorKind::InvalidInteger,
                offset,
                "integer is wider than Core Foundation supports",
            ));
        }
        let payload = self.object_payload(offset, 1, width, ErrorKind::InvalidInteger)?;
        let bits = wide_be_u64(&self.source[payload.clone()]);
        let (integer, numeric) = cf_integer_from_bits(width, bits)
            .expect("power-of-two widths up to sixteen are exhaustive");
        let node = self.builder.integer(integer);
        Ok(self.register(offset, node, SemanticNode::Number(numeric)))
    }

    fn decode_real(&mut self, offset: usize, marker: u8) -> Result<Decoded> {
        let (node, semantic) = match marker & 0x0f {
            2 => {
                let payload = self.object_payload(offset, 1, 4, ErrorKind::InvalidReal)?;
                let bits = be_u32(&self.source[payload]);
                (
                    self.builder.real(Real::F32(f32::from_bits(bits))),
                    SemanticNode::Number(NumericValue::F32(bits)),
                )
            }
            3 => {
                let payload = self.object_payload(offset, 1, 8, ErrorKind::InvalidReal)?;
                let bits = be_u64(&self.source[payload]);
                (
                    self.builder.real(Real::F64(f64::from_bits(bits))),
                    SemanticNode::Number(NumericValue::F64(bits)),
                )
            }
            _ => {
                return Err(error_at(
                    ErrorKind::InvalidReal,
                    offset,
                    "real must contain an f32 or f64",
                ));
            }
        };
        Ok(self.register(offset, node, semantic))
    }

    fn decode_date(&mut self, offset: usize, marker: u8) -> Result<Decoded> {
        if marker != 0x33 {
            return Err(error_at(
                ErrorKind::InvalidDate,
                offset,
                "date must contain an eight-byte floating-point value",
            ));
        }
        let payload = self.object_payload(offset, 1, 8, ErrorKind::InvalidDate)?;
        let bits = be_u64(&self.source[payload]);
        let node = self
            .builder
            .date(Date::from_cf_absolute_time(f64::from_bits(bits)));
        Ok(self.register(offset, node, SemanticNode::Date(bits)))
    }

    fn decode_data(&mut self, offset: usize, marker: u8) -> Result<Decoded> {
        let (length, payload_start) = self.decode_count(offset, marker)?;
        if length > self.max_data_bytes {
            return Err(error_at(
                ErrorKind::LimitExceeded,
                offset,
                "data object exceeds the configured limit",
            ));
        }
        let payload = self.object_range(payload_start, length, ErrorKind::InvalidData)?;
        let node = self.builder.source_data(payload.clone());
        Ok(self.register(offset, node, SemanticNode::Data(payload)))
    }

    fn decode_ascii_string(&mut self, offset: usize, marker: u8) -> Result<Decoded> {
        let (length, payload_start) = self.decode_count(offset, marker)?;
        if length > self.max_string_bytes {
            return Err(error_at(
                ErrorKind::LimitExceeded,
                offset,
                "string exceeds the configured byte limit",
            ));
        }
        let payload = self.object_range(payload_start, length, ErrorKind::InvalidString)?;
        if !self.source[payload.clone()].is_ascii() {
            return Err(error_at(
                ErrorKind::InvalidString,
                payload.start,
                "ASCII string contains a non-ASCII byte",
            ));
        }
        let node = self.builder.source_string(payload.clone());
        Ok(self.register(
            offset,
            node,
            SemanticNode::String(StringStorage::Ascii(payload)),
        ))
    }

    fn decode_utf16_string(&mut self, offset: usize, marker: u8) -> Result<Decoded> {
        let (units, payload_start) = self.decode_count(offset, marker)?;
        let byte_length = units.checked_mul(2).ok_or_else(|| {
            error_at(
                ErrorKind::InvalidString,
                offset,
                "UTF-16 string length overflows",
            )
        })?;
        if byte_length > self.max_string_bytes {
            return Err(error_at(
                ErrorKind::LimitExceeded,
                offset,
                "string exceeds the configured byte limit",
            ));
        }
        let payload = self.object_range(payload_start, byte_length, ErrorKind::InvalidString)?;
        let node = self.builder.source_utf16be_string(payload.clone());
        Ok(self.register(
            offset,
            node,
            SemanticNode::String(StringStorage::Utf16Be(payload)),
        ))
    }

    fn decode_uid(&mut self, offset: usize, marker: u8) -> Result<Decoded> {
        if !P::permits_uid() {
            return Err(error_at(
                ErrorKind::UnsupportedObject,
                offset,
                "UID is not a public property-list type",
            ));
        }
        let width = usize::from(marker & 0x0f) + 1;
        let payload = self.object_payload(offset, 1, width, ErrorKind::InvalidInteger)?;
        let value = wide_be_u64(&self.source[payload]);
        let value = u32::try_from(value).map_err(|_| {
            error_at(
                ErrorKind::InvalidInteger,
                offset,
                "UID value exceeds the Core Foundation range",
            )
        })?;
        let node = self.builder.uid(value);
        Ok(self.register(offset, node, SemanticNode::Uid(value)))
    }

    fn decode_sequence(
        &mut self,
        offset: usize,
        marker: u8,
        depth: usize,
        is_set: bool,
    ) -> Result<Decoded> {
        if is_set && !P::permits_set() {
            return Err(error_at(
                ErrorKind::UnsupportedObject,
                offset,
                "set is not a public property-list type",
            ));
        }
        let (count, references_start) = self.decode_count(offset, marker)?;
        if count > self.max_container_len {
            return Err(error_at(
                ErrorKind::LimitExceeded,
                offset,
                "container exceeds the configured length limit",
            ));
        }
        let references = self.read_references(references_start, count)?;

        if !self.visiting.insert(offset) {
            return Err(error_at(
                ErrorKind::InvalidReference,
                offset,
                "cyclic object reference",
            ));
        }
        let decoded_result = (|| {
            let mut children = Vec::new();
            children
                .try_reserve_exact(references.len())
                .map_err(|_| allocation_error(offset, "container entries"))?;
            for child_offset in references {
                children.push(self.decode_object(child_offset, depth + 1)?);
            }

            if is_set {
                let mut unique = Vec::new();
                unique
                    .try_reserve_exact(children.len())
                    .map_err(|_| allocation_error(offset, "set entries"))?;
                let mut bucket_heads = HashMap::new();
                bucket_heads
                    .try_reserve(children.len())
                    .map_err(|_| allocation_error(offset, "set fingerprint buckets"))?;
                let mut next_candidate = Vec::new();
                next_candidate
                    .try_reserve_exact(children.len())
                    .map_err(|_| allocation_error(offset, "set fingerprint chains"))?;
                for child in children {
                    let fingerprint = self.fingerprint(child.semantic);
                    let duplicate = self.bucket_contains_equal(
                        bucket_heads.get(&fingerprint).copied(),
                        &next_candidate,
                        &unique,
                        child.semantic,
                        |existing: &Decoded| existing.semantic,
                    );
                    if !duplicate {
                        let index = unique.len();
                        let previous = bucket_heads.insert(fingerprint, index);
                        unique.push(child);
                        next_candidate.push(previous);
                    }
                }
                let node_ids = unique.iter().map(|child| child.node).collect();
                let semantic_ids = unique.iter().map(|child| child.semantic).collect();
                let node = self.builder.set(node_ids);
                Ok(self.register(offset, node, SemanticNode::Set(semantic_ids)))
            } else {
                let node_ids = children.iter().map(|child| child.node).collect();
                let semantic_ids = children.iter().map(|child| child.semantic).collect();
                let node = self.builder.array(node_ids);
                Ok(self.register(offset, node, SemanticNode::Array(semantic_ids)))
            }
        })();
        self.visiting.remove(&offset);
        decoded_result
    }

    fn decode_dictionary(&mut self, offset: usize, marker: u8, depth: usize) -> Result<Decoded> {
        let (count, references_start) = self.decode_count(offset, marker)?;
        if count > self.max_container_len {
            return Err(error_at(
                ErrorKind::LimitExceeded,
                offset,
                "dictionary exceeds the configured length limit",
            ));
        }
        let reference_count = count.checked_mul(2).ok_or_else(|| {
            error_at(
                ErrorKind::InvalidReference,
                offset,
                "dictionary reference count overflows",
            )
        })?;
        let references = self.read_references(references_start, reference_count)?;
        let (key_offsets, value_offsets) = references.split_at(count);

        if !self.visiting.insert(offset) {
            return Err(error_at(
                ErrorKind::InvalidReference,
                offset,
                "cyclic object reference",
            ));
        }
        let decoded_result = (|| {
            let mut keys = Vec::new();
            keys.try_reserve_exact(count)
                .map_err(|_| allocation_error(offset, "dictionary keys"))?;
            for &key_offset in key_offsets {
                let key = self.decode_object(key_offset, depth + 1)?;
                if !P::permits_dictionary_key(&self.semantics[key.semantic.0]) {
                    return Err(error_at(
                        ErrorKind::InvalidKey,
                        key_offset,
                        "invalid dictionary key type",
                    ));
                }
                keys.push(key);
            }

            let mut values = Vec::new();
            values
                .try_reserve_exact(count)
                .map_err(|_| allocation_error(offset, "dictionary values"))?;
            for &value_offset in value_offsets {
                values.push(self.decode_object(value_offset, depth + 1)?);
            }

            let mut entries: Vec<(Decoded, Decoded)> = Vec::new();
            entries
                .try_reserve_exact(count)
                .map_err(|_| allocation_error(offset, "dictionary entries"))?;
            let mut bucket_heads = HashMap::new();
            bucket_heads
                .try_reserve(count)
                .map_err(|_| allocation_error(offset, "dictionary fingerprint buckets"))?;
            let mut next_candidate = Vec::new();
            next_candidate
                .try_reserve_exact(count)
                .map_err(|_| allocation_error(offset, "dictionary fingerprint chains"))?;
            for (key, value) in keys.into_iter().zip(values) {
                let fingerprint = self.fingerprint(key.semantic);
                let duplicate = self.bucket_contains_equal(
                    bucket_heads.get(&fingerprint).copied(),
                    &next_candidate,
                    &entries,
                    key.semantic,
                    |(existing, _): &(Decoded, Decoded)| existing.semantic,
                );
                if !duplicate {
                    // The pinned binary reader constructs dictionaries with
                    // CFDictionaryAddValue/__CFDictionaryCreateTransfer.
                    // Both preserve the first value for an equal duplicate
                    // key. XML uses CFDictionarySetValue and is intentionally
                    // last-wins instead.
                    let index = entries.len();
                    let previous = bucket_heads.insert(fingerprint, index);
                    entries.push((key, value));
                    next_candidate.push(previous);
                }
            }

            let node_entries = entries
                .iter()
                .map(|(key, value)| (key.node, value.node))
                .collect();
            let semantic_entries = entries
                .iter()
                .map(|(key, value)| (key.semantic, value.semantic))
                .collect();
            let node = self.builder.dictionary(node_entries);
            Ok(self.register(offset, node, SemanticNode::Dictionary(semantic_entries)))
        })();
        self.visiting.remove(&offset);
        decoded_result
    }

    fn decode_count(&self, object_offset: usize, marker: u8) -> Result<(usize, usize)> {
        if let Some(compact) = inline_count(marker) {
            return Ok((compact, object_offset + 1));
        }

        let count_marker_offset = object_offset.checked_add(1).ok_or_else(|| {
            error_at(
                ErrorKind::InvalidInteger,
                object_offset,
                "count marker offset overflows",
            )
        })?;
        let count_marker = *self
            .source
            .get(count_marker_offset)
            .filter(|_| count_marker_offset < self.trailer.object_table_end)
            .ok_or_else(|| {
                error_at(
                    ErrorKind::InvalidInteger,
                    count_marker_offset,
                    "missing extended count",
                )
            })?;
        if count_marker >> 4 != 0x1 {
            return Err(error_at(
                ErrorKind::InvalidInteger,
                count_marker_offset,
                "extended object count is not an integer",
            ));
        }
        let width = marker_power_width(count_marker);
        if !P::valid_count_width(width) {
            return Err(error_at(
                ErrorKind::InvalidInteger,
                count_marker_offset,
                "extended count uses a non-canonical width",
            ));
        }
        let payload_start = count_marker_offset + 1;
        let payload = self.object_range(payload_start, width, ErrorKind::InvalidInteger)?;
        let value = wide_be_u64(&self.source[payload]);
        if value > isize::MAX as u64 {
            return Err(error_at(
                ErrorKind::InvalidInteger,
                count_marker_offset,
                "object count exceeds CFIndex",
            ));
        }
        let value = usize::try_from(value).map_err(|_| {
            error_at(
                ErrorKind::InvalidInteger,
                count_marker_offset,
                "object count does not fit usize",
            )
        })?;
        let content_start = payload_start.checked_add(width).ok_or_else(|| {
            error_at(
                ErrorKind::InvalidInteger,
                payload_start,
                "object content offset overflows",
            )
        })?;
        Ok((value, content_start))
    }

    fn read_references(&self, start: usize, count: usize) -> Result<Vec<usize>> {
        let byte_length = count
            .checked_mul(self.trailer.reference_width)
            .ok_or_else(|| {
                error_at(
                    ErrorKind::InvalidReference,
                    start,
                    "object-reference block overflows",
                )
            })?;
        let range = self.object_range(start, byte_length, ErrorKind::InvalidReference)?;
        let mut offsets = Vec::new();
        offsets
            .try_reserve_exact(count)
            .map_err(|_| allocation_error(start, "object references"))?;
        for (index, encoded) in self.source[range]
            .chunks_exact(self.trailer.reference_width)
            .enumerate()
        {
            let reference = wide_be_u64(encoded);
            if reference >= self.trailer.object_count as u64 {
                return Err(error_at(
                    ErrorKind::InvalidReference,
                    start + index * self.trailer.reference_width,
                    "object reference is out of range",
                ));
            }
            offsets.push(self.trailer.offsets[reference as usize]);
        }
        Ok(offsets)
    }

    fn object_payload(
        &self,
        object_offset: usize,
        prefix: usize,
        length: usize,
        kind: ErrorKind,
    ) -> Result<Range<usize>> {
        let start = object_offset
            .checked_add(prefix)
            .ok_or_else(|| error_at(kind, object_offset, "object payload offset overflows"))?;
        self.object_range(start, length, kind)
    }

    fn object_range(&self, start: usize, length: usize, kind: ErrorKind) -> Result<Range<usize>> {
        match checked_object_range(start, length, self.trailer.object_table_end) {
            RangeCheck::Valid(range) => Ok(range),
            RangeCheck::Overflow => Err(error_at(kind, start, "object range overflows")),
            RangeCheck::Outside => Err(error_at(
                kind,
                start,
                "object payload extends beyond the object table",
            )),
        }
    }

    fn register(&mut self, offset: usize, node: NodeId, semantic: SemanticNode) -> Decoded {
        let semantic_id = SemanticId(self.semantics.len());
        let fingerprint = self.semantic_fingerprint(semantic_id, &semantic);
        self.semantics.push(semantic);
        self.fingerprints.push(fingerprint);
        let decoded = Decoded {
            node,
            semantic: semantic_id,
        };
        self.cache.insert(offset, decoded);
        decoded
    }

    #[inline]
    fn fingerprint(&self, semantic: SemanticId) -> SemanticFingerprint {
        self.fingerprints[semantic.0]
    }

    fn semantic_fingerprint(
        &self,
        semantic_id: SemanticId,
        semantic: &SemanticNode,
    ) -> SemanticFingerprint {
        const NULL: u8 = 0;
        const BOOLEAN: u8 = 1;
        const NUMBER: u8 = 2;
        const DATE: u8 = 3;
        const DATE_NAN_IDENTITY: u8 = 4;
        const DATA: u8 = 5;
        const STRING: u8 = 6;
        const UID_IDENTITY: u8 = 7;
        const ARRAY: u8 = 8;
        const SET: u8 = 9;
        const DICTIONARY: u8 = 10;
        const DICTIONARY_ENTRY: u8 = 11;

        let mut hasher = self.fingerprint_state.build_hasher();
        match semantic {
            SemanticNode::Null => NULL.hash(&mut hasher),
            SemanticNode::Boolean(value) => {
                BOOLEAN.hash(&mut hasher);
                value.hash(&mut hasher);
            }
            SemanticNode::Number(value) => {
                NUMBER.hash(&mut hasher);
                canonical_number(*value).hash(&mut hasher);
            }
            SemanticNode::Date(bits) => {
                let value = f64::from_bits(*bits);
                if value.is_nan() {
                    // Distinct CFDate NaNs compare unequal. Giving them
                    // identity fingerprints prevents a file containing one
                    // repeated NaN bit pattern from forcing a quadratic
                    // collision bucket.
                    DATE_NAN_IDENTITY.hash(&mut hasher);
                    semantic_id.hash(&mut hasher);
                } else {
                    DATE.hash(&mut hasher);
                    // CFDate equality treats the two zero signs as equal.
                    if value == 0.0 {
                        0_u64.hash(&mut hasher);
                    } else {
                        bits.hash(&mut hasher);
                    }
                }
            }
            SemanticNode::Data(range) => {
                DATA.hash(&mut hasher);
                self.source[range.clone()].hash(&mut hasher);
            }
            SemanticNode::String(storage) => {
                STRING.hash(&mut hasher);
                let length = string_unit_len(storage);
                length.hash(&mut hasher);
                for index in 0..length {
                    string_unit(self.source, storage, index).hash(&mut hasher);
                }
            }
            SemanticNode::Uid(_) => {
                // The pinned UID class compares by object identity rather than
                // payload, so the semantic id is its canonical equality key.
                UID_IDENTITY.hash(&mut hasher);
                semantic_id.hash(&mut hasher);
            }
            SemanticNode::Array(values) => {
                ARRAY.hash(&mut hasher);
                values.len().hash(&mut hasher);
                for &value in values {
                    self.fingerprint(value).hash(&mut hasher);
                }
            }
            SemanticNode::Set(values) => {
                return self.unordered_fingerprint(
                    SET,
                    values.iter().map(|&value| self.fingerprint(value).0),
                );
            }
            SemanticNode::Dictionary(entries) => {
                let entry_fingerprints = entries.iter().map(|&(key, value)| {
                    let mut entry_hasher = self.fingerprint_state.build_hasher();
                    DICTIONARY_ENTRY.hash(&mut entry_hasher);
                    self.fingerprint(key).hash(&mut entry_hasher);
                    self.fingerprint(value).hash(&mut entry_hasher);
                    entry_hasher.finish()
                });
                return self.unordered_fingerprint(DICTIONARY, entry_fingerprints);
            }
        }
        SemanticFingerprint(hasher.finish())
    }

    fn unordered_fingerprint(
        &self,
        tag: u8,
        values: impl Iterator<Item = u64>,
    ) -> SemanticFingerprint {
        let mut count = 0_usize;
        let mut sum = 0_u64;
        let mut xor = 0_u64;
        let mut product = 1_u64;
        for value in values {
            count += 1;
            sum = sum.wrapping_add(value);
            xor ^= value.rotate_left((value >> 58) as u32);
            product = product.wrapping_mul(value | 1);
        }

        let mut hasher = self.fingerprint_state.build_hasher();
        tag.hash(&mut hasher);
        count.hash(&mut hasher);
        sum.hash(&mut hasher);
        xor.hash(&mut hasher);
        product.hash(&mut hasher);
        SemanticFingerprint(hasher.finish())
    }

    fn bucket_contains_equal<T>(
        &self,
        mut candidate: Option<usize>,
        next_candidate: &[Option<usize>],
        values: &[T],
        needle: SemanticId,
        semantic: impl Fn(&T) -> SemanticId,
    ) -> bool {
        while let Some(index) = candidate {
            if self.cf_equal(semantic(&values[index]), needle) {
                return true;
            }
            candidate = next_candidate[index];
        }
        false
    }

    fn cf_equal(&self, left: SemanticId, right: SemanticId) -> bool {
        let mut memo = HashMap::new();
        self.cf_equal_inner(left, right, &mut memo)
    }

    fn cf_equal_inner(
        &self,
        left: SemanticId,
        right: SemanticId,
        memo: &mut HashMap<(usize, usize), bool>,
    ) -> bool {
        if left == right {
            return true;
        }
        if self.fingerprint(left) != self.fingerprint(right) {
            return false;
        }
        let key = if left.0 < right.0 {
            (left.0, right.0)
        } else {
            (right.0, left.0)
        };
        if let Some(equal) = memo.get(&key) {
            return *equal;
        }

        let equal = match (&self.semantics[left.0], &self.semantics[right.0]) {
            (SemanticNode::Null, SemanticNode::Null) => true,
            (SemanticNode::Boolean(left), SemanticNode::Boolean(right)) => left == right,
            (SemanticNode::Number(left), SemanticNode::Number(right)) => {
                numbers_equal(*left, *right)
            }
            (SemanticNode::Date(left), SemanticNode::Date(right)) => {
                f64::from_bits(*left) == f64::from_bits(*right)
            }
            (SemanticNode::Data(left), SemanticNode::Data(right)) => {
                self.source[left.clone()] == self.source[right.clone()]
            }
            (SemanticNode::String(left), SemanticNode::String(right)) => {
                self.strings_equal(left, right)
            }
            // The historical UID runtime class has no equality callback:
            // distinct UID objects compare by pointer identity. The `left ==
            // right` fast path above still makes shared object references
            // identical, regardless of their numeric payload.
            (SemanticNode::Uid(_), SemanticNode::Uid(_)) => false,
            (SemanticNode::Array(left), SemanticNode::Array(right)) => {
                left.len() == right.len()
                    && left
                        .iter()
                        .zip(right)
                        .all(|(&a, &b)| self.cf_equal_inner(a, b, memo))
            }
            (SemanticNode::Set(left), SemanticNode::Set(right)) => {
                self.unordered_values_equal(left, right, memo)
            }
            (SemanticNode::Dictionary(left), SemanticNode::Dictionary(right)) => {
                self.dictionaries_equal(left, right, memo)
            }
            _ => false,
        };
        memo.insert(key, equal);
        equal
    }

    fn unordered_values_equal(
        &self,
        left: &[SemanticId],
        right: &[SemanticId],
        memo: &mut HashMap<(usize, usize), bool>,
    ) -> bool {
        if left.len() != right.len() {
            return false;
        }
        let mut bucket_heads = HashMap::with_capacity(right.len());
        let mut next_candidate = Vec::with_capacity(right.len());
        for (index, &value) in right.iter().enumerate() {
            let previous = bucket_heads.insert(self.fingerprint(value), index);
            next_candidate.push(previous);
        }
        let mut matched = vec![false; right.len()];
        for &left_value in left {
            let mut candidate = bucket_heads.get(&self.fingerprint(left_value)).copied();
            let mut matching_index = None;
            while let Some(index) = candidate {
                if !matched[index] && self.cf_equal_inner(left_value, right[index], memo) {
                    matching_index = Some(index);
                    break;
                }
                candidate = next_candidate[index];
            }
            let Some(index) = matching_index else {
                return false;
            };
            matched[index] = true;
        }
        true
    }

    fn dictionaries_equal(
        &self,
        left: &[(SemanticId, SemanticId)],
        right: &[(SemanticId, SemanticId)],
        memo: &mut HashMap<(usize, usize), bool>,
    ) -> bool {
        if left.len() != right.len() {
            return false;
        }
        let mut bucket_heads = HashMap::with_capacity(right.len());
        let mut next_candidate = Vec::with_capacity(right.len());
        for (index, &(key, _)) in right.iter().enumerate() {
            let previous = bucket_heads.insert(self.fingerprint(key), index);
            next_candidate.push(previous);
        }
        for &(left_key, left_value) in left {
            let mut candidate = bucket_heads.get(&self.fingerprint(left_key)).copied();
            let mut matching_value = None;
            while let Some(index) = candidate {
                let (right_key, right_value) = right[index];
                if self.cf_equal_inner(left_key, right_key, memo) {
                    matching_value = Some(right_value);
                    break;
                }
                candidate = next_candidate[index];
            }
            let Some(right_value) = matching_value else {
                return false;
            };
            if !self.cf_equal_inner(left_value, right_value, memo) {
                return false;
            }
        }
        true
    }

    fn strings_equal(&self, left: &StringStorage, right: &StringStorage) -> bool {
        let left_len = string_unit_len(left);
        if left_len != string_unit_len(right) {
            return false;
        }
        (0..left_len).all(|index| {
            string_unit(self.source, left, index) == string_unit(self.source, right, index)
        })
    }
}

#[inline]
fn error_at(kind: ErrorKind, offset: usize, message: impl Into<Cow<'static, str>>) -> Error {
    Error::at(kind, Format::Binary, offset).with_message(message)
}

#[inline]
fn allocation_error(offset: usize, subject: &'static str) -> Error {
    error_at(
        ErrorKind::LimitExceeded,
        offset,
        format!("allocation reservation failed for {subject}"),
    )
}

#[inline]
fn be_u32(bytes: &[u8]) -> u32 {
    let bytes: [u8; 4] = bytes.try_into().expect("validated four-byte field");
    u32::from_be_bytes(bytes)
}

#[inline]
fn be_u64(bytes: &[u8]) -> u64 {
    let bytes: [u8; 8] = bytes.try_into().expect("validated eight-byte field");
    u64::from_be_bytes(bytes)
}

/// Computes the two variable binary-plist section lengths without wrapping.
///
/// This is kept as a total scalar kernel so the exact arithmetic used by the
/// parser can be model checked independently of allocation and I/O.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TableLayoutError {
    TableSizeOverflow,
    FileSizeOverflow,
}

#[inline]
fn checked_table_layout(
    object_table_end: u64,
    object_count: u64,
    offset_width: usize,
) -> std::result::Result<(u64, u64), TableLayoutError> {
    let table_size = object_count
        .checked_mul(offset_width as u64)
        .ok_or(TableLayoutError::TableSizeOverflow)?;
    let file_size = object_table_end
        .checked_add(table_size)
        .and_then(|value| value.checked_add(TRAILER_LEN as u64))
        .ok_or(TableLayoutError::FileSizeOverflow)?;
    Ok((table_size, file_size))
}

/// Returns whether an unsigned big-endian field of `width` bytes can encode
/// every value through `inclusive_max`.
#[inline]
fn width_can_address(width: usize, inclusive_max: u64) -> bool {
    width >= 8 || (1_u64 << (width * 8)) > inclusive_max
}

/// Decodes the marker's power-of-two payload width.
#[inline]
fn marker_power_width(marker: u8) -> usize {
    1_usize << usize::from(marker & 0x0f)
}

/// Returns an inline collection count, or `None` for the extended-count form.
#[inline]
fn inline_count(marker: u8) -> Option<usize> {
    let count = usize::from(marker & 0x0f);
    (count != 0x0f).then_some(count)
}

/// Applies the format-`00` integer signedness rule after byte folding.
#[inline]
fn cf_integer_from_bits(width: usize, bits: u64) -> Option<(Integer, NumericValue)> {
    match width {
        1 | 2 | 4 => Some((
            Integer::Unsigned(u128::from(bits)),
            NumericValue::Unsigned(u128::from(bits)),
        )),
        8 => {
            let value = i128::from(bits as i64);
            Some((Integer::Signed(value), NumericValue::Signed(value)))
        }
        16 => Some((
            Integer::Unsigned(u128::from(bits)),
            NumericValue::Unsigned(u128::from(bits)),
        )),
        _ => None,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RangeCheck {
    Valid(Range<usize>),
    Overflow,
    Outside,
}

/// Performs the exact checked range operation used for every object payload.
#[inline]
fn checked_object_range(start: usize, length: usize, limit: usize) -> RangeCheck {
    let Some(end) = start.checked_add(length) else {
        return RangeCheck::Overflow;
    };
    if start > limit || end > limit {
        RangeCheck::Outside
    } else {
        RangeCheck::Valid(start..end)
    }
}

/// Reads a Core Foundation sized integer. Widths greater than eight retain
/// only the low 64 bits, matching unsigned C arithmetic in the pinned target.
#[inline]
fn wide_be_u64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0_u64, |value, byte| {
        value.wrapping_shl(8) | u64::from(*byte)
    })
}

#[inline]
fn string_unit_len(storage: &StringStorage) -> usize {
    match storage {
        StringStorage::Ascii(range) => range.len(),
        StringStorage::Utf16Be(range) => range.len() / 2,
    }
}

#[inline]
fn string_unit(source: &[u8], storage: &StringStorage, index: usize) -> u16 {
    match storage {
        StringStorage::Ascii(range) => u16::from(source[range.start + index]),
        StringStorage::Utf16Be(range) => {
            let offset = range.start + index * 2;
            u16::from_be_bytes([source[offset], source[offset + 1]])
        }
    }
}

fn canonical_number(value: NumericValue) -> CanonicalNumber {
    if let Some(integer) = integer_value(value) {
        return CanonicalNumber::Integer(integer);
    }

    let float = float_value(value);
    if float.is_nan() {
        return CanonicalNumber::Nan;
    }
    if float.is_finite()
        && !(float.is_sign_negative() && float == 0.0)
        && float.fract() == 0.0
        && float >= i128::MIN as f64
        && float < i128::MAX as f64
    {
        let integer = float as i128;
        if integer as f64 == float {
            return CanonicalNumber::Integer(integer);
        }
    }
    CanonicalNumber::Float(float.to_bits())
}

fn numbers_equal(left: NumericValue, right: NumericValue) -> bool {
    match (integer_value(left), integer_value(right)) {
        (Some(left), Some(right)) => return left == right,
        (Some(integer), None) => return float_integer_equal(float_value(right), integer),
        (None, Some(integer)) => return float_integer_equal(float_value(left), integer),
        (None, None) => {}
    }

    let left = float_value(left);
    let right = float_value(right);
    if left.is_nan() || right.is_nan() {
        return left.is_nan() && right.is_nan();
    }
    if left.is_sign_negative() != right.is_sign_negative() {
        return false;
    }
    left == right
}

#[inline]
fn integer_value(value: NumericValue) -> Option<i128> {
    match value {
        NumericValue::Signed(value) => Some(value),
        NumericValue::Unsigned(value) => i128::try_from(value).ok(),
        NumericValue::F32(_) | NumericValue::F64(_) => None,
    }
}

#[inline]
fn float_value(value: NumericValue) -> f64 {
    match value {
        NumericValue::F32(bits) => f64::from(f32::from_bits(bits)),
        NumericValue::F64(bits) => f64::from_bits(bits),
        NumericValue::Signed(_) | NumericValue::Unsigned(_) => {
            unreachable!("float_value is called only for a floating representation")
        }
    }
}

fn float_integer_equal(float: f64, integer: i128) -> bool {
    if !float.is_finite() || float.is_sign_negative() && integer == 0 || float.fract() != 0.0 {
        return false;
    }
    if float < i128::MIN as f64 || float >= i128::MAX as f64 {
        return false;
    }
    let converted = float as i128;
    converted == integer && converted as f64 == float
}

// Kani injects its support crate only while `cargo kani` is compiling this
// crate. Keeping the harnesses beside these private functions ensures the
// model checker exercises the same kernels as production decoding.
#[cfg(kani)]
mod kani_proofs {
    use super::{
        cf_integer_from_bits, checked_object_range, checked_table_layout, inline_count,
        wide_be_u64, width_can_address, NumericValue, RangeCheck, TRAILER_LEN,
    };
    use crate::Integer;

    const MAX_SIZED_INTEGER_BYTES: usize = 16;

    fn exact_big_endian_value(bytes: &[u8]) -> u128 {
        let mut value = 0_u128;
        for &byte in bytes {
            value = (value << 8) | u128::from(byte);
        }
        value
    }

    /// Exhaustive over every byte string of length zero through sixteen.
    #[kani::proof]
    #[kani::unwind(17)]
    fn wide_be_u64_is_the_low_half_of_the_exact_value() {
        let bytes: [u8; MAX_SIZED_INTEGER_BYTES] = kani::any();
        let length: u8 = kani::any();
        kani::assume(usize::from(length) <= MAX_SIZED_INTEGER_BYTES);
        let bytes = &bytes[..usize::from(length)];

        assert_eq!(wide_be_u64(bytes), exact_big_endian_value(bytes) as u64);
    }

    /// Exhaustive over all folded 64-bit values for every accepted integer
    /// width (1, 2, 4, 8, and 16 bytes).
    #[kani::proof]
    fn format_00_integer_signedness_matches_the_specification() {
        let exponent: u8 = kani::any();
        let bits: u64 = kani::any();
        kani::assume(exponent <= 4);
        let width = 1_usize << usize::from(exponent);
        let result = cf_integer_from_bits(width, bits);

        if width == 8 {
            let signed = i128::from(bits as i64);
            assert_eq!(
                result,
                Some((Integer::Signed(signed), NumericValue::Signed(signed)))
            );
        } else {
            let unsigned = u128::from(bits);
            assert_eq!(
                result,
                Some((
                    Integer::Unsigned(unsigned),
                    NumericValue::Unsigned(unsigned)
                ))
            );
        }
    }

    /// Exhaustive over all 64-bit start, length, and object-table limits.
    #[kani::proof]
    fn object_range_check_matches_exact_arithmetic() {
        let start: usize = kani::any();
        let length: usize = kani::any();
        let limit: usize = kani::any();
        let exact_end = (start as u128) + (length as u128);

        let expected = if exact_end > usize::MAX as u128 {
            RangeCheck::Overflow
        } else if start > limit || exact_end > limit as u128 {
            RangeCheck::Outside
        } else {
            RangeCheck::Valid(start..exact_end as usize)
        };
        assert_eq!(checked_object_range(start, length, limit), expected);
    }

    /// Exhaustive over every section position and object count admitted by the
    /// default limits, and all widths representable in a trailer byte.
    #[kani::proof]
    fn bounded_table_layout_matches_exact_arithmetic() {
        const MAX_INPUT_BYTES: u64 = 256 * 1024 * 1024;
        const MAX_OBJECTS: u64 = 1_000_000;

        let object_table_end = u64::from(kani::any::<u32>());
        let object_count = u64::from(kani::any::<u32>());
        let encoded_width: u8 = kani::any();
        let offset_width = usize::from(encoded_width);
        kani::assume(object_table_end <= MAX_INPUT_BYTES);
        kani::assume(object_count <= MAX_OBJECTS);

        let exact_table_size = (object_count as u128) * (offset_width as u128);
        let exact_file_size = (object_table_end as u128) + exact_table_size + (TRAILER_LEN as u128);
        match checked_table_layout(object_table_end, object_count, offset_width) {
            Ok((table_size, file_size)) => {
                assert_eq!(u128::from(table_size), exact_table_size);
                assert_eq!(u128::from(file_size), exact_file_size);
            }
            Err(_) => panic!("bounded table layout cannot overflow u64"),
        }
    }

    /// Exhaustive over all trailer widths and 64-bit inclusive maxima.
    #[kani::proof]
    fn address_width_check_matches_exact_capacity() {
        let encoded_width: u8 = kani::any();
        let inclusive_max: u64 = kani::any();
        let width = usize::from(encoded_width);
        let expected = width >= 8 || (1_u128 << (width * 8)) > inclusive_max as u128;

        assert_eq!(width_can_address(width, inclusive_max), expected);
    }

    /// Exhaustive over every possible marker byte.
    #[kani::proof]
    fn inline_count_marker_matches_the_format_nibble() {
        let marker: u8 = kani::any();
        let nibble = usize::from(marker & 0x0f);
        let expected = if nibble == 0x0f { None } else { Some(nibble) };

        assert_eq!(inline_count(marker), expected);
    }
}

#[cfg(test)]
mod tests {
    use super::{numbers_equal, wide_be_u64, NumericValue};

    #[test]
    fn wide_integer_keeps_the_low_sixty_four_bits() {
        let bytes = [
            0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x11, 0x22, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
            0xcd, 0xef,
        ];
        assert_eq!(wide_be_u64(&bytes), 0x0123_4567_89ab_cdef);
    }

    #[test]
    fn cf_number_equality_distinguishes_signed_zero() {
        assert!(numbers_equal(
            NumericValue::F32(f32::NAN.to_bits()),
            NumericValue::F64(f64::NAN.to_bits())
        ));
        assert!(!numbers_equal(
            NumericValue::F64((-0.0_f64).to_bits()),
            NumericValue::Signed(0)
        ));
        assert!(numbers_equal(
            NumericValue::F64(1.0_f64.to_bits()),
            NumericValue::Unsigned(1)
        ));
    }
}
