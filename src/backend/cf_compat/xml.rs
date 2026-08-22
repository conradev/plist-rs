//! Safe property-list XML readers.
//!
//! This module intentionally does not use a general-purpose XML parser.  The
//! CoreFoundation property-list reader implements a small, legacy XML dialect
//! with observable behavior that differs from a validating XML processor.  A
//! single tokenizer is shared by two policies here: the compatibility policy
//! preserves those behaviors, while the pure policy accepts only the public
//! property-list grammar.
//!
//! Compatibility provenance is the APSL-2.0 CoreFoundation
//! `CFPropertyList.c` snapshot at opensource-apple/CF commit
//! `3cc41a76b1491f50813e28a4ec09954ffa359e6f`. The parser is a safe Rust
//! implementation of the observable grammar, not incorporated C source.
//! Swift Corelibs Foundation commit `761b621` is used as a hardening
//! cross-check where the historical implementation relied on unchecked
//! pointer arithmetic or platform conversion routines.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;

use crate::document::{NodeId, Parsed, ParsedBuilder};
use crate::{BackendKind, Date, Error, ErrorKind, Format, Integer, ParseOptions, Real, Result};

#[cfg(any(target_os = "ios", target_os = "android"))]
const CF_MAX_DEPTH: usize = 128;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
const CF_MAX_DEPTH: usize = 512;
const CF_UID_KEY: &[u16] = &[
    b'C' as u16,
    b'F' as u16,
    b'$' as u16,
    b'U' as u16,
    b'I' as u16,
    b'D' as u16,
];

/// Parses XML using CoreFoundation-compatible acceptance rules.
pub(crate) fn parse_xml_cf(source: &[u8], options: &ParseOptions) -> Result<Parsed> {
    parse_with_profile(source, options, Profile::CoreFoundation)
}

/// Parses XML using the public, dependency-free property-list grammar.
pub(crate) fn parse_xml_pure(source: &[u8], options: &ParseOptions) -> Result<Parsed> {
    parse_with_profile(source, options, Profile::Pure)
}

fn parse_with_profile(source: &[u8], options: &ParseOptions, profile: Profile) -> Result<Parsed> {
    if source.len() > options.limits().max_input_bytes() {
        return Err(error_at(
            profile.backend(),
            ErrorKind::LimitExceeded,
            0,
            "XML input exceeds the configured byte limit",
        ));
    }

    let decoded = DecodedInput::new(source, profile.backend())?;
    XmlParser::new(source.len(), &decoded, options, profile).parse_document()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Profile {
    CoreFoundation,
    Pure,
}

impl Profile {
    const fn backend(self) -> BackendKind {
        match self {
            Self::CoreFoundation => BackendKind::CoreFoundation,
            Self::Pure => BackendKind::Pure,
        }
    }

    const fn is_cf(self) -> bool {
        matches!(self, Self::CoreFoundation)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WireEncoding {
    Utf8,
    Utf16Be,
    Utf16Le,
    Utf32Be,
    Utf32Le,
}

#[derive(Debug)]
enum PositionMap<'source> {
    Shift(usize),
    Transcoded(RefCell<TranscodedPositionMap<'source>>),
}

#[derive(Debug)]
struct TranscodedPositionMap<'source> {
    source: &'source [u8],
    encoding: WireEncoding,
    source_start: usize,
    decoded_cursor: usize,
    source_cursor: usize,
}

impl<'source> TranscodedPositionMap<'source> {
    fn new(source: &'source [u8], encoding: WireEncoding, skip: usize) -> Self {
        Self {
            source,
            encoding,
            source_start: skip,
            decoded_cursor: 0,
            source_cursor: skip,
        }
    }

    fn source_offset(&mut self, target: usize) -> usize {
        if target < self.decoded_cursor {
            self.decoded_cursor = 0;
            self.source_cursor = self.source_start;
        }

        while self.decoded_cursor < target && self.source_cursor < self.source.len() {
            let character_start = self.source_cursor;
            let (character, source_width) = self.next_character();
            let decoded_width = character.len_utf8();
            if target < self.decoded_cursor.saturating_add(decoded_width) {
                return character_start;
            }
            self.decoded_cursor += decoded_width;
            self.source_cursor += source_width;
        }
        self.source_cursor.min(self.source.len())
    }

    fn next_character(&self) -> (char, usize) {
        match self.encoding {
            WireEncoding::Utf16Be | WireEncoding::Utf16Le => {
                let little_endian = self.encoding == WireEncoding::Utf16Le;
                let first = read_u16(
                    &self.source[self.source_cursor..self.source_cursor + 2],
                    little_endian,
                );
                if (0xd800..=0xdbff).contains(&first) {
                    let second = read_u16(
                        &self.source[self.source_cursor + 2..self.source_cursor + 4],
                        little_endian,
                    );
                    let high = u32::from(first - 0xd800);
                    let low = u32::from(second - 0xdc00);
                    (
                        char::from_u32(0x1_0000 + (high << 10) + low)
                            .expect("validated UTF-16 surrogate pair"),
                        4,
                    )
                } else {
                    (
                        char::from_u32(u32::from(first)).expect("validated UTF-16 scalar"),
                        2,
                    )
                }
            }
            WireEncoding::Utf32Be | WireEncoding::Utf32Le => {
                let bytes: [u8; 4] = self.source[self.source_cursor..self.source_cursor + 4]
                    .try_into()
                    .expect("validated UTF-32 code unit");
                let scalar = if self.encoding == WireEncoding::Utf32Le {
                    u32::from_le_bytes(bytes)
                } else {
                    u32::from_be_bytes(bytes)
                };
                (char::from_u32(scalar).expect("validated UTF-32 scalar"), 4)
            }
            WireEncoding::Utf8 => unreachable!("UTF-8 uses a constant shift map"),
        }
    }
}

#[derive(Debug)]
struct DecodedInput<'source> {
    bytes: Cow<'source, [u8]>,
    positions: PositionMap<'source>,
    wire_encoding: WireEncoding,
}

impl<'source> DecodedInput<'source> {
    fn new(source: &'source [u8], backend: BackendKind) -> Result<Self> {
        let (encoding, skip) = detect_encoding(source, backend)?;
        match encoding {
            WireEncoding::Utf8 => {
                let bytes = &source[skip..];
                if backend == BackendKind::Pure {
                    std::str::from_utf8(bytes).map_err(|_| {
                        error_at(
                            backend,
                            ErrorKind::InvalidString,
                            skip,
                            "XML input is not valid UTF-8",
                        )
                    })?;
                }
                Ok(Self {
                    bytes: Cow::Borrowed(bytes),
                    positions: PositionMap::Shift(skip),
                    wire_encoding: encoding,
                })
            }
            WireEncoding::Utf16Be | WireEncoding::Utf16Le => {
                let little_endian = encoding == WireEncoding::Utf16Le;
                let bytes = transcode_utf16(source, skip, little_endian, backend)?;
                Ok(Self {
                    bytes: Cow::Owned(bytes),
                    positions: PositionMap::Transcoded(RefCell::new(TranscodedPositionMap::new(
                        source, encoding, skip,
                    ))),
                    wire_encoding: encoding,
                })
            }
            WireEncoding::Utf32Be | WireEncoding::Utf32Le => {
                let little_endian = encoding == WireEncoding::Utf32Le;
                let bytes = transcode_utf32(source, skip, little_endian, backend)?;
                Ok(Self {
                    bytes: Cow::Owned(bytes),
                    positions: PositionMap::Transcoded(RefCell::new(TranscodedPositionMap::new(
                        source, encoding, skip,
                    ))),
                    wire_encoding: encoding,
                })
            }
        }
    }

    fn source_offset(&self, decoded_offset: usize) -> usize {
        match &self.positions {
            PositionMap::Shift(shift) => shift.saturating_add(decoded_offset),
            PositionMap::Transcoded(positions) => {
                positions.borrow_mut().source_offset(decoded_offset)
            }
        }
    }

    fn source_text(&self, range: Range<usize>) -> Option<TextContent> {
        match self.wire_encoding {
            WireEncoding::Utf8 => Some(TextContent::Utf8Source(
                self.source_offset(range.start)..self.source_offset(range.end),
            )),
            WireEncoding::Utf16Be => Some(TextContent::Utf16BeSource(
                self.source_offset(range.start)..self.source_offset(range.end),
            )),
            WireEncoding::Utf16Le => Some(TextContent::Utf16LeSource(
                self.source_offset(range.start)..self.source_offset(range.end),
            )),
            WireEncoding::Utf32Be | WireEncoding::Utf32Le => None,
        }
    }
}

fn detect_encoding(source: &[u8], backend: BackendKind) -> Result<(WireEncoding, usize)> {
    if source.starts_with(&[0x00, 0x00, 0xfe, 0xff]) {
        return Ok((WireEncoding::Utf32Be, 4));
    }
    if source.starts_with(&[0xff, 0xfe, 0x00, 0x00]) {
        return Ok((WireEncoding::Utf32Le, 4));
    }
    if source.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Ok((WireEncoding::Utf8, 3));
    }
    if source.starts_with(&[0xfe, 0xff]) {
        return Ok((WireEncoding::Utf16Be, 2));
    }
    if source.starts_with(&[0xff, 0xfe]) {
        return Ok((WireEncoding::Utf16Le, 2));
    }

    // XML's explicit BOM-less Unicode signatures. The historical reader used
    // a broader host-dependent two-byte heuristic; explicit endianness keeps
    // this safe implementation deterministic across targets.
    if source.starts_with(&[0x00, 0x00, 0x00, b'<']) {
        return Ok((WireEncoding::Utf32Be, 0));
    }
    if source.starts_with(&[b'<', 0x00, 0x00, 0x00]) {
        return Ok((WireEncoding::Utf32Le, 0));
    }
    if source.starts_with(&[0x00, b'<']) {
        return Ok((WireEncoding::Utf16Be, 0));
    }
    if source.starts_with(&[b'<', 0x00]) {
        return Ok((WireEncoding::Utf16Le, 0));
    }

    let Some(name) = declared_ascii_encoding(source) else {
        return Ok((WireEncoding::Utf8, 0));
    };
    let normalized = name.to_ascii_lowercase();
    let encoding = match normalized.as_str() {
        "utf-8" | "utf8" => WireEncoding::Utf8,
        "utf-16be" => WireEncoding::Utf16Be,
        "utf-16le" => WireEncoding::Utf16Le,
        // Without a BOM the XML convention is big endian.  Properly encoded
        // UTF-16 input will normally have been identified by its signature.
        "utf-16" | "utf16" => WireEncoding::Utf16Be,
        "utf-32be" => WireEncoding::Utf32Be,
        "utf-32le" => WireEncoding::Utf32Le,
        "utf-32" | "utf32" => WireEncoding::Utf32Be,
        _ => {
            return Err(error_at(
                backend,
                ErrorKind::InvalidString,
                0,
                format!("unsupported XML encoding {name:?}"),
            ));
        }
    };
    Ok((encoding, 0))
}

fn declared_ascii_encoding(source: &[u8]) -> Option<String> {
    if !source.starts_with(b"<?xml") {
        return None;
    }
    let declaration_end = find_bytes(source, 5, b"?>").unwrap_or(source.len());
    let declaration = &source[5..declaration_end];
    let marker = b"encoding";
    let start = declaration
        .windows(marker.len())
        .position(|window| window == marker)?;
    let mut cursor = start + marker.len();
    while declaration
        .get(cursor)
        .copied()
        .is_some_and(is_xml_whitespace)
    {
        cursor += 1;
    }
    if declaration.get(cursor) != Some(&b'=') {
        return None;
    }
    cursor += 1;
    while declaration
        .get(cursor)
        .copied()
        .is_some_and(is_xml_whitespace)
    {
        cursor += 1;
    }
    let quote = *declaration.get(cursor)?;
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    cursor += 1;
    let end = declaration[cursor..]
        .iter()
        .position(|byte| *byte == quote)?
        + cursor;
    std::str::from_utf8(&declaration[cursor..end])
        .ok()
        .map(ToOwned::to_owned)
}

fn transcode_utf16(
    source: &[u8],
    skip: usize,
    little_endian: bool,
    backend: BackendKind,
) -> Result<Vec<u8>> {
    let payload = &source[skip..];
    if payload.len() % 2 != 0 {
        return Err(error_at(
            backend,
            ErrorKind::InvalidString,
            source.len().saturating_sub(1),
            "UTF-16 XML input has an odd byte length",
        ));
    }

    let mut bytes = Vec::new();
    let mut cursor = 0;
    while cursor < payload.len() {
        let unit_start = skip + cursor;
        let first = read_u16(&payload[cursor..cursor + 2], little_endian);
        cursor += 2;
        let scalar = if (0xd800..=0xdbff).contains(&first) {
            if cursor + 2 > payload.len() {
                return Err(error_at(
                    backend,
                    ErrorKind::InvalidString,
                    unit_start,
                    "unterminated UTF-16 surrogate pair",
                ));
            }
            let second = read_u16(&payload[cursor..cursor + 2], little_endian);
            if !(0xdc00..=0xdfff).contains(&second) {
                return Err(error_at(
                    backend,
                    ErrorKind::InvalidString,
                    skip + cursor,
                    "invalid UTF-16 low surrogate",
                ));
            }
            cursor += 2;
            let high = u32::from(first - 0xd800);
            let low = u32::from(second - 0xdc00);
            0x1_0000 + (high << 10) + low
        } else if (0xdc00..=0xdfff).contains(&first) {
            return Err(error_at(
                backend,
                ErrorKind::InvalidString,
                unit_start,
                "unexpected UTF-16 low surrogate",
            ));
        } else {
            u32::from(first)
        };
        let character = char::from_u32(scalar).ok_or_else(|| {
            error_at(
                backend,
                ErrorKind::InvalidString,
                unit_start,
                "invalid Unicode scalar in UTF-16 XML input",
            )
        })?;
        push_transcoded_character(&mut bytes, character, backend, unit_start)?;
    }
    Ok(bytes)
}

fn transcode_utf32(
    source: &[u8],
    skip: usize,
    little_endian: bool,
    backend: BackendKind,
) -> Result<Vec<u8>> {
    let payload = &source[skip..];
    if payload.len() % 4 != 0 {
        return Err(error_at(
            backend,
            ErrorKind::InvalidString,
            source.len().saturating_sub(payload.len() % 4),
            "UTF-32 XML input has a partial code unit",
        ));
    }

    let mut bytes = Vec::new();
    for (index, chunk) in payload.chunks_exact(4).enumerate() {
        let source_start = skip + index * 4;
        let scalar = if little_endian {
            u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
        } else {
            u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
        };
        let character = char::from_u32(scalar).ok_or_else(|| {
            error_at(
                backend,
                ErrorKind::InvalidString,
                source_start,
                "invalid Unicode scalar in UTF-32 XML input",
            )
        })?;
        push_transcoded_character(&mut bytes, character, backend, source_start)?;
    }
    Ok(bytes)
}

fn read_u16(bytes: &[u8], little_endian: bool) -> u16 {
    if little_endian {
        u16::from_le_bytes([bytes[0], bytes[1]])
    } else {
        u16::from_be_bytes([bytes[0], bytes[1]])
    }
}

fn push_transcoded_character(
    bytes: &mut Vec<u8>,
    character: char,
    backend: BackendKind,
    source_offset: usize,
) -> Result<()> {
    let mut encoded = [0; 4];
    let encoded = character.encode_utf8(&mut encoded).as_bytes();
    if bytes.capacity().saturating_sub(bytes.len()) < encoded.len() {
        bytes.try_reserve(encoded.len()).map_err(|_| {
            error_at(
                backend,
                ErrorKind::LimitExceeded,
                source_offset,
                "UTF XML transcoding allocation failed",
            )
        })?;
    }
    bytes.extend_from_slice(encoded);
    Ok(())
}

fn error_at(
    backend: BackendKind,
    kind: ErrorKind,
    offset: usize,
    message: impl Into<Cow<'static, str>>,
) -> Error {
    Error::at(kind, Format::Xml, offset)
        .with_backend(backend)
        .with_message(message)
}

fn find_bytes(haystack: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(start.min(haystack.len()));
    }
    haystack
        .get(start..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|position| start + position)
}

fn is_xml_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Tag {
    Plist,
    Array,
    Dict,
    Key,
    String,
    Data,
    Date,
    Real,
    Integer,
    True,
    False,
    Unknown,
}

impl Tag {
    fn from_bytes(name: &[u8]) -> Self {
        match name {
            b"plist" => Self::Plist,
            b"array" => Self::Array,
            b"dict" => Self::Dict,
            b"key" => Self::Key,
            b"string" => Self::String,
            b"data" => Self::Data,
            b"date" => Self::Date,
            b"real" => Self::Real,
            b"integer" => Self::Integer,
            b"true" => Self::True,
            b"false" => Self::False,
            _ => Self::Unknown,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Plist => "plist",
            Self::Array => "array",
            Self::Dict => "dict",
            Self::Key => "key",
            Self::String => "string",
            Self::Data => "data",
            Self::Date => "date",
            Self::Real => "real",
            Self::Integer => "integer",
            Self::True => "true",
            Self::False => "false",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug)]
struct StartTag {
    tag: Tag,
    name: Range<usize>,
    attributes: Range<usize>,
    self_closing: bool,
    offset: usize,
}

impl StartTag {
    fn has_attributes(&self, bytes: &[u8]) -> bool {
        bytes[self.attributes.clone()]
            .iter()
            .copied()
            .any(|byte| !is_xml_whitespace(byte))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Context {
    Root,
    Value,
    ArrayMember,
    DictionaryKey,
    DictionaryValue,
}

#[derive(Debug)]
struct ParsedElement {
    id: NodeId,
    key: Option<Vec<u16>>,
}

#[derive(Debug)]
enum TextContent {
    Utf8Source(Range<usize>),
    Utf16BeSource(Range<usize>),
    Utf16LeSource(Range<usize>),
    Owned(String),
}

impl TextContent {
    fn as_str<'a>(&'a self, decoded: &'a [u8], decoded_range: &Range<usize>) -> Option<&'a str> {
        match self {
            Self::Owned(value) => Some(value),
            Self::Utf8Source(_) | Self::Utf16BeSource(_) | Self::Utf16LeSource(_) => {
                std::str::from_utf8(&decoded[decoded_range.clone()]).ok()
            }
        }
    }

    fn to_utf16(&self, decoded: &[u8], decoded_range: &Range<usize>) -> Option<Vec<u16>> {
        Some(
            self.as_str(decoded, decoded_range)?
                .encode_utf16()
                .collect(),
        )
    }

    fn storage_len(&self) -> usize {
        match self {
            Self::Utf8Source(range) | Self::Utf16BeSource(range) | Self::Utf16LeSource(range) => {
                range.len()
            }
            Self::Owned(value) => value.len(),
        }
    }

    fn into_node(self, builder: &mut ParsedBuilder) -> NodeId {
        match self {
            Self::Utf8Source(range) => builder.source_string(range),
            Self::Utf16BeSource(range) => builder.source_utf16be_string(range),
            Self::Utf16LeSource(range) => builder.source_utf16le_string(range),
            Self::Owned(value) => builder.owned_string(value),
        }
    }
}

#[derive(Debug)]
struct NormalizedText(String);

impl NormalizedText {
    fn new() -> Self {
        Self(String::new())
    }

    fn storage_len(&self) -> usize {
        self.0.len()
    }

    fn push_str(&mut self, value: &str) {
        self.0.push_str(value);
    }

    fn push_char(&mut self, value: char) {
        self.0.push(value);
    }

    fn into_content(self) -> TextContent {
        TextContent::Owned(self.0)
    }
}

#[derive(Clone, Copy)]
enum EntityCharacter {
    Scalar(char),
    Empty,
}

struct XmlParser<'input, 'options> {
    bytes: &'input [u8],
    decoded: &'input DecodedInput<'input>,
    cursor: usize,
    builder: ParsedBuilder,
    options: &'options ParseOptions,
    profile: Profile,
    object_count: usize,
}

impl<'input, 'options> XmlParser<'input, 'options> {
    fn new(
        source_len: usize,
        decoded: &'input DecodedInput<'input>,
        options: &'options ParseOptions,
        profile: Profile,
    ) -> Self {
        Self {
            bytes: &decoded.bytes,
            decoded,
            cursor: 0,
            builder: ParsedBuilder::new(
                source_len,
                Format::Xml,
                profile.backend(),
                options.limits().max_objects(),
            ),
            options,
            profile,
            object_count: 0,
        }
    }

    fn parse_document(mut self) -> Result<Parsed> {
        self.skip_misc(true)?;
        if self.cursor >= self.bytes.len() {
            return Err(self.error(ErrorKind::Malformed, "no XML content found"));
        }

        let start = self.parse_start_tag()?;
        if self.profile == Profile::Pure && start.tag != Tag::Plist {
            return Err(self.error_at_decoded(
                ErrorKind::Malformed,
                start.offset,
                "the public XML property-list grammar requires a <plist> root",
            ));
        }
        let root = self.parse_element_after_start(start, 0, Context::Root)?.id;

        if self.profile == Profile::Pure {
            self.skip_misc(false)?;
            if self.cursor != self.bytes.len() {
                return Err(self.error(
                    ErrorKind::Malformed,
                    "content appears after the root property-list element",
                ));
            }
        }
        // CoreFoundation intentionally returns immediately after the first
        // complete root element, even when arbitrary bytes follow it.
        self.builder.finish(root)
    }

    fn parse_element(&mut self, depth: usize, context: Context) -> Result<ParsedElement> {
        let start = self.parse_start_tag()?;
        self.parse_element_after_start(start, depth, context)
    }

    fn parse_element_after_start(
        &mut self,
        start: StartTag,
        depth: usize,
        context: Context,
    ) -> Result<ParsedElement> {
        if start.tag == Tag::Unknown {
            let name = String::from_utf8_lossy(&self.bytes[start.name.clone()]);
            return Err(self.error_at_decoded(
                ErrorKind::UnsupportedObject,
                start.offset,
                format!("unknown property-list XML tag <{name}>"),
            ));
        }

        if context == Context::DictionaryKey && start.tag != Tag::Key {
            return Err(self.error_at_decoded(
                ErrorKind::InvalidKey,
                start.offset,
                "a dictionary entry must begin with <key>",
            ));
        }
        if self.profile == Profile::Pure {
            if start.tag == Tag::Key && context != Context::DictionaryKey {
                return Err(self.error_at_decoded(
                    ErrorKind::InvalidKey,
                    start.offset,
                    "<key> is only valid in dictionary key position",
                ));
            }
            if start.tag == Tag::Plist && context != Context::Root {
                return Err(self.error_at_decoded(
                    ErrorKind::Malformed,
                    start.offset,
                    "a nested <plist> element is not part of the public grammar",
                ));
            }
            self.validate_public_attributes(&start)?;
        }

        if start.tag == Tag::Plist {
            return self.parse_plist(start, depth);
        }

        self.reserve_object(start.offset)?;
        match start.tag {
            Tag::Array => self.parse_array(start, depth),
            Tag::Dict => self.parse_dictionary(start, depth),
            Tag::Key => self.parse_string(start, true),
            Tag::String => self.parse_string(start, false),
            Tag::Data => self.parse_data(start),
            Tag::Date => self.parse_date(start),
            Tag::Real => self.parse_real(start),
            Tag::Integer => self.parse_integer(start),
            Tag::True => self.parse_boolean(start, true),
            Tag::False => self.parse_boolean(start, false),
            Tag::Plist | Tag::Unknown => unreachable!("handled above"),
        }
    }

    fn parse_plist(&mut self, start: StartTag, depth: usize) -> Result<ParsedElement> {
        if start.self_closing {
            return Err(self.error_at_decoded(
                ErrorKind::Malformed,
                start.offset,
                "an empty <plist/> element has no root object",
            ));
        }
        self.skip_misc(false)?;
        if self.at_close_tag(Tag::Plist) {
            return Err(self.error(ErrorKind::Malformed, "an empty <plist> has no root object"));
        }
        let value = self.parse_element(depth, Context::Value)?;
        self.skip_misc(false)?;
        if !self.at_close_tag(Tag::Plist) {
            return Err(self.error(
                ErrorKind::Malformed,
                "a <plist> element may contain exactly one object",
            ));
        }
        self.expect_close_tag(Tag::Plist)?;
        Ok(ParsedElement {
            id: value.id,
            key: None,
        })
    }

    fn parse_array(&mut self, start: StartTag, depth: usize) -> Result<ParsedElement> {
        if start.self_closing {
            let id = self.builder.array(Vec::new());
            return Ok(ParsedElement { id, key: None });
        }
        let mut values = Vec::new();
        loop {
            self.skip_misc(false)?;
            if self.at_close_tag(Tag::Array) {
                self.expect_close_tag(Tag::Array)?;
                break;
            }
            if self.cursor >= self.bytes.len() {
                return Err(self.error(ErrorKind::Malformed, "unterminated <array> element"));
            }
            if values.len() >= self.options.limits().max_container_len() {
                return Err(self.error(
                    ErrorKind::LimitExceeded,
                    "array exceeds the configured member limit",
                ));
            }
            let child_depth = self.child_depth(depth, self.cursor)?;
            values.push(self.parse_element(child_depth, Context::ArrayMember)?.id);
        }
        let id = self.builder.array(values);
        Ok(ParsedElement { id, key: None })
    }

    fn parse_dictionary(&mut self, start: StartTag, depth: usize) -> Result<ParsedElement> {
        if start.self_closing {
            let id = self.builder.dictionary(Vec::new());
            return Ok(ParsedElement { id, key: None });
        }
        let mut entries = Vec::new();
        let mut key_indices = HashMap::<Vec<u16>, usize>::new();
        let mut parsed_entries = 0usize;

        loop {
            self.skip_misc(false)?;
            if self.at_close_tag(Tag::Dict) {
                self.expect_close_tag(Tag::Dict)?;
                break;
            }
            if self.cursor >= self.bytes.len() {
                return Err(self.error(ErrorKind::Malformed, "unterminated <dict> element"));
            }
            if parsed_entries >= self.options.limits().max_container_len() {
                return Err(self.error(
                    ErrorKind::LimitExceeded,
                    "dictionary exceeds the configured entry limit",
                ));
            }

            let child_depth = self.child_depth(depth, self.cursor)?;
            let key_element = self.parse_element(child_depth, Context::DictionaryKey)?;
            let key = key_element
                .key
                .expect("dictionary-key context only accepts key elements");
            self.skip_misc(false)?;
            if self.cursor >= self.bytes.len() || self.at_close_tag(Tag::Dict) {
                return Err(self.error(
                    ErrorKind::Malformed,
                    "dictionary key has no corresponding value",
                ));
            }
            let value = self
                .parse_element(child_depth, Context::DictionaryValue)?
                .id;
            parsed_entries += 1;

            if let Some(index) = key_indices.get(&key).copied() {
                entries[index] = (key_element.id, value);
            } else {
                key_indices.insert(key, entries.len());
                entries.push((key_element.id, value));
            }
        }

        if self.profile.is_cf() && entries.len() == 1 && key_indices.contains_key(CF_UID_KEY) {
            if let Some(uid) = self.builder.cf_number_sint32_bits(entries[0].1) {
                let id = self.builder.uid(uid);
                return Ok(ParsedElement { id, key: None });
            }
        }

        let id = self.builder.dictionary(entries);
        Ok(ParsedElement { id, key: None })
    }

    fn parse_string(&mut self, start: StartTag, is_key: bool) -> Result<ParsedElement> {
        let (content, decoded_range) = if start.self_closing {
            let range = self.cursor..self.cursor;
            (self.direct_text(range.clone()), range)
        } else {
            self.parse_text_content(start.tag)?
        };
        let key = if is_key {
            Some(
                content
                    .to_utf16(self.bytes, &decoded_range)
                    .ok_or_else(|| {
                        self.error_at_decoded(
                            ErrorKind::InvalidString,
                            decoded_range.start,
                            "string content is not valid UTF-8",
                        )
                    })?,
            )
        } else {
            None
        };
        let id = content.into_node(&mut self.builder);
        Ok(ParsedElement { id, key })
    }

    fn parse_data(&mut self, start: StartTag) -> Result<ParsedElement> {
        if start.self_closing {
            if self.profile.is_cf() {
                return Err(self.error_at_decoded(
                    ErrorKind::InvalidData,
                    start.offset,
                    "CoreFoundation rejects the self-closing <data/> spelling",
                ));
            }
            let id = self.builder.owned_data(Vec::new());
            return Ok(ParsedElement { id, key: None });
        }

        let content = self.raw_content(Tag::Data)?;
        let data = if self.profile.is_cf() {
            self.decode_cf_base64(content.clone())?
        } else {
            self.decode_public_base64(content.clone())?
        };
        let id = self.builder.owned_data(data);
        Ok(ParsedElement { id, key: None })
    }

    fn parse_date(&mut self, start: StartTag) -> Result<ParsedElement> {
        if start.self_closing {
            return Err(self.error_at_decoded(
                ErrorKind::InvalidDate,
                start.offset,
                "an empty <date/> has no value",
            ));
        }
        let content = self.raw_content(Tag::Date)?;
        let text = std::str::from_utf8(&self.bytes[content.clone()]).map_err(|_| {
            self.error_at_decoded(
                ErrorKind::InvalidDate,
                content.start,
                "date content is not valid UTF-8",
            )
        })?;
        let absolute_time = parse_xml_date(text, self.profile).ok_or_else(|| {
            self.error_at_decoded(
                ErrorKind::InvalidDate,
                content.start,
                "date must use the UTC YYYY-MM-DDTHH:MM:SSZ form",
            )
        })?;
        let id = self
            .builder
            .date(Date::from_cf_absolute_time(absolute_time));
        Ok(ParsedElement { id, key: None })
    }

    fn parse_real(&mut self, start: StartTag) -> Result<ParsedElement> {
        if start.self_closing {
            return Err(self.error_at_decoded(
                ErrorKind::InvalidReal,
                start.offset,
                "an empty <real/> has no value",
            ));
        }
        let (content, decoded_range) = self.parse_text_content(Tag::Real)?;
        let text = content.as_str(self.bytes, &decoded_range).ok_or_else(|| {
            self.error_at_decoded(
                ErrorKind::InvalidReal,
                decoded_range.start,
                "real content is not valid UTF-8",
            )
        })?;
        if text.is_empty() {
            return Err(self.error_at_decoded(
                ErrorKind::InvalidReal,
                decoded_range.start,
                "an empty <real> has no value",
            ));
        }
        let value = parse_xml_real(text, self.profile).ok_or_else(|| {
            self.error_at_decoded(
                ErrorKind::InvalidReal,
                decoded_range.start,
                "misformatted floating-point value",
            )
        })?;
        let id = self.builder.real(Real::F64(value));
        Ok(ParsedElement { id, key: None })
    }

    fn parse_integer(&mut self, start: StartTag) -> Result<ParsedElement> {
        if start.self_closing {
            return Err(self.error_at_decoded(
                ErrorKind::InvalidInteger,
                start.offset,
                "an empty <integer/> has no value",
            ));
        }
        let content = self.raw_content(Tag::Integer)?;
        let value =
            parse_xml_integer(&self.bytes[content.clone()], self.profile).ok_or_else(|| {
                self.error_at_decoded(
                    ErrorKind::InvalidInteger,
                    content.start,
                    "misformatted or out-of-range integer",
                )
            })?;
        let id = self.builder.integer(value);
        Ok(ParsedElement { id, key: None })
    }

    fn parse_boolean(&mut self, start: StartTag, value: bool) -> Result<ParsedElement> {
        if !start.self_closing {
            // CoreFoundation accepts paired empty tags, but not even whitespace
            // between them.  The public EMPTY grammar has the same result.
            self.expect_close_tag(start.tag)?;
        }
        let id = self.builder.boolean(value);
        Ok(ParsedElement { id, key: None })
    }

    fn parse_text_content(&mut self, tag: Tag) -> Result<(TextContent, Range<usize>)> {
        let content_start = self.cursor;
        let mut mark = self.cursor;
        let mut normalized: Option<NormalizedText> = None;

        while self.cursor < self.bytes.len() {
            match self.bytes[self.cursor] {
                b'&' => {
                    self.copy_pending_text(&mut normalized, mark, self.cursor)?;
                    self.parse_entity(&mut normalized)?;
                    mark = self.cursor;
                }
                b'<' if self.bytes[self.cursor..].starts_with(b"<![CDATA[") => {
                    self.copy_pending_text(&mut normalized, mark, self.cursor)?;
                    self.parse_cdata(&mut normalized)?;
                    mark = self.cursor;
                }
                b'<' => break,
                _ => self.cursor += 1,
            }
        }

        if self.cursor >= self.bytes.len() {
            return Err(self.error(
                ErrorKind::Malformed,
                format!("unterminated <{}> element", tag.name()),
            ));
        }
        let content_end = self.cursor;
        if let Some(value) = normalized.as_mut() {
            self.append_normalized(value, &self.bytes[mark..content_end])?;
        }
        self.expect_close_tag(tag)?;

        let decoded_range = content_start..content_end;
        let content = match normalized {
            Some(value) => value.into_content(),
            None => self.direct_text(decoded_range.clone()),
        };
        let storage_len = content.storage_len();
        if storage_len > self.options.limits().max_string_bytes() {
            return Err(self.error_at_decoded(
                ErrorKind::LimitExceeded,
                content_start,
                "string exceeds the configured storage-byte limit",
            ));
        }
        let text = content.as_str(self.bytes, &decoded_range).ok_or_else(|| {
            self.error_at_decoded(
                ErrorKind::InvalidString,
                content_start,
                "string content is not valid UTF-8",
            )
        })?;
        if self.profile == Profile::Pure && !text.chars().all(is_public_xml_character) {
            return Err(self.error_at_decoded(
                ErrorKind::InvalidString,
                content_start,
                "string contains a character forbidden by XML 1.0",
            ));
        }
        Ok((content, decoded_range))
    }

    fn direct_text(&self, decoded_range: Range<usize>) -> TextContent {
        self.decoded
            .source_text(decoded_range.clone())
            .unwrap_or_else(|| {
                TextContent::Owned(
                    std::str::from_utf8(&self.bytes[decoded_range])
                        .expect("decoded XML must remain valid UTF-8")
                        .to_owned(),
                )
            })
    }

    fn copy_pending_text(
        &self,
        normalized: &mut Option<NormalizedText>,
        start: usize,
        end: usize,
    ) -> Result<()> {
        let value = normalized.get_or_insert_with(NormalizedText::new);
        self.append_normalized(value, &self.bytes[start..end])
    }

    fn append_normalized(&self, output: &mut NormalizedText, bytes: &[u8]) -> Result<()> {
        let text = std::str::from_utf8(bytes).map_err(|_| {
            self.error(
                ErrorKind::InvalidString,
                "normalized string content is not valid UTF-8",
            )
        })?;
        output.push_str(text);
        self.check_normalized_limit(output)
    }

    fn check_normalized_limit(&self, output: &NormalizedText) -> Result<()> {
        let length = output.storage_len();
        if length > self.options.limits().max_string_bytes() {
            return Err(self.error(
                ErrorKind::LimitExceeded,
                "string exceeds the configured storage-byte limit",
            ));
        }
        Ok(())
    }

    fn parse_entity(&mut self, normalized: &mut Option<NormalizedText>) -> Result<()> {
        let entity_offset = self.cursor;
        self.cursor += 1;
        let semicolon = self.bytes[self.cursor..]
            .iter()
            .position(|byte| *byte == b';')
            .map(|relative| self.cursor + relative)
            .ok_or_else(|| {
                self.error_at_decoded(
                    ErrorKind::InvalidString,
                    entity_offset,
                    "unterminated XML entity reference",
                )
            })?;
        let entity = &self.bytes[self.cursor..semicolon];
        let character = match entity {
            b"lt" => EntityCharacter::Scalar('<'),
            b"gt" => EntityCharacter::Scalar('>'),
            b"amp" => EntityCharacter::Scalar('&'),
            b"apos" => EntityCharacter::Scalar('\''),
            b"quot" => EntityCharacter::Scalar('"'),
            _ if entity.starts_with(b"#") => {
                let (radix, digits) = if entity.starts_with(b"#x") {
                    (16_u16, &entity[2..])
                } else {
                    (10_u16, &entity[1..])
                };
                if self.profile.is_cf() {
                    let mut unit = 0_u16;
                    for byte in digits {
                        let digit = match byte {
                            b'0'..=b'9' => u16::from(byte - b'0'),
                            b'a'..=b'f' if radix == 16 => u16::from(byte - b'a' + 10),
                            b'A'..=b'F' if radix == 16 => u16::from(byte - b'A' + 10),
                            _ => {
                                return Err(self.error_at_decoded(
                                    ErrorKind::InvalidString,
                                    entity_offset,
                                    "invalid numeric XML entity reference",
                                ));
                            }
                        };
                        unit = unit.wrapping_mul(radix).wrapping_add(digit);
                    }
                    char::from_u32(u32::from(unit))
                        .map(EntityCharacter::Scalar)
                        .unwrap_or(EntityCharacter::Empty)
                } else {
                    if digits.is_empty() || digits.len() > 8 {
                        return Err(self.error_at_decoded(
                            ErrorKind::InvalidString,
                            entity_offset,
                            "invalid numeric XML entity reference",
                        ));
                    }
                    let digits = std::str::from_utf8(digits)
                        .expect("numeric entity bytes are a subset of decoded UTF-8");
                    let scalar = u32::from_str_radix(digits, u32::from(radix)).map_err(|_| {
                        self.error_at_decoded(
                            ErrorKind::InvalidString,
                            entity_offset,
                            "invalid numeric XML entity reference",
                        )
                    })?;
                    EntityCharacter::Scalar(char::from_u32(scalar).ok_or_else(|| {
                        self.error_at_decoded(
                            ErrorKind::InvalidString,
                            entity_offset,
                            "numeric XML entity is not a Unicode scalar",
                        )
                    })?)
                }
            }
            _ => {
                return Err(self.error_at_decoded(
                    ErrorKind::InvalidString,
                    entity_offset,
                    "unknown XML entity reference",
                ));
            }
        };
        if self.profile == Profile::Pure {
            let EntityCharacter::Scalar(character) = character else {
                unreachable!("pure numeric entities always produce a scalar")
            };
            if !is_public_xml_character(character) {
                return Err(self.error_at_decoded(
                    ErrorKind::InvalidString,
                    entity_offset,
                    "numeric XML entity denotes a character forbidden by XML 1.0",
                ));
            }
        }
        let output = normalized.get_or_insert_with(NormalizedText::new);
        match character {
            EntityCharacter::Scalar(character) => output.push_char(character),
            // The historical reader converts each numeric entity through a
            // one-unit CFString. An unpaired surrogate converts to zero UTF-8
            // bytes, so it disappears instead of surviving as ill-formed text.
            EntityCharacter::Empty => {}
        }
        self.check_normalized_limit(output)?;
        self.cursor = semicolon + 1;
        Ok(())
    }

    fn parse_cdata(&mut self, normalized: &mut Option<NormalizedText>) -> Result<()> {
        let cdata_offset = self.cursor;
        let content_start = self.cursor + b"<![CDATA[".len();
        let end = find_bytes(self.bytes, content_start, b"]]>").ok_or_else(|| {
            self.error_at_decoded(
                ErrorKind::InvalidString,
                cdata_offset,
                "unterminated CDATA section",
            )
        })?;
        let output = normalized.get_or_insert_with(NormalizedText::new);
        self.append_normalized(output, &self.bytes[content_start..end])?;
        self.cursor = end + 3;
        Ok(())
    }

    fn raw_content(&mut self, tag: Tag) -> Result<Range<usize>> {
        let start = self.cursor;
        let relative_end = self.bytes[start..]
            .iter()
            .position(|byte| *byte == b'<')
            .ok_or_else(|| {
                self.error(
                    ErrorKind::Malformed,
                    format!("unterminated <{}> element", tag.name()),
                )
            })?;
        self.cursor = start + relative_end;
        let range = start..self.cursor;
        self.expect_close_tag(tag)?;
        Ok(range)
    }

    fn parse_start_tag(&mut self) -> Result<StartTag> {
        let offset = self.cursor;
        if self.bytes.get(self.cursor) != Some(&b'<') {
            return Err(self.error(ErrorKind::Malformed, "expected an XML start tag"));
        }
        self.cursor += 1;
        if self.cursor >= self.bytes.len() || matches!(self.bytes[self.cursor], b'/' | b'!' | b'?')
        {
            return Err(self.error_at_decoded(
                ErrorKind::Malformed,
                offset,
                "expected an XML element name",
            ));
        }

        let name_start = self.cursor;
        while self.cursor < self.bytes.len()
            && !matches!(self.bytes[self.cursor], b'>' | b' ' | b'\t' | b'\n' | b'\r')
            && (self.profile.is_cf() || self.bytes[self.cursor] != b'/')
        {
            self.cursor += 1;
        }
        let raw_name_end = self.cursor;
        if name_start == raw_name_end {
            return Err(self.error_at_decoded(
                ErrorKind::Malformed,
                offset,
                "empty XML element name",
            ));
        }

        // Deliberately mirror the property-list parser rather than a complete
        // XML tokenizer: attributes are opaque and the first '>' ends a tag.
        let close = self.bytes[self.cursor..]
            .iter()
            .position(|byte| *byte == b'>')
            .map(|relative| self.cursor + relative)
            .ok_or_else(|| {
                self.error_at_decoded(ErrorKind::Malformed, offset, "unterminated XML start tag")
            })?;
        // The historical reader recognizes an empty element only when '/'
        // immediately precedes '>'; whitespace after the slash changes it to
        // an ordinary paired start tag.
        let self_closing = close > name_start && self.bytes[close - 1] == b'/';
        let mut significant_end = close;
        if self_closing {
            significant_end -= 1;
        }
        while significant_end > raw_name_end && is_xml_whitespace(self.bytes[significant_end - 1]) {
            significant_end -= 1;
        }

        // CoreFoundation includes '/' in a directly adjacent malformed name,
        // except for the one trailing slash that marks an empty element.
        let name_end = if self.profile.is_cf()
            && self_closing
            && raw_name_end == close
            && raw_name_end > name_start
        {
            raw_name_end - 1
        } else {
            raw_name_end
        };

        let name = name_start..name_end;
        let tag = Tag::from_bytes(&self.bytes[name.clone()]);
        let attributes = name_end..significant_end;
        self.cursor = close + 1;
        Ok(StartTag {
            tag,
            name,
            attributes,
            self_closing,
            offset,
        })
    }

    fn expect_close_tag(&mut self, expected: Tag) -> Result<()> {
        let offset = self.cursor;
        if !self.bytes[self.cursor..].starts_with(b"</") {
            return Err(self.error_at_decoded(
                ErrorKind::Malformed,
                offset,
                format!("expected </{}>", expected.name()),
            ));
        }
        self.cursor += 2;
        let name = expected.name().as_bytes();
        if !self.bytes[self.cursor..].starts_with(name) {
            return Err(self.error_at_decoded(
                ErrorKind::Malformed,
                offset,
                format!("close tag does not match <{}>", expected.name()),
            ));
        }
        self.cursor += name.len();
        while self
            .bytes
            .get(self.cursor)
            .copied()
            .is_some_and(is_xml_whitespace)
        {
            self.cursor += 1;
        }
        if self.bytes.get(self.cursor) != Some(&b'>') {
            return Err(self.error_at_decoded(
                ErrorKind::Malformed,
                offset,
                format!("malformed </{}> close tag", expected.name()),
            ));
        }
        self.cursor += 1;
        Ok(())
    }

    fn at_close_tag(&self, expected: Tag) -> bool {
        let suffix = &self.bytes[self.cursor..];
        if !suffix.starts_with(b"</") {
            return false;
        }
        let name = expected.name().as_bytes();
        suffix.get(2..).is_some_and(|rest| {
            rest.starts_with(name)
                && rest
                    .get(name.len())
                    .copied()
                    .is_some_and(|byte| byte == b'>' || is_xml_whitespace(byte))
        })
    }

    fn skip_misc(&mut self, allow_doctype: bool) -> Result<()> {
        loop {
            while self
                .bytes
                .get(self.cursor)
                .copied()
                .is_some_and(is_xml_whitespace)
            {
                self.cursor += 1;
            }
            if self.bytes[self.cursor..].starts_with(b"<!--") {
                self.skip_comment()?;
            } else if self.bytes[self.cursor..].starts_with(b"<?") {
                self.skip_processing_instruction()?;
            } else if self.bytes[self.cursor..].starts_with(b"<!DOCTYPE") {
                if !allow_doctype {
                    return Err(self.error(
                        ErrorKind::Malformed,
                        "a document type declaration is only valid before the root",
                    ));
                }
                self.skip_doctype()?;
            } else {
                return Ok(());
            }
        }
    }

    fn skip_comment(&mut self) -> Result<()> {
        let offset = self.cursor;
        let end = find_bytes(self.bytes, self.cursor + 4, b"-->").ok_or_else(|| {
            self.error_at_decoded(ErrorKind::Malformed, offset, "unterminated XML comment")
        })?;
        self.cursor = end + 3;
        Ok(())
    }

    fn skip_processing_instruction(&mut self) -> Result<()> {
        let offset = self.cursor;
        let end = find_bytes(self.bytes, self.cursor + 2, b"?>").ok_or_else(|| {
            self.error_at_decoded(
                ErrorKind::Malformed,
                offset,
                "unterminated XML processing instruction",
            )
        })?;
        self.cursor = end + 2;
        Ok(())
    }

    fn skip_doctype(&mut self) -> Result<()> {
        let offset = self.cursor;
        self.cursor += b"<!DOCTYPE".len();
        while self.cursor < self.bytes.len() {
            let byte = self.bytes[self.cursor];
            match byte {
                b'[' => {
                    return Err(self.error_at_decoded(
                        ErrorKind::Malformed,
                        self.cursor,
                        "inline DTD subsets are not accepted",
                    ));
                }
                b'>' => {
                    self.cursor += 1;
                    return Ok(());
                }
                _ => self.cursor += 1,
            }
        }
        Err(self.error_at_decoded(
            ErrorKind::Malformed,
            offset,
            "unterminated document type declaration",
        ))
    }

    fn validate_public_attributes(&self, start: &StartTag) -> Result<()> {
        if start.tag == Tag::Plist {
            if !valid_public_plist_attributes(&self.bytes[start.attributes.clone()]) {
                return Err(self.error_at_decoded(
                    ErrorKind::Malformed,
                    start.offset,
                    "<plist> must have exactly version=\"1.0\"",
                ));
            }
        } else if start.has_attributes(self.bytes) {
            return Err(self.error_at_decoded(
                ErrorKind::Malformed,
                start.offset,
                "public property-list value elements do not have attributes",
            ));
        }
        Ok(())
    }

    fn reserve_object(&mut self, decoded_offset: usize) -> Result<()> {
        if self.object_count >= self.options.limits().max_objects() {
            return Err(self.error_at_decoded(
                ErrorKind::LimitExceeded,
                decoded_offset,
                "XML object count exceeds the configured limit",
            ));
        }
        self.object_count += 1;
        Ok(())
    }

    fn child_depth(&self, depth: usize, decoded_offset: usize) -> Result<usize> {
        let next = depth.checked_add(1).ok_or_else(|| {
            self.error_at_decoded(
                ErrorKind::LimitExceeded,
                decoded_offset,
                "XML nesting depth overflowed usize",
            )
        })?;
        let maximum = self.options.limits().max_depth().min(CF_MAX_DEPTH);
        if next > maximum {
            return Err(self.error_at_decoded(
                ErrorKind::LimitExceeded,
                decoded_offset,
                "XML nesting exceeds the configured depth limit",
            ));
        }
        Ok(next)
    }

    fn error(&self, kind: ErrorKind, message: impl Into<Cow<'static, str>>) -> Error {
        self.error_at_decoded(kind, self.cursor, message)
    }

    fn error_at_decoded(
        &self,
        kind: ErrorKind,
        decoded_offset: usize,
        message: impl Into<Cow<'static, str>>,
    ) -> Error {
        error_at(
            self.profile.backend(),
            kind,
            self.decoded.source_offset(decoded_offset),
            message,
        )
    }

    fn decode_cf_base64(&self, range: Range<usize>) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        let mut accumulator = 0u32;
        let mut count = 0usize;
        let mut equals = 0usize;

        for (index, byte) in self.bytes[range.clone()].iter().copied().enumerate() {
            if byte >= 0x80 {
                return Err(self.error_at_decoded(
                    ErrorKind::InvalidData,
                    range.start + index,
                    "non-ASCII character in CoreFoundation-style Base64 data",
                ));
            }
            if byte == b'=' {
                equals += 1;
            } else if !byte.is_ascii_whitespace() {
                equals = 0;
            }
            let Some(value) = base64_value(byte) else {
                continue;
            };
            count += 1;
            accumulator = (accumulator << 6) | u32::from(value);
            if count % 4 == 0 {
                let bytes_to_add = 1 + usize::from(equals < 2) + usize::from(equals < 1);
                if output.len().saturating_add(bytes_to_add)
                    > self.options.limits().max_data_bytes()
                {
                    return Err(self.error_at_decoded(
                        ErrorKind::LimitExceeded,
                        range.start + index,
                        "data exceeds the configured decoded-byte limit",
                    ));
                }
                output.push(((accumulator >> 16) & 0xff) as u8);
                if equals < 2 {
                    output.push(((accumulator >> 8) & 0xff) as u8);
                }
                if equals < 1 {
                    output.push((accumulator & 0xff) as u8);
                }
                accumulator = 0;
            }
        }
        // CoreFoundation discards an incomplete trailing quartet.
        Ok(output)
    }

    fn decode_public_base64(&self, range: Range<usize>) -> Result<Vec<u8>> {
        let mut compact = Vec::with_capacity(range.len());
        for (index, byte) in self.bytes[range.clone()].iter().copied().enumerate() {
            if byte.is_ascii_whitespace() {
                continue;
            }
            if base64_value(byte).is_none() {
                return Err(self.error_at_decoded(
                    ErrorKind::InvalidData,
                    range.start + index,
                    "invalid character in Base64 data",
                ));
            }
            compact.push(byte);
        }
        if compact.len() % 4 != 0 {
            return Err(self.error_at_decoded(
                ErrorKind::InvalidData,
                range.start,
                "Base64 data length is not a multiple of four",
            ));
        }

        let mut output = Vec::with_capacity(compact.len() / 4 * 3);
        let quartet_count = compact.len() / 4;
        for (quartet_index, quartet) in compact.chunks_exact(4).enumerate() {
            let last = quartet_index + 1 == quartet_count;
            let padding = usize::from(quartet[3] == b'=') + usize::from(quartet[2] == b'=');
            if padding > 0 && !last
                || quartet[0] == b'='
                || quartet[1] == b'='
                || (quartet[2] == b'=' && quartet[3] != b'=')
            {
                return Err(self.error_at_decoded(
                    ErrorKind::InvalidData,
                    range.start,
                    "misplaced Base64 padding",
                ));
            }
            let a = u32::from(base64_value(quartet[0]).expect("validated Base64 byte"));
            let b = u32::from(base64_value(quartet[1]).expect("validated Base64 byte"));
            let c = u32::from(base64_value(quartet[2]).expect("validated Base64 byte"));
            let d = u32::from(base64_value(quartet[3]).expect("validated Base64 byte"));
            let accumulator = (a << 18) | (b << 12) | (c << 6) | d;
            let bytes_to_add = 3 - padding;
            if output.len().saturating_add(bytes_to_add) > self.options.limits().max_data_bytes() {
                return Err(self.error_at_decoded(
                    ErrorKind::LimitExceeded,
                    range.start,
                    "data exceeds the configured decoded-byte limit",
                ));
            }
            output.push(((accumulator >> 16) & 0xff) as u8);
            if padding < 2 {
                output.push(((accumulator >> 8) & 0xff) as u8);
            }
            if padding < 1 {
                output.push((accumulator & 0xff) as u8);
            }
        }
        Ok(output)
    }
}

fn valid_public_plist_attributes(attributes: &[u8]) -> bool {
    let mut cursor = 0;
    skip_ascii_xml_whitespace(attributes, &mut cursor);
    if !attributes[cursor..].starts_with(b"version") {
        return false;
    }
    cursor += b"version".len();
    skip_ascii_xml_whitespace(attributes, &mut cursor);
    if attributes.get(cursor) != Some(&b'=') {
        return false;
    }
    cursor += 1;
    skip_ascii_xml_whitespace(attributes, &mut cursor);
    let Some(quote @ (b'\'' | b'"')) = attributes.get(cursor).copied() else {
        return false;
    };
    cursor += 1;
    if !attributes[cursor..].starts_with(b"1.0") {
        return false;
    }
    cursor += 3;
    if attributes.get(cursor) != Some(&quote) {
        return false;
    }
    cursor += 1;
    skip_ascii_xml_whitespace(attributes, &mut cursor);
    cursor == attributes.len()
}

fn skip_ascii_xml_whitespace(bytes: &[u8], cursor: &mut usize) {
    while bytes.get(*cursor).copied().is_some_and(is_xml_whitespace) {
        *cursor += 1;
    }
}

fn is_public_xml_character(character: char) -> bool {
    matches!(character, '\u{9}' | '\u{a}' | '\u{d}')
        || ('\u{20}'..='\u{d7ff}').contains(&character)
        || ('\u{e000}'..='\u{fffd}').contains(&character)
        || ('\u{10000}'..='\u{10ffff}').contains(&character)
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        b'=' => Some(0),
        _ => None,
    }
}

fn parse_xml_integer(input: &[u8], profile: Profile) -> Option<Integer> {
    let mut rest = input;
    if profile.is_cf() {
        rest = trim_cf_integer_whitespace(rest)?;
    }
    if rest.is_empty() {
        return None;
    }

    let mut negative = false;
    if let Some(tail) = rest.strip_prefix(b"-") {
        negative = true;
        rest = tail;
        if profile.is_cf() {
            rest = trim_cf_integer_whitespace(rest)?;
        }
    } else if let Some(tail) = rest.strip_prefix(b"+") {
        rest = tail;
        if profile.is_cf() {
            rest = trim_cf_integer_whitespace(rest)?;
        }
    }
    if rest.is_empty() {
        return None;
    }

    let (radix, digits) = if profile.is_cf() {
        if let Some(hex) = rest
            .strip_prefix(b"0x")
            .or_else(|| rest.strip_prefix(b"0X"))
        {
            (16u64, hex)
        } else {
            (10u64, rest)
        }
    } else {
        (10u64, rest)
    };
    if digits.is_empty() {
        return None;
    }

    let mut magnitude = 0u64;
    for &byte in digits {
        let digit = match byte {
            b'0'..=b'9' => u64::from(byte - b'0'),
            b'a'..=b'f' if radix == 16 => u64::from(byte - b'a' + 10),
            b'A'..=b'F' if radix == 16 => u64::from(byte - b'A' + 10),
            _ => return None,
        };
        magnitude = magnitude.checked_mul(radix)?.checked_add(digit)?;
    }

    if negative {
        if magnitude > i64::MAX as u64 + 1 {
            return None;
        }
        let signed = if magnitude == i64::MAX as u64 + 1 {
            i128::from(i64::MIN)
        } else {
            -i128::from(magnitude)
        };
        Some(Integer::Signed(signed))
    } else if magnitude <= i64::MAX as u64 {
        Some(Integer::Signed(i128::from(magnitude)))
    } else {
        Some(Integer::Unsigned(u128::from(magnitude)))
    }
}

fn trim_cf_integer_whitespace(input: &[u8]) -> Option<&[u8]> {
    let mut cursor = 0;
    while cursor < input.len() && is_cf_whitespace_at(&input[cursor..]) {
        // The historical routine advances one byte even when its look-ahead
        // recognizes a multi-byte UTF-8 sequence. If that leaves the cursor in
        // the middle of a scalar, the subsequent numeric scan rejects it.
        cursor += 1;
    }
    input.get(cursor..)
}

fn is_cf_whitespace_at(input: &[u8]) -> bool {
    let Some(&first) = input.first() else {
        return false;
    };
    if first < 0x21 || (first > 0x7e && first < 0xa1) {
        return true;
    }
    if input.len() < 3 {
        return false;
    }
    match (first, input[1], input[2]) {
        (0xe2, 0x80, third) => (80..=0x8b).contains(&third) || third == 0xaf,
        (0xe2, 0x81, 0x9f) | (0xe3, 0x80, 0x80) => true,
        _ => false,
    }
}

fn parse_xml_real(input: &str, profile: Profile) -> Option<f64> {
    if input.is_empty() {
        return None;
    }

    if profile.is_cf() {
        if input.eq_ignore_ascii_case("nan") {
            return Some(f64::NAN);
        }
        if input.eq_ignore_ascii_case("+infinity")
            || input.eq_ignore_ascii_case("infinity")
            || input.eq_ignore_ascii_case("+inf")
            || input.eq_ignore_ascii_case("inf")
        {
            return Some(f64::INFINITY);
        }
        if input.eq_ignore_ascii_case("-infinity") || input.eq_ignore_ascii_case("-inf") {
            return Some(f64::NEG_INFINITY);
        }
        let numeric = trim_cf_real_whitespace(input);
        // The pinned scanner passes only this ASCII number set to strtod.
        // In particular, Rust's signed NaN spellings must not leak through.
        if numeric.is_empty()
            || !numeric
                .bytes()
                .all(|byte| matches!(byte, b'+' | b'-' | b'.' | b'0'..=b'9' | b'E' | b'e'))
        {
            return None;
        }
        return numeric.parse::<f64>().ok();
    } else {
        match input {
            "nan" => return Some(f64::NAN),
            "+infinity" | "infinity" => return Some(f64::INFINITY),
            "-infinity" => return Some(f64::NEG_INFINITY),
            _ => {}
        }
    }

    input.parse::<f64>().ok()
}

fn trim_cf_real_whitespace(input: &str) -> &str {
    let mut cursor = 0;
    for (offset, character) in input.char_indices() {
        if !is_cf_real_whitespace(character) {
            cursor = offset;
            break;
        }
        cursor = offset + character.len_utf8();
    }
    &input[cursor..]
}

fn is_cf_real_whitespace(character: char) -> bool {
    let scalar = u32::from(character);
    scalar < 0x21
        || (scalar > 0x7e && scalar < 0xa1)
        || (0x2000..=0x200b).contains(&scalar)
        || scalar == 0x3000
}

fn parse_xml_date(input: &str, profile: Profile) -> Option<f64> {
    let bytes = input.as_bytes();
    let mut cursor = 0usize;
    let negative_year = bytes.first() == Some(&b'-');
    if negative_year {
        cursor += 1;
    }
    let year_start = cursor;
    while bytes
        .get(cursor)
        .copied()
        .is_some_and(|byte| byte.is_ascii_digit())
    {
        cursor += 1;
    }
    let year_digits = cursor - year_start;
    if (year_digits == 0 && !(profile.is_cf() && negative_year))
        || (profile == Profile::Pure && (negative_year || year_digits != 4))
    {
        return None;
    }
    if bytes.get(cursor) != Some(&b'-') {
        return None;
    }
    let mut year = parse_decimal_i128(&bytes[year_start..cursor])?;
    if profile.is_cf() {
        // The parser accumulates into int32_t, and CFDate then subtracts 2001
        // in that same type before widening. Keep only the historical domain
        // in which both operations are defined.
        let maximum = if negative_year {
            i128::from(i32::MAX - 2000)
        } else {
            i128::from(i32::MAX)
        };
        if year > maximum {
            return None;
        }
    }
    if negative_year {
        year = -year;
    }
    cursor += 1;

    let month = read_two_digits(bytes, &mut cursor)?;
    expect_date_byte(bytes, &mut cursor, b'-')?;
    let day = read_two_digits(bytes, &mut cursor)?;
    expect_date_byte(bytes, &mut cursor, b'T')?;
    let hour = read_two_digits(bytes, &mut cursor)?;
    expect_date_byte(bytes, &mut cursor, b':')?;
    let minute = read_two_digits(bytes, &mut cursor)?;
    expect_date_byte(bytes, &mut cursor, b':')?;
    let second = read_two_digits(bytes, &mut cursor)?;
    expect_date_byte(bytes, &mut cursor, b'Z')?;
    if cursor != bytes.len() {
        return None;
    }

    if profile == Profile::Pure
        && (!(1..=12).contains(&month)
            || day == 0
            || day > days_in_month(year, month)
            || hour > 23
            || minute > 59
            || second > 59)
    {
        return None;
    }

    let days = if profile.is_cf() {
        // Historical CFDate.c indexes this 16-entry table directly. Months
        // 0-15 are defined (though noncanonical); larger values are C UB and
        // are rejected by this safe implementation.
        const DAYS_BEFORE_MONTH: [u16; 16] = [
            0, 0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334, 365, 0, 0,
        ];
        let month_index = usize::try_from(month).ok()?;
        let mut days_before = i128::from(*DAYS_BEFORE_MONTH.get(month_index)?);
        if month > 2 && is_leap_year(year) {
            days_before += 1;
        }
        days_from_civil(year, 1, 1)
            .checked_add(days_before)?
            .checked_add(i128::from(day) - 1)?
    } else {
        days_from_civil(year, i128::from(month), 1).checked_add(i128::from(day) - 1)?
    };
    let unix_seconds = days
        .checked_mul(86_400)?
        .checked_add(i128::from(hour) * 3_600)?
        .checked_add(i128::from(minute) * 60)?
        .checked_add(i128::from(second))?;
    let absolute_time = unix_seconds as f64 - Date::UNIX_EPOCH_OFFSET;
    absolute_time.is_finite().then_some(absolute_time)
}

fn read_two_digits(bytes: &[u8], cursor: &mut usize) -> Option<u32> {
    let first = *bytes.get(*cursor)?;
    let second = *bytes.get(cursor.checked_add(1)?)?;
    if !first.is_ascii_digit() || !second.is_ascii_digit() {
        return None;
    }
    *cursor += 2;
    Some(u32::from(first - b'0') * 10 + u32::from(second - b'0'))
}

fn expect_date_byte(bytes: &[u8], cursor: &mut usize, expected: u8) -> Option<()> {
    if bytes.get(*cursor) != Some(&expected) {
        return None;
    }
    *cursor += 1;
    Some(())
}

fn parse_decimal_i128(bytes: &[u8]) -> Option<i128> {
    bytes.iter().try_fold(0i128, |value, byte| {
        value
            .checked_mul(10)?
            .checked_add(i128::from(byte.checked_sub(b'0')?))
    })
}

fn days_in_month(year: i128, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: i128) -> bool {
    year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
}

// Howard Hinnant's civil-date transform, expressed with Euclidean arithmetic
// so astronomical year zero and negative years behave deterministically.
fn days_from_civil(year: i128, month: i128, day: i128) -> i128 {
    let adjusted_year = year - i128::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::Limits;

    fn cf_document(source: &[u8]) -> Document<'_> {
        let options = ParseOptions::new()
            .with_format(Format::Xml)
            .with_backend(BackendKind::CoreFoundation);
        let parsed = parse_xml_cf(source, &options).expect("CF-compatible XML should parse");
        Document::from_parts(source, parsed)
    }

    fn pure_document(source: &[u8]) -> Document<'_> {
        let options = ParseOptions::new()
            .with_format(Format::Xml)
            .with_backend(BackendKind::Pure);
        let parsed = parse_xml_pure(source, &options).expect("public plist XML should parse");
        Document::from_parts(source, parsed)
    }

    #[test]
    fn cf_accepts_bare_root_normalizes_text_and_ignores_trailing_content() {
        let source = b"<string>A&amp;<![CDATA[B]]></string>not XML";
        let document = cf_document(source);
        assert_eq!(
            document.root().string().unwrap().to_string().unwrap(),
            "A&B"
        );
    }

    #[test]
    fn pure_profile_requires_one_wrapped_root() {
        let options = ParseOptions::new().with_format(Format::Xml);
        assert!(parse_xml_pure(b"<string>x</string>", &options).is_err());
        assert!(parse_xml_pure(
            b"<plist version=\"1.0\"><string>x</string></plist>junk",
            &options
        )
        .is_err());

        let document = pure_document(b"<plist version=\"1.0\"><string>x</string></plist>");
        assert_eq!(document.root().as_str(), Some("x"));
    }

    #[test]
    fn duplicate_dictionary_keys_are_last_wins() {
        let source = b"<plist version=\"1.0\"><dict>\
            <key>answer</key><integer>1</integer>\
            <key>answer</key><integer>2</integer>\
            </dict></plist>";
        let document = pure_document(source);
        let dictionary = document.root().as_dictionary().unwrap();
        assert_eq!(dictionary.len(), 1);
        assert_eq!(
            dictionary.get("answer").unwrap().as_integer(),
            Some(Integer::Signed(2))
        );
    }

    #[test]
    fn cf_converts_single_cf_uid_number_dictionary_after_deduplication() {
        let integer = cf_document(
            b"<dict><key>CF$UID</key><integer>1</integer>\
              <key>CF$UID</key><integer>-2</integer></dict>",
        );
        assert_eq!(integer.root().as_uid(), Some((-2_i32) as u32));

        let real = cf_document(b"<dict><key>CF$UID</key><real>7.9</real></dict>");
        assert_eq!(real.root().as_uid(), Some(7));

        let positive_wrap =
            cf_document(b"<dict><key>CF$UID</key><integer>2147483648</integer></dict>");
        assert_eq!(positive_wrap.root().as_uid(), Some(0x8000_0000));

        let wide_wrap =
            cf_document(b"<dict><key>CF&#36;UID</key><integer>4294967297</integer></dict>");
        assert_eq!(wide_wrap.root().as_uid(), Some(1));

        let non_number = cf_document(b"<dict><key>CF$UID</key><string>7</string></dict>");
        assert_eq!(non_number.root().kind(), crate::ValueKind::Dictionary);

        let pure = pure_document(
            b"<plist version=\"1.0\"><dict><key>CF$UID</key><integer>7</integer></dict></plist>",
        );
        assert_eq!(pure.root().kind(), crate::ValueKind::Dictionary);
    }

    #[test]
    fn cf_base64_ignores_ascii_garbage_but_pure_rejects_it() {
        let source = b"<data>Q!Q==</data>";
        assert_eq!(cf_document(source).root().as_data(), Some(&b"A"[..]));

        let options = ParseOptions::new().with_format(Format::Xml);
        let wrapped = b"<plist version=\"1.0\"><data>Q!Q==</data></plist>";
        let error = parse_xml_pure(wrapped, &options).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidData);
    }

    #[test]
    fn cf_numeric_and_calendar_compatibility() {
        let integer = cf_document(b"<integer> - \t 0xFACE</integer>");
        assert_eq!(integer.root().as_integer(), Some(Integer::Signed(-0xface)));

        let invalid_utf8_whitespace = cf_document(b"<integer>\x801</integer>");
        assert_eq!(
            invalid_utf8_whitespace.root().as_integer(),
            Some(Integer::Signed(1))
        );

        let bytewise_whitespace = cf_document("<integer>\u{205f}1</integer>".as_bytes());
        assert_eq!(
            bytewise_whitespace.root().as_integer(),
            Some(Integer::Signed(1))
        );

        let real = cf_document("<real>\u{85}\u{a0}1.5</real>".as_bytes());
        assert_eq!(real.root().as_real().unwrap().as_f64(), 1.5);

        let options = ParseOptions::new()
            .with_format(Format::Xml)
            .with_backend(BackendKind::CoreFoundation);
        assert!(parse_xml_cf("<integer>\u{202f}1</integer>".as_bytes(), &options).is_err());
        assert!(parse_xml_cf("<integer>\u{1680}1</integer>".as_bytes(), &options).is_err());
        assert!(parse_xml_cf(b"<real>  nAn</real>", &options).is_err());
        assert!(parse_xml_cf("<real>\u{205f}1</real>".as_bytes(), &options).is_err());
        assert!(parse_xml_cf(b"<real>+nan</real>", &options).is_err());
        assert!(cf_document(b"<real>NaN</real>")
            .root()
            .as_real()
            .unwrap()
            .as_f64()
            .is_nan());

        let normalized = cf_document(b"<date>2024-01-32T20:43:14Z</date>");
        let canonical = cf_document(b"<date>2024-02-01T20:43:14Z</date>");
        assert_eq!(
            normalized.root().as_date().unwrap().cf_absolute_time(),
            canonical.root().as_date().unwrap().cf_absolute_time()
        );

        let month_zero = cf_document(b"<date>2024-00-01T00:00:00Z</date>");
        let january = cf_document(b"<date>2024-01-01T00:00:00Z</date>");
        assert_eq!(
            month_zero.root().as_date().unwrap().cf_absolute_time(),
            january.root().as_date().unwrap().cf_absolute_time()
        );

        let month_thirteen = cf_document(b"<date>2023-13-01T00:00:00Z</date>");
        assert_eq!(
            month_thirteen.root().as_date().unwrap().cf_absolute_time(),
            january.root().as_date().unwrap().cf_absolute_time()
        );

        let empty_negative_year = cf_document(b"<date>--01-01T00:00:00Z</date>");
        let year_zero = cf_document(b"<date>0-01-01T00:00:00Z</date>");
        assert_eq!(
            empty_negative_year
                .root()
                .as_date()
                .unwrap()
                .cf_absolute_time(),
            year_zero.root().as_date().unwrap().cf_absolute_time()
        );
        assert!(parse_xml_cf(b"<date>2024-16-01T00:00:00Z</date>", &options).is_err());
        assert!(parse_xml_cf(b"<date>-2147481648-01-01T00:00:00Z</date>", &options,).is_err());
        assert!(parse_xml_cf(b"<date>-2147481647-01-01T00:00:00Z</date>", &options,).is_ok());
    }

    #[test]
    fn skips_processing_instructions_comments_and_external_doctype() {
        let source = br#"<?xml version="1.0" encoding="UTF-8"?>
            <!-- prolog -->
            <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
                "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
            <plist ignored="yes"><string>ok</string></plist>"#;
        assert_eq!(cf_document(source).root().as_str(), Some("ok"));
    }

    #[test]
    fn both_profiles_reject_inline_dtd_subsets() {
        let source = br#"<!DOCTYPE plist [
            <!ELEMENT plist ANY>
        ]>
        <plist version="1.0"><string>ok</string></plist>"#;
        let cf_options = ParseOptions::new()
            .with_format(Format::Xml)
            .with_backend(BackendKind::CoreFoundation);
        assert_eq!(
            parse_xml_cf(source, &cf_options).unwrap_err().kind(),
            ErrorKind::Malformed
        );

        let pure_options = ParseOptions::new()
            .with_format(Format::Xml)
            .with_backend(BackendKind::Pure);
        assert_eq!(
            parse_xml_pure(source, &pure_options).unwrap_err().kind(),
            ErrorKind::Malformed
        );
    }

    #[test]
    fn cf_doctype_scan_is_quote_blind() {
        let options = ParseOptions::new()
            .with_format(Format::Xml)
            .with_backend(BackendKind::CoreFoundation);
        assert!(parse_xml_cf(
            br#"<!DOCTYPE plist SYSTEM "foo>bar"><string>ok</string>"#,
            &options,
        )
        .is_err());
        assert!(parse_xml_cf(
            br#"<!DOCTYPE plist SYSTEM "foo[bar"><string>ok</string>"#,
            &options,
        )
        .is_err());
    }

    #[test]
    fn cf_ignores_invalid_utf8_after_the_completed_root() {
        let source = b"<string>ok</string>\xff";
        assert_eq!(cf_document(source).root().as_str(), Some("ok"));

        let options = ParseOptions::new()
            .with_format(Format::Xml)
            .with_backend(BackendKind::CoreFoundation);
        assert_eq!(
            parse_xml_cf(b"<string>bad\xff</string>", &options)
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidString
        );
    }

    #[test]
    fn cf_numeric_entities_wrap_to_one_utf16_code_unit() {
        let wrapped = cf_document(b"<string>&#128512;</string>");
        assert_eq!(
            wrapped.root().string().unwrap().to_string().unwrap(),
            "\u{f600}"
        );

        let surrogate = cf_document(b"<string>&#55296;</string>");
        assert_eq!(surrogate.root().as_str(), Some(""));

        assert_eq!(
            cf_document(b"<string>&#;</string>").root().as_str(),
            Some("\0")
        );
        assert_eq!(
            pure_document(b"<plist version=\"1.0\"><string>&#128512;</string></plist>")
                .root()
                .as_str(),
            Some("😀")
        );
    }

    #[test]
    fn cf_keeps_slashes_inside_malformed_element_names() {
        let options = ParseOptions::new()
            .with_format(Format::Xml)
            .with_backend(BackendKind::CoreFoundation);
        assert!(parse_xml_cf(b"<array/foo></array>", &options).is_err());
        assert!(parse_xml_cf(b"<array/foo/>", &options).is_err());
        assert!(parse_xml_cf(b"<array/>", &options).is_ok());

        let paired = cf_document(b"<array><true / ></true><false/></array>");
        let mut values = paired.root().as_array().unwrap().iter();
        assert_eq!(values.next().unwrap().as_bool(), Some(true));
        assert_eq!(values.next().unwrap().as_bool(), Some(false));
        assert!(values.next().is_none());
    }

    #[test]
    fn transcodes_utf16_and_utf32_inputs() {
        let xml = "<string>snowman ☃</string>";

        let mut utf16le = vec![0xff, 0xfe];
        for unit in xml.encode_utf16() {
            utf16le.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(
            cf_document(&utf16le)
                .root()
                .string()
                .unwrap()
                .to_string()
                .unwrap(),
            "snowman ☃"
        );

        let mut utf32be = vec![0x00, 0x00, 0xfe, 0xff];
        for character in xml.chars() {
            utf32be.extend_from_slice(&u32::from(character).to_be_bytes());
        }
        assert_eq!(cf_document(&utf32be).root().as_str(), Some("snowman ☃"));
    }

    #[test]
    fn configured_limits_fail_before_unbounded_growth() {
        let options = ParseOptions::new()
            .with_format(Format::Xml)
            .with_backend(BackendKind::CoreFoundation)
            .with_limits(Limits::new().with_max_depth(1));
        let error = parse_xml_cf(
            b"<array><array><string>x</string></array></array>",
            &options,
        )
        .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::LimitExceeded);
    }
}
