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
 */

/*	CFPropertyList.c
	Copyright (c) 1999-2014, Apple Inc. All rights reserved.
	Responsibility: Tony Parker
*/

/*	CFDate.c
	Copyright (c) 1998-2014, Apple Inc. All rights reserved.
	Responsibility: Christopher Kane
*/

/*
 * Modified 2026-08-22 by plist-rs contributors: translated the
 * pointer-free XML lexical and scalar reader core to executable Rust/Verus,
 * made the defined-execution boundary explicit, and added machine-checked
 * functional-correctness proofs.
 */

//! Executable Verus port of the pointer-free XML-plist reader core in
//! `opensource-apple/CF@3cc41a76.../CFPropertyList.c` and the Gregorian
//! helpers it calls in `CFDate.c`.
//!
//! Each `APPLE:` comment maps the implementation to the pinned C line range.
//! Specs are mathematical models independent of the executable loops.  The
//! individual executable functions are source-traced to defined executions of
//! those mapped fragments. The calendar functions stop at exact integer UTC
//! seconds; they do not claim equivalence for C's final binary64 rounding. At
//! a C undefined-behavior edge the port returns the documented safe result.

use vstd::prelude::*;
use vstd::wrapping::u16_specs;

verus! {

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorScan {
    pub found: bool,
    pub cursor: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DtdScan {
    Closed { cursor: usize },
    InlineSubset { cursor: usize },
    Malformed { cursor: usize },
    Eof { cursor: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityFold {
    Complete { cursor: usize, value: u16 },
    Invalid { cursor: usize, value: u16 },
    Eof { cursor: usize, value: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Base64Quad {
    pub first: u8,
    pub second: u8,
    pub third: u8,
    pub len: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TwoDigit {
    pub ok: bool,
    pub cursor: usize,
    pub value: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegerScan {
    Value { cursor: usize, negative: bool, magnitude: u64 },
    Empty { cursor: usize },
    Incomplete { cursor: usize },
    PrematureEof { cursor: usize },
    InvalidDigit { cursor: usize },
    DecimalHexDigit { cursor: usize },
    Overflow { cursor: usize },
    Underflow { cursor: usize },
}

pub open spec fn xml_whitespace_spec(byte: u8) -> bool {
    byte == b' ' || byte == b'\t' || byte == b'\n' || byte == b'\r'
}

/// APPLE: `skipWhitespace`, CFPropertyList.c:759-773.
pub fn is_xml_whitespace(byte: u8) -> (yes: bool)
    ensures yes == xml_whitespace_spec(byte),
{
    byte == b' ' || byte == b'\t' || byte == b'\n' || byte == b'\r'
}

pub open spec fn skip_xml_whitespace_spec(bytes: Seq<u8>, start: nat) -> nat
    recommends start <= bytes.len(),
    decreases bytes.len() - start
{
    if start >= bytes.len() {
        bytes.len()
    } else if !xml_whitespace_spec(bytes[start as int]) {
        start
    } else {
        skip_xml_whitespace_spec(bytes, start + 1)
    }
}

/// APPLE: `skipWhitespace`, CFPropertyList.c:759-773.
pub fn skip_xml_whitespace(bytes: &Vec<u8>, start: usize) -> (cursor: usize)
    requires start <= bytes.len(),
    ensures
        cursor as nat == skip_xml_whitespace_spec(bytes@, start as nat),
        start <= cursor <= bytes.len(),
        forall|i: int| start <= i < cursor ==> xml_whitespace_spec(bytes@[i]),
        cursor < bytes.len() ==> !xml_whitespace_spec(bytes@[cursor as int]),
{
    let mut cursor = start;
    while cursor < bytes.len() && is_xml_whitespace(bytes[cursor])
        invariant
            start <= cursor <= bytes.len(),
            skip_xml_whitespace_spec(bytes@, start as nat)
                == skip_xml_whitespace_spec(bytes@, cursor as nat),
            forall|i: int| start <= i < cursor ==> xml_whitespace_spec(bytes@[i]),
        decreases bytes.len() - cursor
    {
        cursor += 1;
    }
    cursor
}

pub open spec fn find_byte_spec(bytes: Seq<u8>, start: nat, needle: u8) -> nat
    recommends start <= bytes.len(),
    decreases bytes.len() - start
{
    if start >= bytes.len() {
        bytes.len()
    } else if bytes[start as int] == needle {
        start
    } else {
        find_byte_spec(bytes, start + 1, needle)
    }
}

/// APPLE: byte-at-a-time searches used at CFPropertyList.c:781-787,
/// 794-800, 816-824, 848-854, and 884-892.
pub fn find_byte(bytes: &Vec<u8>, start: usize, needle: u8) -> (cursor: usize)
    requires start <= bytes.len(),
    ensures
        cursor as nat == find_byte_spec(bytes@, start as nat, needle),
        start <= cursor <= bytes.len(),
        forall|i: int| start <= i < cursor ==> bytes@[i] != needle,
        cursor < bytes.len() ==> bytes@[cursor as int] == needle,
{
    let mut cursor = start;
    while cursor < bytes.len() && bytes[cursor] != needle
        invariant
            start <= cursor <= bytes.len(),
            find_byte_spec(bytes@, start as nat, needle)
                == find_byte_spec(bytes@, cursor as nat, needle),
            forall|i: int| start <= i < cursor ==> bytes@[i] != needle,
        decreases bytes.len() - cursor
    {
        cursor += 1;
    }
    cursor
}

pub open spec fn comment_scan_spec(bytes: Seq<u8>, start: nat, at: nat) -> CursorScan
    recommends start <= at <= bytes.len(), bytes.len() <= usize::MAX,
    decreases bytes.len() - at
{
    // The strict inequality preserves the source's `p < end - 3` boundary.
    if bytes.len() - at <= 3 {
        CursorScan { found: false, cursor: start as usize }
    } else if bytes[at as int] == b'-'
        && bytes[(at + 1) as int] == b'-'
        && bytes[(at + 2) as int] == b'>'
    {
        CursorScan { found: true, cursor: (at + 3) as usize }
    } else {
        comment_scan_spec(bytes, start, at + 1)
    }
}

/// APPLE: `skipXMLComment`, CFPropertyList.c:777-789.
pub fn skip_xml_comment(bytes: &Vec<u8>, start: usize) -> (result: CursorScan)
    requires start <= bytes.len(),
    ensures result == comment_scan_spec(bytes@, start as nat, start as nat),
{
    let mut cursor = start;
    while bytes.len() - cursor > 3
        invariant
            start <= cursor <= bytes.len(),
            comment_scan_spec(bytes@, start as nat, start as nat)
                == comment_scan_spec(bytes@, start as nat, cursor as nat),
        decreases bytes.len() - cursor
    {
        if bytes[cursor] == b'-'
            && bytes[cursor + 1] == b'-'
            && bytes[cursor + 2] == b'>'
        {
            return CursorScan { found: true, cursor: cursor + 3 };
        }
        cursor += 1;
    }
    CursorScan { found: false, cursor: start }
}

pub open spec fn pi_scan_spec(bytes: Seq<u8>, start: nat, at: nat) -> CursorScan
    recommends start <= at <= bytes.len(), bytes.len() <= usize::MAX,
    decreases bytes.len() - at
{
    // The strict inequality preserves the source's `curr < end - 2` boundary.
    if bytes.len() - at <= 2 {
        CursorScan { found: false, cursor: start as usize }
    } else if bytes[at as int] == b'?' && bytes[(at + 1) as int] == b'>' {
        CursorScan { found: true, cursor: (at + 2) as usize }
    } else {
        pi_scan_spec(bytes, start, at + 1)
    }
}

/// APPLE: `skipXMLProcessingInstruction`, CFPropertyList.c:791-803.
pub fn skip_xml_processing_instruction(
    bytes: &Vec<u8>,
    start: usize,
) -> (result: CursorScan)
    requires start <= bytes.len(),
    ensures result == pi_scan_spec(bytes@, start as nat, start as nat),
{
    let mut cursor = start;
    while bytes.len() - cursor > 2
        invariant
            start <= cursor <= bytes.len(),
            pi_scan_spec(bytes@, start as nat, start as nat)
                == pi_scan_spec(bytes@, start as nat, cursor as nat),
        decreases bytes.len() - cursor
    {
        if bytes[cursor] == b'?' && bytes[cursor + 1] == b'>' {
            return CursorScan { found: true, cursor: cursor + 2 };
        }
        cursor += 1;
    }
    CursorScan { found: false, cursor: start }
}

pub open spec fn quote_blind_decl_spec(bytes: Seq<u8>, start: nat) -> CursorScan
    recommends start <= bytes.len(), bytes.len() <= usize::MAX,
{
    let at = find_byte_spec(bytes, start, b'>');
    if at >= bytes.len() {
        CursorScan { found: false, cursor: start as usize }
    } else {
        CursorScan { found: true, cursor: (at + 1) as usize }
    }
}

/// APPLE: the deliberately quote-blind declaration scan in `skipInlineDTD`,
/// CFPropertyList.c:882-892. A `>` byte closes even inside a quoted literal.
pub fn scan_quote_blind_dtd_declaration(
    bytes: &Vec<u8>,
    start: usize,
) -> (result: CursorScan)
    requires start <= bytes.len(),
    ensures result == quote_blind_decl_spec(bytes@, start as nat),
{
    let at = find_byte(bytes, start, b'>');
    if at >= bytes.len() {
        CursorScan { found: false, cursor: start }
    } else {
        CursorScan { found: true, cursor: at + 1 }
    }
}

pub open spec fn has_doctype_spec(bytes: Seq<u8>, start: nat) -> bool
    recommends start <= bytes.len(),
{
    bytes.len() - start >= 7
        && bytes[start as int] == b'D'
        && bytes[(start + 1) as int] == b'O'
        && bytes[(start + 2) as int] == b'C'
        && bytes[(start + 3) as int] == b'T'
        && bytes[(start + 4) as int] == b'Y'
        && bytes[(start + 5) as int] == b'P'
        && bytes[(start + 6) as int] == b'E'
}

pub fn has_doctype(bytes: &Vec<u8>, start: usize) -> (yes: bool)
    requires start <= bytes.len(),
    ensures yes == has_doctype_spec(bytes@, start as nat),
{
    bytes.len() - start >= 7
        && bytes[start] == b'D'
        && bytes[start + 1] == b'O'
        && bytes[start + 2] == b'C'
        && bytes[start + 3] == b'T'
        && bytes[start + 4] == b'Y'
        && bytes[start + 5] == b'P'
        && bytes[start + 6] == b'E'
}

pub open spec fn dtd_body_spec(bytes: Seq<u8>, at: nat) -> DtdScan
    recommends at <= bytes.len(), bytes.len() <= usize::MAX,
    decreases bytes.len() - at
{
    if at >= bytes.len() {
        DtdScan::Eof { cursor: at as usize }
    } else if bytes[at as int] == b'[' {
        DtdScan::InlineSubset { cursor: at as usize }
    } else if bytes[at as int] == b'>' {
        DtdScan::Closed { cursor: (at + 1) as usize }
    } else {
        dtd_body_spec(bytes, at + 1)
    }
}

pub open spec fn skip_dtd_spec(bytes: Seq<u8>, start: nat) -> DtdScan
    recommends start <= bytes.len(), bytes.len() <= usize::MAX,
{
    if !has_doctype_spec(bytes, start) {
        DtdScan::Malformed { cursor: start as usize }
    } else {
        let body = skip_xml_whitespace_spec(bytes, start + 7);
        dtd_body_spec(bytes, body)
    }
}

/// APPLE: the outer, quote-blind `skipDTD` pass, CFPropertyList.c:805-828.
/// The inline-subset arm exposes the source cursor at `[`, immediately before
/// the CF-runtime-dependent `skipInlineDTD` dispatch at lines 830-843.
pub fn skip_dtd(bytes: &Vec<u8>, start: usize) -> (result: DtdScan)
    requires start <= bytes.len(),
    ensures result == skip_dtd_spec(bytes@, start as nat),
{
    proof {
        assert(bytes@.len() <= usize::MAX);
    }
    if !has_doctype(bytes, start) {
        return DtdScan::Malformed { cursor: start };
    }
    let body = skip_xml_whitespace(bytes, start + 7);
    assert(skip_dtd_spec(bytes@, start as nat) == dtd_body_spec(bytes@, body as nat));
    let mut cursor = body;
    while cursor < bytes.len()
        invariant
            body <= cursor <= bytes.len(),
            skip_dtd_spec(bytes@, start as nat) == dtd_body_spec(bytes@, cursor as nat),
        decreases bytes.len() - cursor
    {
        if bytes[cursor] == b'[' {
            assert(dtd_body_spec(bytes@, cursor as nat)
                == DtdScan::InlineSubset { cursor });
            return DtdScan::InlineSubset { cursor };
        }
        if bytes[cursor] == b'>' {
            assert(dtd_body_spec(bytes@, cursor as nat)
                == DtdScan::Closed { cursor: (cursor + 1) as usize });
            return DtdScan::Closed { cursor: cursor + 1 };
        }
        cursor += 1;
    }
    DtdScan::Eof { cursor }
}

pub open spec fn entity_digit_spec(byte: u8, hexadecimal: bool) -> Option<u16> {
    if b'0' <= byte && byte <= b'9' {
        Some((byte - b'0') as u16)
    } else if hexadecimal && b'a' <= byte && byte <= b'f' {
        Some((byte - b'a' + 10) as u16)
    } else if hexadecimal && b'A' <= byte && byte <= b'F' {
        Some((byte - b'A' + 10) as u16)
    } else {
        None
    }
}

pub fn entity_digit(byte: u8, hexadecimal: bool) -> (digit: Option<u16>)
    ensures digit == entity_digit_spec(byte, hexadecimal),
{
    if b'0' <= byte && byte <= b'9' {
        Some((byte - b'0') as u16)
    } else if hexadecimal && b'a' <= byte && byte <= b'f' {
        Some((byte - b'a' + 10) as u16)
    } else if hexadecimal && b'A' <= byte && byte <= b'F' {
        Some((byte - b'A' + 10) as u16)
    } else {
        None
    }
}

pub open spec fn entity_fold_spec(
    bytes: Seq<u8>,
    at: nat,
    hexadecimal: bool,
    value: u16,
) -> EntityFold
    recommends at <= bytes.len(), bytes.len() <= usize::MAX,
    decreases bytes.len() - at
{
    if at >= bytes.len() {
        EntityFold::Eof { cursor: at as usize, value }
    } else if bytes[at as int] == b';' {
        EntityFold::Complete { cursor: (at + 1) as usize, value }
    } else {
        let shifted = if hexadecimal {
            u16_specs::wrapping_shl(value, 4)
        } else {
            u16_specs::wrapping_mul(value, 10)
        };
        match entity_digit_spec(bytes[at as int], hexadecimal) {
            Some(digit) => entity_fold_spec(
                bytes,
                at + 1,
                hexadecimal,
                u16_specs::wrapping_add(shifted, digit),
            ),
            None => EntityFold::Invalid { cursor: (at + 1) as usize, value: shifted },
        }
    }
}

/// APPLE: numeric arm of `parseEntityReference_pl`,
/// CFPropertyList.c:1047-1090. `start` is immediately after `#` or `#x`.
pub fn fold_numeric_entity(
    bytes: &Vec<u8>,
    start: usize,
    hexadecimal: bool,
) -> (result: EntityFold)
    requires start <= bytes.len(),
    ensures result == entity_fold_spec(bytes@, start as nat, hexadecimal, 0),
{
    let mut cursor = start;
    let mut value: u16 = 0;
    while cursor < bytes.len()
        invariant
            start <= cursor <= bytes.len(),
            entity_fold_spec(bytes@, start as nat, hexadecimal, 0)
                == entity_fold_spec(bytes@, cursor as nat, hexadecimal, value),
        decreases bytes.len() - cursor
    {
        let byte = bytes[cursor];
        cursor += 1;
        if byte == b';' {
            return EntityFold::Complete { cursor, value };
        }
        value = if hexadecimal {
            value.wrapping_shl(4)
        } else {
            value.wrapping_mul(10)
        };
        match entity_digit(byte, hexadecimal) {
            Some(digit) => value = value.wrapping_add(digit),
            None => return EntityFold::Invalid { cursor, value },
        }
    }
    EntityFold::Eof { cursor, value }
}

pub open spec fn base64_value_spec(byte: u8) -> Option<u8> {
    if b'A' <= byte && byte <= b'Z' {
        Some((byte - b'A') as u8)
    } else if b'a' <= byte && byte <= b'z' {
        Some((byte - b'a' + 26) as u8)
    } else if b'0' <= byte && byte <= b'9' {
        Some((byte - b'0' + 52) as u8)
    } else if byte == b'+' {
        Some(62)
    } else if byte == b'/' {
        Some(63)
    } else if byte == b'=' {
        Some(0)
    } else {
        None
    }
}

/// APPLE: `dataDecodeTable`, CFPropertyList.c:1568-1585.
pub fn base64_value(byte: u8) -> (value: Option<u8>)
    ensures value == base64_value_spec(byte),
{
    if b'A' <= byte && byte <= b'Z' {
        Some(byte - b'A')
    } else if b'a' <= byte && byte <= b'z' {
        Some(byte - b'a' + 26)
    } else if b'0' <= byte && byte <= b'9' {
        Some(byte - b'0' + 52)
    } else if byte == b'+' {
        Some(62)
    } else if byte == b'/' {
        Some(63)
    } else if byte == b'=' {
        Some(0)
    } else {
        None
    }
}

pub open spec fn base64_word_spec(a: u8, b: u8, c: u8, d: u8) -> int {
    a as int * 262_144 + b as int * 4_096 + c as int * 64 + d as int
}

pub open spec fn base64_quad_spec(a: u8, b: u8, c: u8, d: u8, equals: u8) -> Base64Quad
    recommends a < 64 && b < 64 && c < 64 && d < 64,
{
    let word = base64_word_spec(a, b, c, d);
    Base64Quad {
        first: ((word / 65_536) % 256) as u8,
        second: ((word / 256) % 256) as u8,
        third: (word % 256) as u8,
        len: if equals < 1 { 3 } else if equals < 2 { 2 } else { 1 },
    }
}

/// APPLE: the four-value accumulator and conditional emission in
/// `parseDataTag`, CFPropertyList.c:1606-1625. Integer arithmetic expresses
/// the first C quartet without signed-overflow ambiguity.
pub fn emit_base64_quad(a: u8, b: u8, c: u8, d: u8, equals: u8) -> (out: Base64Quad)
    requires a < 64, b < 64, c < 64, d < 64,
    ensures out == base64_quad_spec(a, b, c, d, equals),
{
    let word: u32 = (a as u32) * 262_144
        + (b as u32) * 4_096
        + (c as u32) * 64
        + d as u32;
    Base64Quad {
        first: ((word / 65_536) % 256) as u8,
        second: ((word / 256) % 256) as u8,
        third: (word % 256) as u8,
        len: if equals < 1 { 3 } else if equals < 2 { 2 } else { 1 },
    }
}

pub open spec fn two_digit_spec(bytes: Seq<u8>, start: nat) -> TwoDigit
    recommends start <= bytes.len(), bytes.len() <= usize::MAX,
{
    // The source uses `curr + 2 >= end`, so exactly two remaining bytes fail.
    if bytes.len() - start <= 2 {
        TwoDigit { ok: false, cursor: start as usize, value: 0 }
    } else if b'0' <= bytes[start as int] && bytes[start as int] <= b'9'
        && b'0' <= bytes[(start + 1) as int] && bytes[(start + 1) as int] <= b'9'
    {
        TwoDigit {
            ok: true,
            cursor: (start + 2) as usize,
            value: ((bytes[start as int] - b'0') * 10
                + (bytes[(start + 1) as int] - b'0')) as u8,
        }
    } else {
        TwoDigit { ok: false, cursor: (start + 2) as usize, value: 0 }
    }
}

/// APPLE: `read2DigitNumber`, CFPropertyList.c:1654-1667.
pub fn read_two_digit_number(bytes: &Vec<u8>, start: usize) -> (result: TwoDigit)
    requires start <= bytes.len(),
    ensures result == two_digit_spec(bytes@, start as nat),
{
    if bytes.len() - start <= 2 {
        return TwoDigit { ok: false, cursor: start, value: 0 };
    }
    let first = bytes[start];
    let second = bytes[start + 1];
    let cursor = start + 2;
    if first < b'0' || first > b'9' || second < b'0' || second > b'9' {
        TwoDigit { ok: false, cursor, value: 0 }
    } else {
        TwoDigit {
            ok: true,
            cursor,
            value: (first - b'0') * 10 + (second - b'0'),
        }
    }
}

pub open spec fn cf_integer_whitespace_at_spec(bytes: Seq<u8>, at: nat) -> bool {
    if at >= bytes.len() {
        false
    } else {
        let first = bytes[at as int];
        first < 0x21
            || (first > 0x7e && first < 0xa1)
            || (bytes.len() - at >= 3
                && ((first == 0xe2
                        && bytes[(at + 1) as int] == 0x80
                        && ((80 <= bytes[(at + 2) as int]
                                && bytes[(at + 2) as int] <= 0x8b)
                            || bytes[(at + 2) as int] == 0xaf))
                    || (first == 0xe2
                        && bytes[(at + 1) as int] == 0x81
                        && bytes[(at + 2) as int] == 0x9f)
                    || (first == 0xe3
                        && bytes[(at + 1) as int] == 0x80
                        && bytes[(at + 2) as int] == 0x80)))
    }
}

/// APPLE: `isWhitespace`, CFPropertyList.c:1829-1868. Notice that the
/// historical lower bound is decimal `80` (`0x50`), not `0x80`.
pub fn is_cf_integer_whitespace_at(bytes: &Vec<u8>, at: usize) -> (yes: bool)
    requires at <= bytes.len(),
    ensures yes == cf_integer_whitespace_at_spec(bytes@, at as nat),
{
    if at >= bytes.len() {
        return false;
    }
    let first = bytes[at];
    if first < 0x21 || (first > 0x7e && first < 0xa1) {
        return true;
    }
    if bytes.len() - at < 3 {
        return false;
    }
    let second = bytes[at + 1];
    let third = bytes[at + 2];
    if first == 0xe2 && second == 0x80 {
        (80 <= third && third <= 0x8b) || third == 0xaf
    } else if first == 0xe2 && second == 0x81 {
        third == 0x9f
    } else if first == 0xe3 && second == 0x80 && third == 0x80 {
        true
    } else {
        false
    }
}

pub open spec fn skip_cf_integer_whitespace_spec(bytes: Seq<u8>, at: nat) -> nat
    decreases bytes.len() - at
{
    if at >= bytes.len() || !cf_integer_whitespace_at_spec(bytes, at) {
        at
    } else {
        skip_cf_integer_whitespace_spec(bytes, at + 1)
    }
}

/// APPLE: the one-byte-at-a-time whitespace loops in `parseIntegerTag`,
/// CFPropertyList.c:1877 and 1883-1887.
pub fn skip_cf_integer_whitespace(bytes: &Vec<u8>, start: usize) -> (cursor: usize)
    requires start <= bytes.len(),
    ensures
        cursor as nat == skip_cf_integer_whitespace_spec(bytes@, start as nat),
        start <= cursor <= bytes.len(),
{
    let mut cursor = start;
    while cursor < bytes.len() && is_cf_integer_whitespace_at(bytes, cursor)
        invariant
            start <= cursor <= bytes.len(),
            skip_cf_integer_whitespace_spec(bytes@, start as nat)
                == skip_cf_integer_whitespace_spec(bytes@, cursor as nat),
        decreases bytes.len() - cursor
    {
        cursor += 1;
    }
    cursor
}

pub open spec fn integer_digit_spec(byte: u8) -> Option<u8> {
    if b'0' <= byte && byte <= b'9' {
        Some((byte - b'0') as u8)
    } else if b'a' <= byte && byte <= b'f' {
        Some((byte - b'a' + 10) as u8)
    } else if b'A' <= byte && byte <= b'F' {
        Some((byte - b'A' + 10) as u8)
    } else {
        None
    }
}

pub fn integer_digit(byte: u8) -> (digit: Option<u8>)
    ensures digit == integer_digit_spec(byte),
{
    if b'0' <= byte && byte <= b'9' {
        Some(byte - b'0')
    } else if b'a' <= byte && byte <= b'f' {
        Some(byte - b'a' + 10)
    } else if b'A' <= byte && byte <= b'F' {
        Some(byte - b'A' + 10)
    } else {
        None
    }
}

pub open spec fn integer_digits_spec(
    bytes: Seq<u8>,
    at: nat,
    radix: u8,
    negative: bool,
    magnitude: u64,
) -> IntegerScan
    recommends bytes.len() <= usize::MAX, radix == 10 || radix == 16,
    decreases bytes.len() - at
{
    if at >= bytes.len() {
        IntegerScan::PrematureEof { cursor: bytes.len() as usize }
    } else if bytes[at as int] == b'<' {
        IntegerScan::Value { cursor: at as usize, negative, magnitude }
    } else {
        match integer_digit_spec(bytes[at as int]) {
            None => IntegerScan::InvalidDigit { cursor: at as usize },
            Some(digit) => {
                if radix == 10 && digit > 9 {
                    IntegerScan::DecimalHexDigit { cursor: at as usize }
                } else {
                    let exact = magnitude as int * radix as int + digit as int;
                    if exact > u64::MAX as int {
                        IntegerScan::Overflow { cursor: at as usize }
                    } else if negative && exact > i64::MAX as int + 1 {
                        IntegerScan::Underflow { cursor: at as usize }
                    } else {
                        integer_digits_spec(
                            bytes,
                            at + 1,
                            radix,
                            negative,
                            exact as u64,
                        )
                    }
                }
            }
        }
    }
}

/// APPLE: digit switch, overflow checks, and accumulator in
/// `parseIntegerTag`, CFPropertyList.c:1921-1959.
pub fn scan_integer_digits(
    bytes: &Vec<u8>,
    start: usize,
    radix: u8,
    negative: bool,
) -> (result: IntegerScan)
    requires start <= bytes.len(), radix == 10 || radix == 16,
    ensures result == integer_digits_spec(bytes@, start as nat, radix, negative, 0),
{
    let mut cursor = start;
    let mut magnitude: u64 = 0;
    while cursor < bytes.len()
        invariant
            start <= cursor <= bytes.len(),
            radix == 10 || radix == 16,
            integer_digits_spec(bytes@, start as nat, radix, negative, 0)
                == integer_digits_spec(bytes@, cursor as nat, radix, negative, magnitude),
        decreases bytes.len() - cursor
    {
        let byte = bytes[cursor];
        if byte == b'<' {
            return IntegerScan::Value { cursor, negative, magnitude };
        }
        let digit = match integer_digit(byte) {
            None => return IntegerScan::InvalidDigit { cursor },
            Some(digit) => digit,
        };
        if radix == 10 && digit > 9 {
            return IntegerScan::DecimalHexDigit { cursor };
        }
        proof {
            assert(magnitude as int * radix as int + digit as int <= u128::MAX as int)
                by (nonlinear_arith);
        }
        // The two source checks are equivalent to this exact widened check.
        let exact = (magnitude as u128) * (radix as u128) + (digit as u128);
        if exact > u64::MAX as u128 {
            return IntegerScan::Overflow { cursor };
        }
        let next = exact as u64;
        if negative && i64::MAX as u64 + 1 < next {
            return IntegerScan::Underflow { cursor };
        }
        magnitude = next;
        cursor += 1;
    }
    IntegerScan::PrematureEof { cursor }
}

pub open spec fn integer_zero_tail_spec(
    bytes: Seq<u8>,
    at: nat,
    negative: bool,
    hexadecimal: bool,
    had_leading_zero: bool,
) -> IntegerScan
    recommends bytes.len() <= usize::MAX,
    decreases bytes.len() - at
{
    if at >= bytes.len() {
        IntegerScan::PrematureEof { cursor: bytes.len() as usize }
    } else if bytes[at as int] == b'0' {
        integer_zero_tail_spec(bytes, at + 1, negative, hexadecimal, true)
    } else if bytes[at as int] == b'<' {
        if had_leading_zero {
            IntegerScan::Value { cursor: at as usize, negative, magnitude: 0 }
        } else {
            IntegerScan::Incomplete { cursor: at as usize }
        }
    } else {
        integer_digits_spec(bytes, at, if hexadecimal { 16 } else { 10 }, negative, 0)
    }
}

pub open spec fn integer_after_sign_spec(
    bytes: Seq<u8>,
    at: nat,
    negative: bool,
) -> IntegerScan
    recommends bytes.len() <= usize::MAX,
{
    if at >= bytes.len() {
        IntegerScan::PrematureEof { cursor: bytes.len() as usize }
    } else if bytes[at as int] == b'0' {
        if at + 1 < bytes.len()
            && (bytes[(at + 1) as int] == b'x' || bytes[(at + 1) as int] == b'X')
        {
            integer_zero_tail_spec(bytes, at + 2, negative, true, false)
        } else {
            integer_zero_tail_spec(bytes, at + 1, negative, false, true)
        }
    } else {
        integer_zero_tail_spec(bytes, at, negative, false, false)
    }
}

pub fn scan_integer_after_sign(
    bytes: &Vec<u8>,
    start: usize,
    negative: bool,
) -> (result: IntegerScan)
    requires start <= bytes.len(),
    ensures result == integer_after_sign_spec(bytes@, start as nat, negative),
{
    if start >= bytes.len() {
        return IntegerScan::PrematureEof { cursor: bytes.len() };
    }
    let mut cursor = start;
    let mut hexadecimal = false;
    let mut had_leading_zero = false;
    if bytes[cursor] == b'0' {
        if bytes.len() - cursor > 1
            && (bytes[cursor + 1] == b'x' || bytes[cursor + 1] == b'X')
        {
            hexadecimal = true;
            cursor += 2;
        } else {
            had_leading_zero = true;
            cursor += 1;
        }
    }
    assert(integer_after_sign_spec(bytes@, start as nat, negative)
        == integer_zero_tail_spec(
            bytes@,
            cursor as nat,
            negative,
            hexadecimal,
            had_leading_zero,
        ));
    while cursor < bytes.len() && bytes[cursor] == b'0'
        invariant
            start <= cursor <= bytes.len(),
            integer_after_sign_spec(bytes@, start as nat, negative)
                == integer_zero_tail_spec(
                    bytes@,
                    cursor as nat,
                    negative,
                    hexadecimal,
                    had_leading_zero,
                ),
        decreases bytes.len() - cursor
    {
        had_leading_zero = true;
        cursor += 1;
    }
    if cursor >= bytes.len() {
        return IntegerScan::PrematureEof { cursor };
    }
    if bytes[cursor] == b'<' {
        if had_leading_zero {
            IntegerScan::Value { cursor, negative, magnitude: 0 }
        } else {
            IntegerScan::Incomplete { cursor }
        }
    } else {
        scan_integer_digits(bytes, cursor, if hexadecimal { 16 } else { 10 }, negative)
    }
}

pub open spec fn integer_scan_spec(bytes: Seq<u8>, start: nat) -> IntegerScan
    recommends start <= bytes.len(), bytes.len() <= usize::MAX,
{
    let first = skip_cf_integer_whitespace_spec(bytes, start);
    if first >= bytes.len() {
        IntegerScan::PrematureEof { cursor: bytes.len() as usize }
    } else if bytes[first as int] == b'<' {
        IntegerScan::Empty { cursor: first as usize }
    } else if bytes[first as int] == b'-' || bytes[first as int] == b'+' {
        let after_sign = skip_cf_integer_whitespace_spec(bytes, first + 1);
        integer_after_sign_spec(bytes, after_sign, bytes[first as int] == b'-')
    } else {
        integer_after_sign_spec(bytes, first, false)
    }
}

/// APPLE: pointer-free lexical portion of `parseIntegerTag`,
/// CFPropertyList.c:1870-1959. The returned cursor remains on the `<`; CF tag
/// matching and CFNumber allocation (lines 1960-1979) are intentionally out
/// of this lexical contract.
pub fn scan_cf_integer(bytes: &Vec<u8>, start: usize) -> (result: IntegerScan)
    requires start <= bytes.len(),
    ensures result == integer_scan_spec(bytes@, start as nat),
{
    let first = skip_cf_integer_whitespace(bytes, start);
    if first >= bytes.len() {
        return IntegerScan::PrematureEof { cursor: bytes.len() };
    }
    let byte = bytes[first];
    if byte == b'<' {
        return IntegerScan::Empty { cursor: first };
    }
    if byte == b'-' || byte == b'+' {
        let after_sign = skip_cf_integer_whitespace(bytes, first + 1);
        scan_integer_after_sign(bytes, after_sign, byte == b'-')
    } else {
        scan_integer_after_sign(bytes, first, false)
    }
}

pub open spec fn c_remainder_400_spec(value: int) -> int {
    if value >= 0 {
        value % 400
    } else {
        -((-value) % 400)
    }
}

pub open spec fn cf_leap_year_spec(relative_year: int) -> bool {
    let remainder = c_remainder_400_spec(relative_year + 1);
    let magnitude = if remainder < 0 { -remainder } else { remainder };
    magnitude % 4 == 0 && magnitude != 100 && magnitude != 200 && magnitude != 300
}

/// APPLE: `isleap`, CFDate.c:220-224. `relative_year == 0` denotes Gregorian
/// 2001. Widening `year + 1` to i128 totalizes the sole signed-overflow edge.
pub fn cf_is_leap_year(relative_year: i64) -> (yes: bool)
    ensures yes == cf_leap_year_spec(relative_year as int),
{
    let shifted: i128 = relative_year as i128 + 1;
    let remainder = if shifted >= 0 {
        shifted % 400
    } else {
        -((-shifted) % 400)
    };
    let magnitude = if remainder < 0 { -remainder } else { remainder };
    magnitude % 4 == 0 && magnitude != 100 && magnitude != 200 && magnitude != 300
}

pub open spec fn days_in_month_spec(month: u8, leap: bool) -> int {
    let ordinary: int = match month {
        0 => 0,
        1 => 31,
        2 => 28,
        3 => 31,
        4 => 30,
        5 => 31,
        6 => 30,
        7 => 31,
        8 => 31,
        9 => 30,
        10 => 31,
        11 => 30,
        12 => 31,
        _ => 0,
    };
    ordinary + if month == 2 && leap { 1int } else { 0int }
}

pub open spec fn days_before_month_spec(month: u8, leap: bool) -> int {
    let ordinary: int = match month {
        0 | 1 => 0,
        2 => 31,
        3 => 59,
        4 => 90,
        5 => 120,
        6 => 151,
        7 => 181,
        8 => 212,
        9 => 243,
        10 => 273,
        11 => 304,
        12 => 334,
        13 => 365,
        _ => 0,
    };
    ordinary + if month > 2 && leap { 1int } else { 0int }
}

pub open spec fn days_after_month_spec(month: u8, leap: bool) -> int {
    let ordinary: int = match month {
        0 => 365,
        1 => 334,
        2 => 306,
        3 => 275,
        4 => 245,
        5 => 214,
        6 => 184,
        7 => 153,
        8 => 122,
        9 => 92,
        10 => 61,
        11 => 31,
        _ => 0,
    };
    ordinary + if month < 2 && leap { 1int } else { 0int }
}

/// APPLE: `daysInMonth` and `__CFDaysInMonth`, CFDate.c:216 and 226-229.
pub fn cf_days_in_month(month: u8, leap: bool) -> (days: u16)
    requires month < 16,
    ensures days as int == days_in_month_spec(month, leap),
{
    let ordinary: u16 = match month {
        0 => 0,
        1 => 31,
        2 => 28,
        3 => 31,
        4 => 30,
        5 => 31,
        6 => 30,
        7 => 31,
        8 => 31,
        9 => 30,
        10 => 31,
        11 => 30,
        12 => 31,
        _ => 0,
    };
    ordinary + if month == 2 && leap { 1 } else { 0 }
}

/// APPLE: `daysBeforeMonth` and `__CFDaysBeforeMonth`,
/// CFDate.c:217 and 231-234.
pub fn cf_days_before_month(month: u8, leap: bool) -> (days: u16)
    requires month < 16,
    ensures days as int == days_before_month_spec(month, leap),
{
    let ordinary: u16 = match month {
        0 | 1 => 0,
        2 => 31,
        3 => 59,
        4 => 90,
        5 => 120,
        6 => 151,
        7 => 181,
        8 => 212,
        9 => 243,
        10 => 273,
        11 => 304,
        12 => 334,
        13 => 365,
        _ => 0,
    };
    ordinary + if month > 2 && leap { 1 } else { 0 }
}

/// APPLE: `daysAfterMonth` and `__CFDaysAfterMonth`,
/// CFDate.c:218 and 236-239.
pub fn cf_days_after_month(month: u8, leap: bool) -> (days: u16)
    requires month < 16,
    ensures days as int == days_after_month_spec(month, leap),
{
    let ordinary: u16 = match month {
        0 => 365,
        1 => 334,
        2 => 306,
        3 => 275,
        4 => 245,
        5 => 214,
        6 => 184,
        7 => 153,
        8 => 122,
        9 => 92,
        10 => 61,
        11 => 31,
        _ => 0,
    };
    ordinary + if month < 2 && leap { 1 } else { 0 }
}

pub open spec fn days_in_relative_year_spec(relative_year: int) -> int {
    days_after_month_spec(0, cf_leap_year_spec(relative_year))
}

pub fn cf_days_in_relative_year(relative_year: i64) -> (days: u16)
    ensures
        days as int == days_in_relative_year_spec(relative_year as int),
        365 <= days <= 366,
{
    cf_days_after_month(0, cf_is_leap_year(relative_year))
}

pub open spec fn year_span_spec(start: int, end: int) -> int
    decreases if end > start { end - start } else { 0 }
{
    if end <= start {
        0
    } else {
        year_span_spec(start, end - 1) + days_in_relative_year_spec(end - 1)
    }
}

pub open spec fn trunc_div_400_spec(year: int) -> int {
    if year >= 0 { year / 400 } else { -((-year) / 400) }
}

pub open spec fn absolute_from_ymd_days_spec(
    relative_year: int,
    month: u8,
    day: u8,
) -> int {
    let blocks = trunc_div_400_spec(relative_year);
    let remainder = relative_year - blocks * 400;
    let within_cycle = if remainder < 0 {
        -year_span_spec(remainder, 0)
    } else {
        year_span_spec(0, remainder)
    };
    blocks * 146_097
        + within_cycle
        + days_before_month_spec(month, cf_leap_year_spec(remainder))
        + day as int
        - 1
}

pub const GREGORIAN_DAY_BOUND: i128 = 1_000_000_000_000_000_000_000_000_000_000;

/// APPLE: `__CFAbsoluteFromYMD`, CFDate.c:269-285. This is the same cycle
/// decomposition and year loop, but the result is exact i128 days instead of
/// a binary64 value.
pub fn cf_absolute_from_ymd_days(
    relative_year: i64,
    month: u8,
    day: u8,
) -> (absolute: i128)
    requires month < 16,
    ensures
        (absolute as int) == absolute_from_ymd_days_spec(relative_year as int, month, day),
        -(GREGORIAN_DAY_BOUND as int) < (absolute as int)
            < (GREGORIAN_DAY_BOUND as int),
{
    let blocks: i64 = relative_year / 400;
    let remainder: i64 = relative_year - blocks * 400;
    let base: i128 = (blocks as i128) * 146_097;
    proof {
        assert(-400 < remainder < 400);
        assert((GREGORIAN_DAY_BOUND as int)
            == 1_000_000_000_000_000_000_000_000_000_000int);
        assert((i64::MIN as int) <= (blocks as int) <= (i64::MAX as int));
        assert(-9_223_372_036_854_775_809int < (blocks as int)
            && (blocks as int) < 9_223_372_036_854_775_808int);
        assert((base as int) == (blocks as int) * 146_097);
        assert(-1_400_000_000_000_000_000_000_000int < (base as int));
        assert((base as int) < 1_400_000_000_000_000_000_000_000int);
        assert(1_400_000_000_000_000_000_200_000int
            < (GREGORIAN_DAY_BOUND as int)) by (compute);
        assert(-(GREGORIAN_DAY_BOUND as int) + 200_000 < (base as int));
        assert((base as int) < (GREGORIAN_DAY_BOUND as int) - 200_000);
    }
    let mut absolute = base;
    if remainder < 0 {
        let mut index = remainder;
        while index < 0
            invariant
                remainder <= index <= 0,
                (absolute as int)
                    == (base as int) - year_span_spec(remainder as int, index as int),
                (base as int) - 366 * (index as int - remainder as int) <= (absolute as int),
                (absolute as int) <= (base as int) - 365 * (index as int - remainder as int),
                -1_400_000_000_000_000_000_000_000int < (base as int)
                    < 1_400_000_000_000_000_000_000_000int,
                -(GREGORIAN_DAY_BOUND as int) < (absolute as int)
                    < (GREGORIAN_DAY_BOUND as int),
            decreases -index
        {
            let days = cf_days_in_relative_year(index);
            absolute -= days as i128;
            index += 1;
        }
    } else {
        let mut index: i64 = 0;
        while index < remainder
            invariant
                0 <= index <= remainder,
                (absolute as int)
                    == (base as int) + year_span_spec(0, index as int),
                (base as int) + 365 * (index as int) <= (absolute as int),
                (absolute as int) <= (base as int) + 366 * (index as int),
                -1_400_000_000_000_000_000_000_000int < (base as int)
                    < 1_400_000_000_000_000_000_000_000int,
                -(GREGORIAN_DAY_BOUND as int) < (absolute as int)
                    < (GREGORIAN_DAY_BOUND as int),
            decreases remainder - index
        {
            let days = cf_days_in_relative_year(index);
            absolute += days as i128;
            index += 1;
        }
    }
    let leap = cf_is_leap_year(remainder);
    let before = cf_days_before_month(month, leap);
    proof {
        assert(before <= 366);
        assert(-1_400_000_000_000_000_000_200_000int < (absolute as int));
        assert((absolute as int) < 1_400_000_000_000_000_000_200_000int);
        assert(-(GREGORIAN_DAY_BOUND as int) + 1_000 < (absolute as int));
        assert((absolute as int) < (GREGORIAN_DAY_BOUND as int) - 1_000);
    }
    absolute + (before as i128) + (day as i128) - 1
}

pub open spec fn absolute_seconds_spec(
    relative_year: int,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
) -> int {
    absolute_from_ymd_days_spec(relative_year, month, day) * 86_400
        + hour as int * 3_600
        + minute as int * 60
        + second as int
}

/// APPLE: the UTC (`tz == NULL`) arithmetic in
/// `CFGregorianDateGetAbsoluteTime`, CFDate.c:299-315, called by
/// CFPropertyList.c:1753-1759. Integer seconds avoid an f64 proof gap.
pub fn cf_absolute_time_seconds(
    relative_year: i64,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
) -> (seconds: i128)
    requires month < 16,
    ensures seconds as int
        == absolute_seconds_spec(relative_year as int, month, day, hour, minute, second),
{
    let days = cf_absolute_from_ymd_days(relative_year, month, day);
    proof {
        let exact = (days as int) * 86_400
            + (hour as int) * 3_600
            + (minute as int) * 60
            + (second as int);
        assert(-86_400_000_000_000_000_000_000_000_000_000_000int
            < (days as int) * 86_400);
        assert((days as int) * 86_400
            < 86_400_000_000_000_000_000_000_000_000_000_000int);
        assert(-86_400_000_000_000_000_000_000_000_000_000_000int < exact);
        assert(exact < 86_400_000_000_000_000_000_000_000_001_000_000int);
        assert((i128::MIN as int)
            < -86_400_000_000_000_000_000_000_000_000_000_000int) by (compute);
        assert(86_400_000_000_000_000_000_000_000_001_000_000int
            < (i128::MAX as int)) by (compute);
        assert((i128::MIN as int) < exact);
        assert(exact < (i128::MAX as int));
    }
    days * 86_400
        + (hour as i128) * 3_600
        + (minute as i128) * 60
        + second as i128
}

fn main() {
    let bytes = vec![b' ', b'\t', b'x'];
    let _cursor = skip_xml_whitespace(&bytes, 0);
}

} // verus!
