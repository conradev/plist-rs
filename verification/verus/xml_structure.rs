/*
 * Copyright (c) 2015 Apple Inc. All rights reserved.
 *
 * @APPLE_LICENSE_HEADER_START@
 *
 * This file contains Original Code and/or Modifications of Original Code
 * as defined in and that are subject to the Apple Public Source License
 * Version 2.0 (the 'License'). You may not use this file except in
 * compliance with the License. Please obtain a copy of the License at
 * http://www.opensource.apple.com/apsl/ and read it before using this
 * file.
 *
 * The Original Code and all software distributed under the License are
 * distributed on an 'AS IS' basis, WITHOUT WARRANTY OF ANY KIND, EITHER
 * EXPRESS OR IMPLIED, AND APPLE HEREBY DISCLAIMS ALL SUCH WARRANTIES,
 * INCLUDING WITHOUT LIMITATION, ANY WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE, QUIET ENJOYMENT OR NON-INFRINGEMENT.
 * Please see the License for the specific language governing rights and
 * limitations under the License.
 *
 * @APPLE_LICENSE_HEADER_END@
 *
 * Modified 2026-08-22 by plist-rs contributors: translated the
 * allocation-independent XML structure state machine to executable
 * Rust/Verus and added machine-checked refinement proofs.
 */

/*	CFPropertyList.c
	Copyright (c) 1999-2014, Apple Inc. All rights reserved.
	Responsibility: Tony Parker
*/

//! Executable structural port of the XML-plist reader in
//! `opensource-apple/CF@3cc41a76.../CFPropertyList.c`.
//!
//! Bridge boundary: raw start/close tags are consumed here. Container proofs
//! consume a finite abstract token stream supplied by the lexical layer proven
//! in `xml_plist.rs`. A `Child` token represents one already-validated complete
//! child element and its abstract node id; `Misc` represents only whitespace,
//! comments, or processing instructions successfully skipped by that layer.
//! No claim is made here about byte decoding, scalar construction, recursive
//! dispatch, CF allocation, retain counts, or error-object text.
//!
//! `APPLE:` comments map executable transitions to the pinned C source. Specs
//! are independent mathematical recurrences. Every loop and recurrence is
//! total over finite inputs and has a decreasing suffix length.

use vstd::prelude::*;

verus! {

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tag {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagScanStatus {
    Parsed,
    Eof,
    Malformed,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartTagScan {
    pub status: TagScanStatus,
    pub tag: Tag,
    pub cursor: usize,
    pub name_start: usize,
    pub name_len: usize,
    pub attributes_start: usize,
    pub attributes_end: usize,
    pub self_closing: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloseTagScan {
    pub ok: bool,
    pub cursor: usize,
}

pub open spec fn structure_whitespace_spec(byte: u8) -> bool {
    byte == b' ' || byte == b'\t' || byte == b'\n' || byte == b'\r'
}

pub fn structure_whitespace(byte: u8) -> (yes: bool)
    ensures yes == structure_whitespace_spec(byte),
{
    byte == b' ' || byte == b'\t' || byte == b'\n' || byte == b'\r'
}

pub open spec fn tag_len_spec(tag: Tag) -> nat {
    match tag {
        Tag::Plist => 5,
        Tag::Array => 5,
        Tag::Dict => 4,
        Tag::Key => 3,
        Tag::String => 6,
        Tag::Data => 4,
        Tag::Date => 4,
        Tag::Real => 4,
        Tag::Integer => 7,
        Tag::True => 4,
        Tag::False => 5,
        Tag::Unknown => 0,
    }
}

pub fn tag_len(tag: Tag) -> (len: usize)
    ensures len as nat == tag_len_spec(tag),
{
    match tag {
        Tag::Plist => 5,
        Tag::Array => 5,
        Tag::Dict => 4,
        Tag::Key => 3,
        Tag::String => 6,
        Tag::Data => 4,
        Tag::Date => 4,
        Tag::Real => 4,
        Tag::Integer => 7,
        Tag::True => 4,
        Tag::False => 5,
        Tag::Unknown => 0,
    }
}

pub open spec fn matches_tag_name_spec(
    bytes: Seq<u8>,
    start: nat,
    len: nat,
    tag: Tag,
) -> bool {
    len == tag_len_spec(tag)
        && start + len <= bytes.len()
        && match tag {
            Tag::Plist => bytes[start as int] == b'p'
                && bytes[(start + 1) as int] == b'l'
                && bytes[(start + 2) as int] == b'i'
                && bytes[(start + 3) as int] == b's'
                && bytes[(start + 4) as int] == b't',
            Tag::Array => bytes[start as int] == b'a'
                && bytes[(start + 1) as int] == b'r'
                && bytes[(start + 2) as int] == b'r'
                && bytes[(start + 3) as int] == b'a'
                && bytes[(start + 4) as int] == b'y',
            Tag::Dict => bytes[start as int] == b'd'
                && bytes[(start + 1) as int] == b'i'
                && bytes[(start + 2) as int] == b'c'
                && bytes[(start + 3) as int] == b't',
            Tag::Key => bytes[start as int] == b'k'
                && bytes[(start + 1) as int] == b'e'
                && bytes[(start + 2) as int] == b'y',
            Tag::String => bytes[start as int] == b's'
                && bytes[(start + 1) as int] == b't'
                && bytes[(start + 2) as int] == b'r'
                && bytes[(start + 3) as int] == b'i'
                && bytes[(start + 4) as int] == b'n'
                && bytes[(start + 5) as int] == b'g',
            Tag::Data => bytes[start as int] == b'd'
                && bytes[(start + 1) as int] == b'a'
                && bytes[(start + 2) as int] == b't'
                && bytes[(start + 3) as int] == b'a',
            Tag::Date => bytes[start as int] == b'd'
                && bytes[(start + 1) as int] == b'a'
                && bytes[(start + 2) as int] == b't'
                && bytes[(start + 3) as int] == b'e',
            Tag::Real => bytes[start as int] == b'r'
                && bytes[(start + 1) as int] == b'e'
                && bytes[(start + 2) as int] == b'a'
                && bytes[(start + 3) as int] == b'l',
            Tag::Integer => bytes[start as int] == b'i'
                && bytes[(start + 1) as int] == b'n'
                && bytes[(start + 2) as int] == b't'
                && bytes[(start + 3) as int] == b'e'
                && bytes[(start + 4) as int] == b'g'
                && bytes[(start + 5) as int] == b'e'
                && bytes[(start + 6) as int] == b'r',
            Tag::True => bytes[start as int] == b't'
                && bytes[(start + 1) as int] == b'r'
                && bytes[(start + 2) as int] == b'u'
                && bytes[(start + 3) as int] == b'e',
            Tag::False => bytes[start as int] == b'f'
                && bytes[(start + 1) as int] == b'a'
                && bytes[(start + 2) as int] == b'l'
                && bytes[(start + 3) as int] == b's'
                && bytes[(start + 4) as int] == b'e',
            Tag::Unknown => false,
        }
}

pub fn matches_tag_name(
    bytes: &Vec<u8>,
    start: usize,
    len: usize,
    tag: Tag,
) -> (yes: bool)
    requires start <= bytes.len(), len <= bytes.len() - start,
    ensures yes == matches_tag_name_spec(bytes@, start as nat, len as nat, tag),
{
    if len != tag_len(tag) {
        return false;
    }
    match tag {
        Tag::Plist => bytes[start] == b'p' && bytes[start + 1] == b'l'
            && bytes[start + 2] == b'i' && bytes[start + 3] == b's'
            && bytes[start + 4] == b't',
        Tag::Array => bytes[start] == b'a' && bytes[start + 1] == b'r'
            && bytes[start + 2] == b'r' && bytes[start + 3] == b'a'
            && bytes[start + 4] == b'y',
        Tag::Dict => bytes[start] == b'd' && bytes[start + 1] == b'i'
            && bytes[start + 2] == b'c' && bytes[start + 3] == b't',
        Tag::Key => bytes[start] == b'k' && bytes[start + 1] == b'e'
            && bytes[start + 2] == b'y',
        Tag::String => bytes[start] == b's' && bytes[start + 1] == b't'
            && bytes[start + 2] == b'r' && bytes[start + 3] == b'i'
            && bytes[start + 4] == b'n' && bytes[start + 5] == b'g',
        Tag::Data => bytes[start] == b'd' && bytes[start + 1] == b'a'
            && bytes[start + 2] == b't' && bytes[start + 3] == b'a',
        Tag::Date => bytes[start] == b'd' && bytes[start + 1] == b'a'
            && bytes[start + 2] == b't' && bytes[start + 3] == b'e',
        Tag::Real => bytes[start] == b'r' && bytes[start + 1] == b'e'
            && bytes[start + 2] == b'a' && bytes[start + 3] == b'l',
        Tag::Integer => bytes[start] == b'i' && bytes[start + 1] == b'n'
            && bytes[start + 2] == b't' && bytes[start + 3] == b'e'
            && bytes[start + 4] == b'g' && bytes[start + 5] == b'e'
            && bytes[start + 6] == b'r',
        Tag::True => bytes[start] == b't' && bytes[start + 1] == b'r'
            && bytes[start + 2] == b'u' && bytes[start + 3] == b'e',
        Tag::False => bytes[start] == b'f' && bytes[start + 1] == b'a'
            && bytes[start + 2] == b'l' && bytes[start + 3] == b's'
            && bytes[start + 4] == b'e',
        Tag::Unknown => false,
    }
}

pub open spec fn classify_tag_spec(bytes: Seq<u8>, start: nat, len: nat) -> Tag {
    if matches_tag_name_spec(bytes, start, len, Tag::Array) { Tag::Array }
    else if matches_tag_name_spec(bytes, start, len, Tag::Dict) { Tag::Dict }
    else if matches_tag_name_spec(bytes, start, len, Tag::Data) { Tag::Data }
    else if matches_tag_name_spec(bytes, start, len, Tag::Date) { Tag::Date }
    else if matches_tag_name_spec(bytes, start, len, Tag::False) { Tag::False }
    else if matches_tag_name_spec(bytes, start, len, Tag::Integer) { Tag::Integer }
    else if matches_tag_name_spec(bytes, start, len, Tag::Key) { Tag::Key }
    else if matches_tag_name_spec(bytes, start, len, Tag::Plist) { Tag::Plist }
    else if matches_tag_name_spec(bytes, start, len, Tag::Real) { Tag::Real }
    else if matches_tag_name_spec(bytes, start, len, Tag::String) { Tag::String }
    else if matches_tag_name_spec(bytes, start, len, Tag::True) { Tag::True }
    else { Tag::Unknown }
}

pub fn classify_tag(bytes: &Vec<u8>, start: usize, len: usize) -> (tag: Tag)
    requires start <= bytes.len(), len <= bytes.len() - start,
    ensures tag == classify_tag_spec(bytes@, start as nat, len as nat),
{
    if matches_tag_name(bytes, start, len, Tag::Array) { Tag::Array }
    else if matches_tag_name(bytes, start, len, Tag::Dict) { Tag::Dict }
    else if matches_tag_name(bytes, start, len, Tag::Data) { Tag::Data }
    else if matches_tag_name(bytes, start, len, Tag::Date) { Tag::Date }
    else if matches_tag_name(bytes, start, len, Tag::False) { Tag::False }
    else if matches_tag_name(bytes, start, len, Tag::Integer) { Tag::Integer }
    else if matches_tag_name(bytes, start, len, Tag::Key) { Tag::Key }
    else if matches_tag_name(bytes, start, len, Tag::Plist) { Tag::Plist }
    else if matches_tag_name(bytes, start, len, Tag::Real) { Tag::Real }
    else if matches_tag_name(bytes, start, len, Tag::String) { Tag::String }
    else if matches_tag_name(bytes, start, len, Tag::True) { Tag::True }
    else { Tag::Unknown }
}

pub open spec fn classified_status_spec(tag: Tag) -> TagScanStatus {
    match tag { Tag::Unknown => TagScanStatus::Unknown, _ => TagScanStatus::Parsed }
}

pub fn classified_status(tag: Tag) -> (status: TagScanStatus)
    ensures status == classified_status_spec(tag),
{
    reveal(classified_status_spec);
    match tag { Tag::Unknown => TagScanStatus::Unknown, _ => TagScanStatus::Parsed }
}

pub open spec fn classified_cursor_spec(tag: Tag, marker: nat, after_close: nat) -> nat {
    match tag { Tag::Unknown => marker, _ => after_close }
}

pub fn classified_cursor(tag: Tag, marker: usize, after_close: usize) -> (cursor: usize)
    ensures cursor as nat == classified_cursor_spec(tag, marker as nat, after_close as nat),
{
    reveal(classified_cursor_spec);
    match tag { Tag::Unknown => marker, _ => after_close }
}

pub open spec fn finish_start_tag_spec(
    bytes: Seq<u8>,
    marker: nat,
    close: nat,
    first_whitespace: Option<nat>,
) -> StartTagScan
    recommends marker <= close < bytes.len(), bytes.len() <= usize::MAX,
{
    // `close == marker` makes the source's `curr - 1` dereference undefined;
    // this safe totalization reports the same malformed empty marker intended
    // by the following source check.
    if close == marker {
        StartTagScan {
            status: TagScanStatus::Malformed,
            tag: Tag::Unknown,
            cursor: marker as usize,
            name_start: marker as usize,
            name_len: 0,
            attributes_start: marker as usize,
            attributes_end: marker as usize,
            self_closing: false,
        }
    } else {
        let self_closing = bytes[(close - 1) as int] == b'/';
        let name_end = match first_whitespace {
            Some(at) => at,
            None => if self_closing { (close - 1) as nat } else { close },
        };
        let name_len: nat = if name_end >= marker { (name_end - marker) as nat } else { 0 };
        let attributes_end = if self_closing { (close - 1) as nat } else { close };
        if name_len == 0 {
            StartTagScan {
                status: TagScanStatus::Malformed,
                tag: Tag::Unknown,
                cursor: marker as usize,
                name_start: marker as usize,
                name_len: 0,
                attributes_start: marker as usize,
                attributes_end: marker as usize,
                self_closing,
            }
        } else {
            let tag = classify_tag_spec(bytes, marker, name_len);
            StartTagScan {
                status: classified_status_spec(tag),
                tag,
                cursor: classified_cursor_spec(tag, marker, close + 1) as usize,
                name_start: marker as usize,
                name_len: name_len as usize,
                attributes_start: name_end as usize,
                attributes_end: attributes_end as usize,
                self_closing,
            }
        }
    }
}

pub fn finish_start_tag(
    bytes: &Vec<u8>,
    marker: usize,
    close: usize,
    first_whitespace: Option<usize>,
) -> (result: StartTagScan)
    requires
        marker <= close < bytes.len(),
        match first_whitespace { Some(at) => marker <= at <= close, None => true },
    ensures result == finish_start_tag_spec(
        bytes@,
        marker as nat,
        close as nat,
        match first_whitespace { Some(at) => Some(at as nat), None => None },
    ),
{
    proof {
        assert(bytes@.len() <= usize::MAX);
    }
    if close == marker {
        return StartTagScan {
            status: TagScanStatus::Malformed,
            tag: Tag::Unknown,
            cursor: marker,
            name_start: marker,
            name_len: 0,
            attributes_start: marker,
            attributes_end: marker,
            self_closing: false,
        };
    }
    let self_closing = bytes[close - 1] == b'/';
    let name_end = match first_whitespace {
        Some(at) => at,
        None => if self_closing { close - 1 } else { close },
    };
    let name_len = name_end - marker;
    let attributes_end = if self_closing { close - 1 } else { close };
    if name_len == 0 {
        return StartTagScan {
            status: TagScanStatus::Malformed,
            tag: Tag::Unknown,
            cursor: marker,
            name_start: marker,
            name_len: 0,
            attributes_start: marker,
            attributes_end: marker,
            self_closing,
        };
    }
    let tag = classify_tag(bytes, marker, name_len);
    let status = classified_status(tag);
    let cursor = classified_cursor(tag, marker, close + 1);
    StartTagScan {
        status,
        tag,
        cursor,
        name_start: marker,
        name_len,
        attributes_start: name_end,
        attributes_end,
        self_closing,
    }
}

pub open spec fn scan_start_tag_state_spec(
    bytes: Seq<u8>,
    marker: nat,
    at: nat,
    first_whitespace: Option<nat>,
) -> StartTagScan
    recommends marker <= at, bytes.len() <= usize::MAX,
    decreases bytes.len() - at
{
    if at >= bytes.len() {
        StartTagScan {
            status: TagScanStatus::Eof,
            tag: Tag::Unknown,
            cursor: bytes.len() as usize,
            name_start: marker as usize,
            name_len: 0,
            attributes_start: marker as usize,
            attributes_end: marker as usize,
            self_closing: false,
        }
    } else if bytes[at as int] == b'>' {
        finish_start_tag_spec(bytes, marker, at, first_whitespace)
    } else {
        let next_whitespace = match first_whitespace {
            Some(saved) => Some(saved),
            None => if structure_whitespace_spec(bytes[at as int]) { Some(at) } else { None },
        };
        scan_start_tag_state_spec(bytes, marker, at + 1, next_whitespace)
    }
}

/// APPLE: `parseXMLElement` marker scan and dispatch,
/// CFPropertyList.c:1984-2060 and 2184-2189. Attribute bytes are deliberately
/// opaque. Only a slash immediately before the first `>` marks an empty tag.
pub fn scan_start_tag(bytes: &Vec<u8>, marker: usize) -> (result: StartTagScan)
    requires marker <= bytes.len(),
    ensures result == scan_start_tag_state_spec(bytes@, marker as nat, marker as nat, None),
{
    let mut cursor = marker;
    let mut first_whitespace: Option<usize> = None;
    while cursor < bytes.len()
        invariant
            marker <= cursor <= bytes.len(),
            match first_whitespace { Some(at) => marker <= at < cursor, None => true },
            scan_start_tag_state_spec(bytes@, marker as nat, marker as nat, None)
                == scan_start_tag_state_spec(
                    bytes@,
                    marker as nat,
                    cursor as nat,
                    match first_whitespace { Some(at) => Some(at as nat), None => None },
                ),
        decreases bytes.len() - cursor
    {
        let byte = bytes[cursor];
        if byte == b'>' {
            return finish_start_tag(bytes, marker, cursor, first_whitespace);
        }
        if first_whitespace.is_none() && structure_whitespace(byte) {
            first_whitespace = Some(cursor);
        }
        cursor += 1;
    }
    StartTagScan {
        status: TagScanStatus::Eof,
        tag: Tag::Unknown,
        cursor: bytes.len(),
        name_start: marker,
        name_len: 0,
        attributes_start: marker,
        attributes_end: marker,
        self_closing: false,
    }
}

pub open spec fn skip_structure_whitespace_spec(bytes: Seq<u8>, at: nat) -> nat
    decreases bytes.len() - at
{
    if at >= bytes.len() || !structure_whitespace_spec(bytes[at as int]) {
        at
    } else {
        skip_structure_whitespace_spec(bytes, at + 1)
    }
}

pub fn skip_structure_whitespace(bytes: &Vec<u8>, start: usize) -> (cursor: usize)
    requires start <= bytes.len(),
    ensures
        cursor as nat == skip_structure_whitespace_spec(bytes@, start as nat),
        start <= cursor <= bytes.len(),
{
    let mut cursor = start;
    while cursor < bytes.len() && structure_whitespace(bytes[cursor])
        invariant
            start <= cursor <= bytes.len(),
            skip_structure_whitespace_spec(bytes@, start as nat)
                == skip_structure_whitespace_spec(bytes@, cursor as nat),
        decreases bytes.len() - cursor
    {
        cursor += 1;
    }
    cursor
}

pub open spec fn close_tag_spec(bytes: Seq<u8>, start: nat, expected: Tag) -> CloseTagScan
    recommends start <= bytes.len(), bytes.len() <= usize::MAX, expected != Tag::Unknown,
{
    let name_len = tag_len_spec(expected);
    if bytes.len() - start < name_len + 3 {
        CloseTagScan { ok: false, cursor: start as usize }
    } else if bytes[start as int] != b'<' {
        CloseTagScan { ok: false, cursor: start as usize }
    } else if bytes[(start + 1) as int] != b'/' {
        CloseTagScan { ok: false, cursor: (start + 1) as usize }
    } else if !matches_tag_name_spec(bytes, start + 2, name_len, expected) {
        CloseTagScan { ok: false, cursor: (start + 2) as usize }
    } else {
        let after_name = start + 2 + name_len;
        let after_space = skip_structure_whitespace_spec(bytes, after_name);
        if after_space >= bytes.len() {
            CloseTagScan { ok: false, cursor: bytes.len() as usize }
        } else if bytes[after_space as int] != b'>' {
            CloseTagScan { ok: false, cursor: after_space as usize }
        } else {
            CloseTagScan { ok: true, cursor: (after_space + 1) as usize }
        }
    }
}

/// APPLE: `checkForCloseTag`, CFPropertyList.c:1213-1251. The failure cursor
/// preserves C short-circuit behavior: a non-`<` leaves it unchanged, whereas
/// `<` followed by a non-slash leaves it advanced by one byte.
pub fn check_close_tag(
    bytes: &Vec<u8>,
    start: usize,
    expected: Tag,
) -> (result: CloseTagScan)
    requires start <= bytes.len(), expected != Tag::Unknown,
    ensures result == close_tag_spec(bytes@, start as nat, expected),
{
    let name_len = tag_len(expected);
    if bytes.len() - start < name_len + 3 {
        return CloseTagScan { ok: false, cursor: start };
    }
    if bytes[start] != b'<' {
        return CloseTagScan { ok: false, cursor: start };
    }
    if bytes[start + 1] != b'/' {
        return CloseTagScan { ok: false, cursor: start + 1 };
    }
    let name_start = start + 2;
    if !matches_tag_name(bytes, name_start, name_len, expected) {
        return CloseTagScan { ok: false, cursor: name_start };
    }
    let after_name = name_start + name_len;
    let cursor = skip_structure_whitespace(bytes, after_name);
    if cursor >= bytes.len() {
        CloseTagScan { ok: false, cursor }
    } else if bytes[cursor] != b'>' {
        CloseTagScan { ok: false, cursor }
    } else {
        CloseTagScan { ok: true, cursor: cursor + 1 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScalarKey {
    /// The lexical bridge assigns equal families only to scalar values whose
    /// equality is meaningful across the same scalar representation.
    pub family: u8,
    pub symbol: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    /// Whitespace, a comment, or a processing instruction already proven to
    /// terminate successfully by `xml_plist.rs`.
    Misc,
    /// One complete non-key child element.
    Value { node: usize },
    /// One complete `<key>` child and its normalized abstract scalar.
    Key { node: usize, scalar: ScalarKey },
    /// A close tag; like `getContentObject`, selection does not consume it.
    Close { tag: Tag },
    /// A lexical or recursive child error.
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionKind {
    Child,
    End,
    Eof,
    Malformed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentSelection {
    pub kind: SelectionKind,
    pub cursor: usize,
    pub node: usize,
    pub is_key: bool,
    pub scalar: ScalarKey,
}

pub const EMPTY_SCALAR: ScalarKey = ScalarKey { family: 0, symbol: 0 };

pub open spec fn select_misc_content_spec(tokens: Seq<Token>, at: nat) -> ContentSelection
    recommends tokens.len() <= usize::MAX,
    decreases tokens.len() - at
{
    if at >= tokens.len() {
        ContentSelection {
            kind: SelectionKind::Eof,
            cursor: tokens.len() as usize,
            node: 0,
            is_key: false,
            scalar: EMPTY_SCALAR,
        }
    } else {
        match tokens[at as int] {
            Token::Misc => select_misc_content_spec(tokens, at + 1),
            Token::Value { node } => ContentSelection {
                kind: SelectionKind::Child,
                cursor: (at + 1) as usize,
                node,
                is_key: false,
                scalar: EMPTY_SCALAR,
            },
            Token::Key { node, scalar } => ContentSelection {
                kind: SelectionKind::Child,
                cursor: (at + 1) as usize,
                node,
                is_key: true,
                scalar,
            },
            Token::Close { .. } => ContentSelection {
                kind: SelectionKind::End,
                cursor: at as usize,
                node: 0,
                is_key: false,
                scalar: EMPTY_SCALAR,
            },
            Token::Error => ContentSelection {
                kind: SelectionKind::Malformed,
                cursor: at as usize,
                node: 0,
                is_key: false,
                scalar: EMPTY_SCALAR,
            },
        }
    }
}

/// APPLE: `getContentObject`, CFPropertyList.c:913-959. The lexical bridge
/// has already distinguished comments/PIs from errors. This transition skips
/// any number of misc tokens, consumes one child, or leaves a close unconsumed.
pub fn select_misc_content(tokens: &Vec<Token>, start: usize) -> (result: ContentSelection)
    requires start <= tokens.len(),
    ensures result == select_misc_content_spec(tokens@, start as nat),
{
    let mut cursor = start;
    while cursor < tokens.len()
        invariant
            start <= cursor <= tokens.len(),
            select_misc_content_spec(tokens@, start as nat)
                == select_misc_content_spec(tokens@, cursor as nat),
        decreases tokens.len() - cursor
    {
        match tokens[cursor] {
            Token::Misc => cursor += 1,
            Token::Value { node } => return ContentSelection {
                kind: SelectionKind::Child,
                cursor: cursor + 1,
                node,
                is_key: false,
                scalar: EMPTY_SCALAR,
            },
            Token::Key { node, scalar } => return ContentSelection {
                kind: SelectionKind::Child,
                cursor: cursor + 1,
                node,
                is_key: true,
                scalar,
            },
            Token::Close { .. } => return ContentSelection {
                kind: SelectionKind::End,
                cursor,
                node: 0,
                is_key: false,
                scalar: EMPTY_SCALAR,
            },
            Token::Error => return ContentSelection {
                kind: SelectionKind::Malformed,
                cursor,
                node: 0,
                is_key: false,
                scalar: EMPTY_SCALAR,
            },
        }
    }
    ContentSelection {
        kind: SelectionKind::Eof,
        cursor,
        node: 0,
        is_key: false,
        scalar: EMPTY_SCALAR,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerStatus {
    Ok,
    Empty,
    ExtraChild,
    ExpectedKey,
    MissingValue,
    CloseMismatch,
    Malformed,
    Eof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SingleParse {
    pub status: ContainerStatus,
    pub cursor: usize,
    pub node: usize,
}

pub open spec fn plist_after_first_spec(
    tokens: Seq<Token>,
    at: nat,
    saved: nat,
    node: usize,
) -> SingleParse
    recommends tokens.len() <= usize::MAX, saved <= at,
    decreases tokens.len() - at
{
    if at >= tokens.len() {
        SingleParse { status: ContainerStatus::Eof, cursor: tokens.len() as usize, node }
    } else {
        match tokens[at as int] {
            Token::Misc => plist_after_first_spec(tokens, at + 1, saved, node),
            Token::Value { .. } | Token::Key { .. } => SingleParse {
                status: ContainerStatus::ExtraChild,
                cursor: saved as usize,
                node,
            },
            Token::Close { tag: Tag::Plist } => SingleParse {
                status: ContainerStatus::Ok,
                cursor: (at + 1) as usize,
                node,
            },
            Token::Close { .. } => SingleParse {
                status: ContainerStatus::CloseMismatch,
                cursor: at as usize,
                node,
            },
            Token::Error => SingleParse {
                status: ContainerStatus::Malformed,
                cursor: at as usize,
                node,
            },
        }
    }
}

pub open spec fn single_plist_child_spec(tokens: Seq<Token>, at: nat) -> SingleParse
    recommends tokens.len() <= usize::MAX,
    decreases tokens.len() - at
{
    if at >= tokens.len() {
        SingleParse { status: ContainerStatus::Empty, cursor: tokens.len() as usize, node: 0 }
    } else {
        match tokens[at as int] {
            Token::Misc => single_plist_child_spec(tokens, at + 1),
            Token::Value { node } | Token::Key { node, .. } => {
                plist_after_first_spec(tokens, at + 1, at + 1, node)
            },
            Token::Close { .. } => SingleParse {
                status: ContainerStatus::Empty,
                cursor: at as usize,
                node: 0,
            },
            Token::Error => SingleParse {
                status: ContainerStatus::Malformed,
                cursor: at as usize,
                node: 0,
            },
        }
    }
}

pub fn parse_plist_after_first(
    tokens: &Vec<Token>,
    start: usize,
    saved: usize,
    node: usize,
) -> (result: SingleParse)
    requires saved <= start <= tokens.len(),
    ensures result == plist_after_first_spec(tokens@, start as nat, saved as nat, node),
{
    let mut cursor = start;
    while cursor < tokens.len()
        invariant
            saved <= start <= cursor <= tokens.len(),
            plist_after_first_spec(tokens@, start as nat, saved as nat, node)
                == plist_after_first_spec(tokens@, cursor as nat, saved as nat, node),
        decreases tokens.len() - cursor
    {
        match tokens[cursor] {
            Token::Misc => cursor += 1,
            Token::Value { .. } | Token::Key { .. } => return SingleParse {
                status: ContainerStatus::ExtraChild,
                cursor: saved,
                node,
            },
            Token::Close { tag: Tag::Plist } => return SingleParse {
                status: ContainerStatus::Ok,
                cursor: cursor + 1,
                node,
            },
            Token::Close { .. } => return SingleParse {
                status: ContainerStatus::CloseMismatch,
                cursor,
                node,
            },
            Token::Error => return SingleParse {
                status: ContainerStatus::Malformed,
                cursor,
                node,
            },
        }
    }
    SingleParse { status: ContainerStatus::Eof, cursor, node }
}

/// APPLE: `parsePListTag`, CFPropertyList.c:1253-1281. On a second child the
/// cursor rolls back to the position saved immediately after the first child,
/// before misc content skipped while probing for the extra object.
pub fn parse_single_plist_child(tokens: &Vec<Token>, start: usize) -> (result: SingleParse)
    requires start <= tokens.len(),
    ensures result == single_plist_child_spec(tokens@, start as nat),
{
    let mut cursor = start;
    while cursor < tokens.len()
        invariant
            start <= cursor <= tokens.len(),
            single_plist_child_spec(tokens@, start as nat)
                == single_plist_child_spec(tokens@, cursor as nat),
        decreases tokens.len() - cursor
    {
        match tokens[cursor] {
            Token::Misc => cursor += 1,
            Token::Value { node } | Token::Key { node, .. } => {
                return parse_plist_after_first(tokens, cursor + 1, cursor + 1, node);
            },
            Token::Close { .. } => return SingleParse {
                status: ContainerStatus::Empty,
                cursor,
                node: 0,
            },
            Token::Error => return SingleParse {
                status: ContainerStatus::Malformed,
                cursor,
                node: 0,
            },
        }
    }
    SingleParse { status: ContainerStatus::Empty, cursor, node: 0 }
}

pub struct SequenceParse {
    pub status: ContainerStatus,
    pub cursor: usize,
    pub nodes: Vec<usize>,
}

pub struct SequenceModel {
    pub status: ContainerStatus,
    pub cursor: nat,
    pub nodes: Seq<usize>,
}

pub open spec fn array_sequence_state_spec(
    tokens: Seq<Token>,
    at: nat,
    nodes: Seq<usize>,
) -> SequenceModel
    decreases tokens.len() - at
{
    if at >= tokens.len() {
        SequenceModel { status: ContainerStatus::Eof, cursor: tokens.len(), nodes }
    } else {
        match tokens[at as int] {
            Token::Misc => array_sequence_state_spec(tokens, at + 1, nodes),
            Token::Value { node } | Token::Key { node, .. } => {
                array_sequence_state_spec(tokens, at + 1, nodes.push(node))
            },
            Token::Close { tag: Tag::Array } => SequenceModel {
                status: ContainerStatus::Ok,
                cursor: at + 1,
                nodes,
            },
            Token::Close { .. } => SequenceModel {
                status: ContainerStatus::CloseMismatch,
                cursor: at,
                nodes,
            },
            Token::Error => SequenceModel {
                status: ContainerStatus::Malformed,
                cursor: at,
                nodes,
            },
        }
    }
}

/// APPLE: order-preserving content loop in `parseArrayTag`,
/// CFPropertyList.c:1347-1439. Key elements are ordinary array values because
/// the source passes a null `isKey` output outside dictionary-key position.
pub fn parse_array_sequence(tokens: &Vec<Token>, start: usize) -> (result: SequenceParse)
    requires start <= tokens.len(),
    ensures
        result.status == array_sequence_state_spec(
            tokens@,
            start as nat,
            Seq::<usize>::empty(),
        ).status,
        result.cursor as nat == array_sequence_state_spec(
            tokens@,
            start as nat,
            Seq::<usize>::empty(),
        ).cursor,
        result.nodes@ == array_sequence_state_spec(
            tokens@,
            start as nat,
            Seq::<usize>::empty(),
        ).nodes,
{
    let mut cursor = start;
    let mut nodes: Vec<usize> = Vec::new();
    while cursor < tokens.len()
        invariant
            start <= cursor <= tokens.len(),
            array_sequence_state_spec(tokens@, start as nat, Seq::<usize>::empty())
                == array_sequence_state_spec(tokens@, cursor as nat, nodes@),
        decreases tokens.len() - cursor
    {
        match tokens[cursor] {
            Token::Misc => cursor += 1,
            Token::Value { node } | Token::Key { node, .. } => {
                nodes.push(node);
                cursor += 1;
            },
            Token::Close { tag: Tag::Array } => return SequenceParse {
                status: ContainerStatus::Ok,
                cursor: cursor + 1,
                nodes,
            },
            Token::Close { .. } => return SequenceParse {
                status: ContainerStatus::CloseMismatch,
                cursor,
                nodes,
            },
            Token::Error => return SequenceParse {
                status: ContainerStatus::Malformed,
                cursor,
                nodes,
            },
        }
    }
    SequenceParse { status: ContainerStatus::Eof, cursor, nodes }
}

pub open spec fn scalar_equal_spec(left: ScalarKey, right: ScalarKey) -> bool {
    left.family == right.family && left.symbol == right.symbol
}

/// Abstract scalar equality used by XML dictionaries. The lexical bridge may
/// choose any collision-free `(family, symbol)` representation; structure
/// logic relies only on the proved equivalence relation below.
pub fn scalar_equal(left: ScalarKey, right: ScalarKey) -> (equal: bool)
    ensures equal == scalar_equal_spec(left, right),
{
    left.family == right.family && left.symbol == right.symbol
}

pub proof fn scalar_equality_is_equivalence(a: ScalarKey, b: ScalarKey, c: ScalarKey)
    ensures
        scalar_equal_spec(a, a),
        scalar_equal_spec(a, b) == scalar_equal_spec(b, a),
        scalar_equal_spec(a, b) && scalar_equal_spec(b, c) ==> scalar_equal_spec(a, c),
{
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DictEntry {
    pub key_node: usize,
    pub scalar: ScalarKey,
    pub value_node: usize,
}

pub open spec fn first_equal_entry_spec(
    entries: Seq<DictEntry>,
    at: nat,
    scalar: ScalarKey,
) -> nat
    decreases entries.len() - at
{
    if at >= entries.len() {
        entries.len()
    } else if scalar_equal_spec(entries[at as int].scalar, scalar) {
        at
    } else {
        first_equal_entry_spec(entries, at + 1, scalar)
    }
}

pub fn find_equal_entry(
    entries: &Vec<DictEntry>,
    start: usize,
    scalar: ScalarKey,
) -> (index: usize)
    requires start <= entries.len(),
    ensures
        index as nat == first_equal_entry_spec(entries@, start as nat, scalar),
        start <= index <= entries.len(),
        forall|i: int| start <= i < index ==>
            !scalar_equal_spec(entries@[i].scalar, scalar),
        index < entries.len() ==> scalar_equal_spec(entries@[index as int].scalar, scalar),
{
    let mut index = start;
    while index < entries.len() && !scalar_equal(entries[index].scalar, scalar)
        invariant
            start <= index <= entries.len(),
            first_equal_entry_spec(entries@, start as nat, scalar)
                == first_equal_entry_spec(entries@, index as nat, scalar),
            forall|i: int| start <= i < index ==>
                !scalar_equal_spec(entries@[i].scalar, scalar),
        decreases entries.len() - index
    {
        index += 1;
    }
    index
}

pub open spec fn upsert_last_wins_spec(
    entries: Seq<DictEntry>,
    entry: DictEntry,
) -> Seq<DictEntry> {
    let index = first_equal_entry_spec(entries, 0, entry.scalar);
    if index < entries.len() {
        entries.update(index as int, entry)
    } else {
        entries.push(entry)
    }
}

/// APPLE: `CFDictionarySetValue`, CFPropertyList.c:1509-1515. Equal XML keys
/// replace the earlier entry at its stable position; a fresh key appends.
pub fn upsert_last_wins(entries: &mut Vec<DictEntry>, entry: DictEntry)
    ensures final(entries)@ == upsert_last_wins_spec(old(entries)@, entry),
{
    let index = find_equal_entry(entries, 0, entry.scalar);
    if index < entries.len() {
        entries[index] = entry;
    } else {
        entries.push(entry);
    }
}

pub proof fn upsert_records_latest_value(entries: Seq<DictEntry>, entry: DictEntry)
    ensures
        ({
            let index = first_equal_entry_spec(entries, 0, entry.scalar);
            let updated = upsert_last_wins_spec(entries, entry);
            if index < entries.len() {
                updated.len() == entries.len()
                    && updated[index as int] == entry
            } else {
                updated.len() == entries.len() + 1
                    && updated.last() == entry
            }
        }),
{
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DictPhase {
    Key,
    Value,
}

pub struct DictParse {
    pub status: ContainerStatus,
    pub cursor: usize,
    pub entries: Vec<DictEntry>,
}

pub struct DictModel {
    pub status: ContainerStatus,
    pub cursor: nat,
    pub entries: Seq<DictEntry>,
}

pub open spec fn dict_state_spec(
    tokens: Seq<Token>,
    at: nat,
    phase: DictPhase,
    pending_key_node: usize,
    pending_scalar: ScalarKey,
    entries: Seq<DictEntry>,
) -> DictModel
    decreases tokens.len() - at
{
    if at >= tokens.len() {
        match phase {
            DictPhase::Key => DictModel {
                status: ContainerStatus::Eof,
                cursor: tokens.len(),
                entries,
            },
            DictPhase::Value => DictModel {
                status: ContainerStatus::MissingValue,
                cursor: tokens.len(),
                entries,
            },
        }
    } else {
        match tokens[at as int] {
            Token::Misc => dict_state_spec(
                tokens,
                at + 1,
                phase,
                pending_key_node,
                pending_scalar,
                entries,
            ),
            Token::Error => DictModel {
                status: ContainerStatus::Malformed,
                cursor: at,
                entries,
            },
            Token::Close { tag } => match phase {
                DictPhase::Value => DictModel {
                    status: ContainerStatus::MissingValue,
                    cursor: at,
                    entries,
                },
                DictPhase::Key => match tag {
                    Tag::Dict => DictModel {
                        status: ContainerStatus::Ok,
                        cursor: at + 1,
                        entries,
                    },
                    _ => DictModel {
                        status: ContainerStatus::CloseMismatch,
                        cursor: at,
                        entries,
                    },
                },
            },
            Token::Key { node, scalar } => match phase {
                DictPhase::Key => dict_state_spec(
                    tokens,
                    at + 1,
                    DictPhase::Value,
                    node,
                    scalar,
                    entries,
                ),
                DictPhase::Value => {
                    let entry = DictEntry {
                        key_node: pending_key_node,
                        scalar: pending_scalar,
                        value_node: node,
                    };
                    dict_state_spec(
                        tokens,
                        at + 1,
                        DictPhase::Key,
                        0,
                        EMPTY_SCALAR,
                        upsert_last_wins_spec(entries, entry),
                    )
                },
            },
            Token::Value { node } => match phase {
                DictPhase::Key => DictModel {
                    status: ContainerStatus::ExpectedKey,
                    cursor: at + 1,
                    entries,
                },
                DictPhase::Value => {
                    let entry = DictEntry {
                        key_node: pending_key_node,
                        scalar: pending_scalar,
                        value_node: node,
                    };
                    dict_state_spec(
                        tokens,
                        at + 1,
                        DictPhase::Key,
                        0,
                        EMPTY_SCALAR,
                        upsert_last_wins_spec(entries, entry),
                    )
                },
            },
        }
    }
}

/// APPLE: alternating key/value loops in `parseDictTag`,
/// CFPropertyList.c:1441-1523, plus the `CFDictionarySetValue` last-wins
/// transition at lines 1509-1515. A `<key>` in value position is accepted as
/// a string value because the source passes a null `isKey` output there.
pub fn parse_dict_entries(tokens: &Vec<Token>, start: usize) -> (result: DictParse)
    requires start <= tokens.len(),
    ensures
        result.status == dict_state_spec(
            tokens@,
            start as nat,
            DictPhase::Key,
            0,
            EMPTY_SCALAR,
            Seq::<DictEntry>::empty(),
        ).status,
        result.cursor as nat == dict_state_spec(
            tokens@,
            start as nat,
            DictPhase::Key,
            0,
            EMPTY_SCALAR,
            Seq::<DictEntry>::empty(),
        ).cursor,
        result.entries@ == dict_state_spec(
            tokens@,
            start as nat,
            DictPhase::Key,
            0,
            EMPTY_SCALAR,
            Seq::<DictEntry>::empty(),
        ).entries,
{
    let mut cursor = start;
    let mut phase = DictPhase::Key;
    let mut pending_key_node: usize = 0;
    let mut pending_scalar = EMPTY_SCALAR;
    let mut entries: Vec<DictEntry> = Vec::new();
    while cursor < tokens.len()
        invariant
            start <= cursor <= tokens.len(),
            dict_state_spec(
                tokens@,
                start as nat,
                DictPhase::Key,
                0,
                EMPTY_SCALAR,
                Seq::<DictEntry>::empty(),
            ) == dict_state_spec(
                tokens@,
                cursor as nat,
                phase,
                pending_key_node,
                pending_scalar,
                entries@,
            ),
        decreases tokens.len() - cursor
    {
        match tokens[cursor] {
            Token::Misc => cursor += 1,
            Token::Error => return DictParse {
                status: ContainerStatus::Malformed,
                cursor,
                entries,
            },
            Token::Close { tag } => match phase {
                DictPhase::Value => return DictParse {
                    status: ContainerStatus::MissingValue,
                    cursor,
                    entries,
                },
                DictPhase::Key => match tag {
                    Tag::Dict => return DictParse {
                        status: ContainerStatus::Ok,
                        cursor: cursor + 1,
                        entries,
                    },
                    _ => return DictParse {
                        status: ContainerStatus::CloseMismatch,
                        cursor,
                        entries,
                    },
                },
            },
            Token::Key { node, scalar } => match phase {
                DictPhase::Key => {
                    pending_key_node = node;
                    pending_scalar = scalar;
                    phase = DictPhase::Value;
                    cursor += 1;
                },
                DictPhase::Value => {
                    let entry = DictEntry {
                        key_node: pending_key_node,
                        scalar: pending_scalar,
                        value_node: node,
                    };
                    upsert_last_wins(&mut entries, entry);
                    pending_key_node = 0;
                    pending_scalar = EMPTY_SCALAR;
                    phase = DictPhase::Key;
                    cursor += 1;
                },
            },
            Token::Value { node } => match phase {
                DictPhase::Key => return DictParse {
                    status: ContainerStatus::ExpectedKey,
                    cursor: cursor + 1,
                    entries,
                },
                DictPhase::Value => {
                    let entry = DictEntry {
                        key_node: pending_key_node,
                        scalar: pending_scalar,
                        value_node: node,
                    };
                    upsert_last_wins(&mut entries, entry);
                    pending_key_node = 0;
                    pending_scalar = EMPTY_SCALAR;
                    phase = DictPhase::Key;
                    cursor += 1;
                },
            },
        }
    }
    match phase {
        DictPhase::Key => DictParse {
            status: ContainerStatus::Eof,
            cursor,
            entries,
        },
        DictPhase::Value => DictParse {
            status: ContainerStatus::MissingValue,
            cursor,
            entries,
        },
    }
}

/// Reserved collision-free scalar supplied by the lexical bridge for the
/// normalized XML string `CF$UID`. `family == 0` is reserved for the empty
/// sentinel, so a bridge implementation must not assign this pair elsewhere.
pub const CF_UID_SCALAR: ScalarKey = ScalarKey {
    family: 1,
    symbol: 0x4346_2455_4944,
};

/// Allocation-independent observation of one already-built CF node.
///
/// Bridge boundary: `is_number` means exactly
/// `CFGetTypeID(node) == CFNumberGetTypeID()`. When it is true,
/// `sint32_bits` is the 32-bit output written by `CFNumberGetValue` with
/// `kCFNumberSInt32Type`, reinterpreted as the source's `uint32_t v`. Proving
/// that CF runtime observation is outside this structural module; the rewrite
/// below proves the complete decision once those observations are supplied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbstractNode {
    pub is_number: bool,
    pub sint32_bits: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DictionaryResult {
    Dictionary,
    Uid { value: u32 },
}

/// Independent mathematical decision for the keyed-archive UID special case.
/// An out-of-range node id is totalized to the ordinary dictionary result;
/// reachable parser states must instead provide a valid abstract node table.
pub open spec fn rewrite_single_cf_uid_spec(
    entries: Seq<DictEntry>,
    nodes: Seq<AbstractNode>,
) -> DictionaryResult {
    if entries.len() != 1 {
        DictionaryResult::Dictionary
    } else {
        let entry = entries[0];
        if !scalar_equal_spec(entry.scalar, CF_UID_SCALAR)
            || entry.value_node as nat >= nodes.len()
        {
            DictionaryResult::Dictionary
        } else {
            let node = nodes[entry.value_node as int];
            if node.is_number {
                DictionaryResult::Uid { value: node.sint32_bits }
            } else {
                DictionaryResult::Dictionary
            }
        }
    }
}

/// APPLE: the single-entry `CF$UID` dictionary rewrite in `parseDictTag`,
/// CFPropertyList.c:1536-1547. This runs after `upsert_last_wins`, matching
/// `CFDictionaryGetCount` and `CFDictionaryGetValue` after duplicate collapse.
pub fn rewrite_single_cf_uid(
    entries: &Vec<DictEntry>,
    nodes: &Vec<AbstractNode>,
) -> (result: DictionaryResult)
    ensures result == rewrite_single_cf_uid_spec(entries@, nodes@),
{
    if entries.len() != 1 {
        return DictionaryResult::Dictionary;
    }
    let entry = entries[0];
    if !scalar_equal(entry.scalar, CF_UID_SCALAR) {
        return DictionaryResult::Dictionary;
    }
    if entry.value_node >= nodes.len() {
        return DictionaryResult::Dictionary;
    }
    let node = nodes[entry.value_node];
    if node.is_number {
        DictionaryResult::Uid { value: node.sint32_bits }
    } else {
        DictionaryResult::Dictionary
    }
}

pub proof fn uid_rewrite_has_exact_guard(
    entries: Seq<DictEntry>,
    nodes: Seq<AbstractNode>,
)
    ensures
        match rewrite_single_cf_uid_spec(entries, nodes) {
            DictionaryResult::Uid { value } =>
                entries.len() == 1
                    && scalar_equal_spec(entries[0].scalar, CF_UID_SCALAR)
                    && (entries[0].value_node as nat) < nodes.len()
                    && nodes[entries[0].value_node as int].is_number
                    && value == nodes[entries[0].value_node as int].sint32_bits,
            DictionaryResult::Dictionary => true,
        },
{
}

fn main() {
    let bytes = vec![b'a', b'r', b'r', b'a', b'y', b'/', b'>'];
    let _tag = scan_start_tag(&bytes, 0);
}

} // verus!
