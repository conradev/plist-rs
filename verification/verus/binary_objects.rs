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

/*	CFBinaryPList.c
	Copyright (c) 2000-2014, Apple Inc. All rights reserved.
	Responsibility: Tony Parker
*/

/*
 * Modified 2026-08-22 by plist-rs contributors.
 * The modifications translate allocation-independent binary property-list
 * reader logic into executable Rust/Verus and add formal specifications,
 * contracts, invariants, and memory-safe offset arithmetic.
 */

//! Executable Verus port of the immutable, unfiltered object wire decoder in
//! `opensource-apple/CF@3cc41a76.../CFBinaryPList.c`.
//!
//! This stops at allocation-independent wire metadata. It preserves source
//! spans and numeric bits but deliberately does not construct CF objects,
//! recurse through the graph, compare dictionary keys, or normalize sets.

use vstd::prelude::*;
use vstd::wrapping::u64_specs;

verus! {

pub const HEADER_LEN: usize = 8;
pub const LONG_MAX_64: u64 = 0x7fff_ffff_ffff_ffff;
pub const UINT32_MAX_64: u64 = 0xffff_ffff;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WireContext {
    /// First byte of the offset table; object bytes occupy `[8, end)`.
    pub object_table_end: usize,
    pub object_ref_size: u8,
    pub num_objects: u64,
    pub offset_int_size: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CountInfo {
    pub count: u64,
    pub payload_start: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectHead {
    pub marker: u8,
    pub payload_start: usize,
    pub end_inclusive: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CountedSpan {
    pub count: u64,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadInt {
    pub value: u64,
    pub next: usize,
    pub payload_width: usize,
    pub folded_width: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedReference {
    pub reference: u64,
    pub offset: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerClass {
    Simple,
    Integer,
    Real,
    Date,
    Data,
    Ascii,
    Utf16,
    Uid,
    Array,
    Set,
    Dictionary,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegerInterpretation {
    Unsigned,
    Signed64,
    Unsigned128Low64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(inconsistent_fields)]
pub enum WireObject {
    Null,
    False,
    True,
    Integer {
        payload: Span,
        width: u8,
        bits: u64,
        interpretation: IntegerInterpretation,
    },
    Real32 { payload: Span, bits: u64 },
    Real64 { payload: Span, bits: u64 },
    Date { payload: Span, bits: u64 },
    Data { payload: Span, count: u64 },
    Ascii { payload: Span, count: u64 },
    Utf16Be { payload: Span, units: u64 },
    Uid { payload: Span, width: u8, value: u32 },
    Array { references: Span, count: u64 },
    Set { references: Span, count: u64 },
    Dictionary {
        keys: Span,
        values: Span,
        count: u64,
    },
}

pub open spec fn marker_class_spec(marker: u8) -> MarkerClass {
    match marker & 0xf0 {
        0x00 => MarkerClass::Simple,
        0x10 => MarkerClass::Integer,
        0x20 => MarkerClass::Real,
        0x30 => MarkerClass::Date,
        0x40 => MarkerClass::Data,
        0x50 => MarkerClass::Ascii,
        0x60 => MarkerClass::Utf16,
        0x80 => MarkerClass::Uid,
        0xa0 => MarkerClass::Array,
        0xc0 => MarkerClass::Set,
        0xd0 => MarkerClass::Dictionary,
        _ => MarkerClass::Unsupported,
    }
}

/// APPLE: marker switch at CFBinaryPList.c:1083-1085, 1098, 1128, 1168,
/// 1190, 1216, 1243, 1280, 1299-1300, and 1439.
pub fn classify_marker(marker: u8) -> (class: MarkerClass)
    ensures
        class == marker_class_spec(marker),
{
    match marker & 0xf0 {
        0x00 => MarkerClass::Simple,
        0x10 => MarkerClass::Integer,
        0x20 => MarkerClass::Real,
        0x30 => MarkerClass::Date,
        0x40 => MarkerClass::Data,
        0x50 => MarkerClass::Ascii,
        0x60 => MarkerClass::Utf16,
        0x80 => MarkerClass::Uid,
        0xa0 => MarkerClass::Array,
        0xc0 => MarkerClass::Set,
        0xd0 => MarkerClass::Dictionary,
        _ => MarkerClass::Unsupported,
    }
}

pub open spec fn sized_int_spec(bytes: Seq<u8>, start: int, len: nat) -> u64
    decreases len
{
    if len == 0 {
        0
    } else if start < 0 || start + len > bytes.len() {
        0
    } else {
        let prefix = sized_int_spec(bytes, start, (len - 1) as nat);
        let byte = bytes[start + len - 1] as u64;
        u64_specs::wrapping_add(u64_specs::wrapping_shl(prefix, 8), byte)
    }
}

/// APPLE: `_getSizedInt`, CFBinaryPList.c:733-757.
pub fn get_sized_int(data: &Vec<u8>, start: usize, val_size: u8) -> (res: u64)
    requires
        start + val_size as usize <= data.len(),
    ensures
        res == sized_int_spec(data@, start as int, val_size as nat),
{
    let mut res: u64 = 0;
    let mut idx: usize = 0;
    while idx < val_size as usize
        invariant
            idx <= val_size as usize,
            start + val_size as usize <= data.len(),
            res == sized_int_spec(data@, start as int, idx as nat),
        decreases val_size as usize - idx
    {
        let byte = data[start + idx] as u64;
        res = res.wrapping_shl(8).wrapping_add(byte);
        idx += 1;
    }
    res
}

pub open spec fn marker_power_width_spec(marker: u8) -> usize {
    match marker & 0x0f {
        0 => 1,
        1 => 2,
        2 => 4,
        3 => 8,
        4 => 16,
        5 => 32,
        6 => 64,
        7 => 128,
        8 => 256,
        9 => 512,
        10 => 1024,
        11 => 2048,
        12 => 4096,
        13 => 8192,
        14 => 16384,
        _ => 32768,
    }
}

pub fn marker_power_width(marker: u8) -> (width: usize)
    ensures
        width == marker_power_width_spec(marker),
        1 <= width <= 32768,
{
    match marker & 0x0f {
        0 => 1,
        1 => 2,
        2 => 4,
        3 => 8,
        4 => 16,
        5 => 32,
        6 => 64,
        7 => 128,
        8 => 256,
        9 => 512,
        10 => 1024,
        11 => 2048,
        12 => 4096,
        13 => 8192,
        14 => 16384,
        _ => 32768,
    }
}

pub open spec fn read_int_spec(
    data: Seq<u8>,
    start: usize,
    end_inclusive: usize,
) -> Option<ReadInt> {
    if end_inclusive >= data.len() || start > end_inclusive {
        None
    } else {
        let marker = data[start as int];
        if marker & 0xf0 != 0x10 {
            None
        } else {
            let count = marker_power_width_spec(marker);
            let ptr = start as int + 1;
            let next = ptr + count as int;
            if next > usize::MAX as int || next > end_inclusive as int + 1 {
                None
            } else {
                let folded_width = count as u8;
                Some(ReadInt {
                    value: sized_int_spec(data, ptr, folded_width as nat),
                    next: next as usize,
                    payload_width: count,
                    folded_width,
                })
            }
        }
    }
}

/// APPLE: `_readInt`, CFBinaryPList.c:855-869.
///
/// The cast from `count` to `u8` is intentional. It preserves the C call's
/// implicit narrowing into `_getSizedInt(uint8_t valSize)`.
pub fn read_int(
    data: &Vec<u8>,
    start: usize,
    end_inclusive: usize,
) -> (result: Option<ReadInt>)
    ensures
        result == read_int_spec(data@, start, end_inclusive),
{
    if end_inclusive >= data.len() || start > end_inclusive {
        return None;
    }
    let marker = data[start];
    if marker & 0xf0 != 0x10 {
        return None;
    }
    let ptr = start + 1;
    let count = marker_power_width(marker);
    if count > usize::MAX - ptr {
        return None;
    }
    let next = ptr + count;
    if next > end_inclusive + 1 {
        return None;
    }
    let folded_width = count as u8;
    let value = get_sized_int(data, ptr, folded_width);
    Some(ReadInt {
        value,
        next,
        payload_width: count,
        folded_width,
    })
}

pub open spec fn decode_count_spec(
    data: Seq<u8>,
    marker: u8,
    ptr: usize,
    end_inclusive: usize,
) -> Option<CountInfo> {
    let compact = marker & 0x0f;
    if compact != 0x0f {
        Some(CountInfo { count: compact as u64, payload_start: ptr })
    } else {
        match read_int_spec(data, ptr, end_inclusive) {
            Some(read) if read.value <= LONG_MAX_64 => Some(CountInfo {
                count: read.value,
                payload_start: read.next,
            }),
            _ => None,
        }
    }
}

/// APPLE: the compact/extended count blocks at CFBinaryPList.c:1195-1201,
/// 1221-1227, 1248-1254, 1305-1311, and 1444-1450.
pub fn decode_count(
    data: &Vec<u8>,
    marker: u8,
    ptr: usize,
    end_inclusive: usize,
) -> (result: Option<CountInfo>)
    ensures
        result == decode_count_spec(data@, marker, ptr, end_inclusive),
{
    let compact = marker & 0x0f;
    if compact != 0x0f {
        Some(CountInfo { count: compact as u64, payload_start: ptr })
    } else {
        match read_int(data, ptr, end_inclusive) {
            Some(read) if read.value <= LONG_MAX_64 => Some(CountInfo {
                count: read.value,
                payload_start: read.next,
            }),
            _ => None,
        }
    }
}

pub open spec fn checked_span_spec(start: usize, len: usize, limit: usize) -> Option<Span> {
    if start as int + len as int > usize::MAX as int
        || start > limit
        || start as int + len as int > limit as int
    {
        None
    } else {
        Some(Span { start, end: (start as int + len as int) as usize })
    }
}

/// Safe half-open form of C's repeated `check_ptr_add(ptr, cnt) - 1` extent
/// checks. A zero-length payload is the empty span at `start`.
pub fn checked_span(start: usize, len: usize, limit: usize) -> (result: Option<Span>)
    ensures
        result == checked_span_spec(start, len, limit),
{
    if len > usize::MAX - start {
        None
    } else {
        let end = start + len;
        if start > limit || end > limit {
            None
        } else {
            Some(Span { start, end })
        }
    }
}

pub open spec fn checked_product_spec(count: u64, width: usize) -> Option<usize> {
    if count as int * width as int > usize::MAX as int {
        None
    } else {
        Some((count as int * width as int) as usize)
    }
}

/// Safe exact-arithmetic form of `check_size_t_mul` on the pinned LP64 model.
pub fn checked_product(count: u64, width: usize) -> (result: Option<usize>)
    ensures
        result == checked_product_spec(count, width),
{
    proof {
        assert(count as int * width as int <= u128::MAX as int) by (nonlinear_arith);
    }
    let exact = (count as u128) * (width as u128);
    if exact > usize::MAX as u128 {
        None
    } else {
        Some(exact as usize)
    }
}

pub open spec fn ascii_span_spec(data: Seq<u8>, span: Span) -> bool {
    span.start <= span.end
        && span.end <= data.len()
}

/// CoreFoundation's compatibility mapping for the nominal ASCII decoder: an
/// input byte, including 0x80..=0xff, becomes the same-valued Unicode scalar.
pub open spec fn ascii_code_point_spec(byte: u8) -> u32 {
    byte as u32
}

pub fn ascii_code_point(byte: u8) -> (code_point: u32)
    ensures
        code_point == ascii_code_point_spec(byte),
{
    byte as u32
}

/// Allocation-independent acceptance condition of
/// `CFStringCreateWithBytes(..., kCFStringEncodingASCII, false)` in the ASCII
/// branch at CFBinaryPList.c:1216-1241.  CoreFoundation accepts every byte in
/// this path, including 0x80..=0xff, and maps it to the same Unicode value; the
/// graph bridge performs that byte-to-code-point mapping after this span check.
pub fn ascii_span(data: &Vec<u8>, span: Span) -> (valid: bool)
    ensures
        valid == ascii_span_spec(data@, span),
{
    span.start <= span.end && span.end <= data.len()
}

pub open spec fn context_base_valid_spec(data: Seq<u8>, ctx: WireContext) -> bool {
    HEADER_LEN < ctx.object_table_end
        && ctx.object_table_end <= data.len()
        && ctx.object_ref_size >= 1
        && ctx.offset_int_size >= 1
}

pub open spec fn object_head_spec(
    data: Seq<u8>,
    ctx: WireContext,
    start: usize,
) -> Option<ObjectHead> {
    if !context_base_valid_spec(data, ctx)
        || start < HEADER_LEN
        || start >= ctx.object_table_end
    {
        None
    } else {
        Some(ObjectHead {
            marker: data[start as int],
            payload_start: (start as int + 1) as usize,
            end_inclusive: (ctx.object_table_end as int - 1) as usize,
        })
    }
}

/// APPLE: object-range and marker load at CFBinaryPList.c:1075-1084.
pub fn read_object_head(
    data: &Vec<u8>,
    ctx: WireContext,
    start: usize,
) -> (result: Option<ObjectHead>)
    ensures
        result == object_head_spec(data@, ctx, start),
{
    if ctx.object_table_end <= HEADER_LEN
        || ctx.object_table_end > data.len()
        || ctx.object_ref_size < 1
        || ctx.offset_int_size < 1
        || start < HEADER_LEN
        || start >= ctx.object_table_end
    {
        return None;
    }
    Some(ObjectHead {
        marker: data[start],
        payload_start: start + 1,
        end_inclusive: ctx.object_table_end - 1,
    })
}

pub open spec fn head_usable_spec(
    data: Seq<u8>,
    ctx: WireContext,
    head: ObjectHead,
) -> bool {
    context_base_valid_spec(data, ctx)
        && HEADER_LEN < head.payload_start <= ctx.object_table_end
        && head.end_inclusive == ctx.object_table_end - 1
        && data[head.payload_start as int - 1] == head.marker
}

pub fn head_usable(
    data: &Vec<u8>,
    ctx: WireContext,
    head: ObjectHead,
) -> (usable: bool)
    ensures
        usable == head_usable_spec(data@, ctx, head),
{
    if ctx.object_table_end <= HEADER_LEN
        || ctx.object_table_end > data.len()
        || ctx.object_ref_size < 1
        || ctx.offset_int_size < 1
        || head.payload_start <= HEADER_LEN
        || head.payload_start > ctx.object_table_end
        || head.end_inclusive != ctx.object_table_end - 1
    {
        false
    } else {
        data[head.payload_start - 1] == head.marker
    }
}

pub open spec fn counted_span_spec(
    data: Seq<u8>,
    ctx: WireContext,
    head: ObjectHead,
    unit_width: usize,
) -> Option<CountedSpan> {
    match decode_count_spec(data, head.marker, head.payload_start, head.end_inclusive) {
        None => None,
        Some(count) => match checked_product_spec(count.count, unit_width) {
            None => None,
            Some(byte_len) => match checked_span_spec(
                count.payload_start,
                byte_len,
                ctx.object_table_end,
            ) {
                None => None,
                Some(span) => Some(CountedSpan { count: count.count, span }),
            },
        },
    }
}

pub fn counted_span(
    data: &Vec<u8>,
    ctx: WireContext,
    head: ObjectHead,
    unit_width: usize,
) -> (result: Option<CountedSpan>)
    ensures
        result == counted_span_spec(data@, ctx, head, unit_width),
{
    let count = match decode_count(
        data,
        head.marker,
        head.payload_start,
        head.end_inclusive,
    ) {
        Some(value) => value,
        None => return None,
    };
    let byte_len = match checked_product(count.count, unit_width) {
        Some(value) => value,
        None => return None,
    };
    let span = match checked_span(count.payload_start, byte_len, ctx.object_table_end) {
        Some(value) => value,
        None => return None,
    };
    Some(CountedSpan { count: count.count, span })
}

pub open spec fn reference_entry_span_spec(
    data: Seq<u8>,
    ctx: WireContext,
    reference: u64,
) -> Option<Span> {
    match checked_product_spec(reference, ctx.offset_int_size as usize) {
        None => None,
        Some(delta) => {
            if delta > usize::MAX - ctx.object_table_end {
                None
            } else {
                checked_span_spec(
                    (ctx.object_table_end as int + delta as int) as usize,
                    ctx.offset_int_size as usize,
                    data.len() as usize,
                )
            }
        },
    }
}

pub open spec fn resolve_reference_spec(
    data: Seq<u8>,
    ctx: WireContext,
    bytesptr: usize,
) -> Option<ResolvedReference> {
    if !context_base_valid_spec(data, ctx) || bytesptr < HEADER_LEN {
        None
    } else {
        match checked_span_spec(
            bytesptr,
            ctx.object_ref_size as usize,
            ctx.object_table_end,
        ) {
            None => None,
            Some(_) => {
                let reference = sized_int_spec(
                    data,
                    bytesptr as int,
                    ctx.object_ref_size as nat,
                );
                if reference >= ctx.num_objects {
                    None
                } else {
                    match reference_entry_span_spec(data, ctx, reference) {
                        None => None,
                        Some(entry) => Some(ResolvedReference {
                            reference,
                            offset: sized_int_spec(
                                data,
                                entry.start as int,
                                ctx.offset_int_size as nat,
                            ),
                        }),
                    }
                }
            },
        }
    }
}

/// APPLE: `_getOffsetOfRefAt`, CFBinaryPList.c:871-885.
///
/// `None` is the safe Rust spelling of the C helper's `UINT64_MAX` sentinel.
/// The two additional table-span checks are tautologies for a trailer accepted
/// by `__CFBinaryPlistGetTopLevelInfo`; they totalize malformed contexts.
pub fn resolve_reference(
    data: &Vec<u8>,
    ctx: WireContext,
    bytesptr: usize,
) -> (result: Option<ResolvedReference>)
    ensures
        result == resolve_reference_spec(data@, ctx, bytesptr),
{
    if ctx.object_table_end <= HEADER_LEN
        || ctx.object_table_end > data.len()
        || ctx.object_ref_size < 1
        || ctx.offset_int_size < 1
        || bytesptr < HEADER_LEN
    {
        return None;
    }
    if checked_span(
        bytesptr,
        ctx.object_ref_size as usize,
        ctx.object_table_end,
    ).is_none() {
        return None;
    }
    let reference = get_sized_int(data, bytesptr, ctx.object_ref_size);
    if reference >= ctx.num_objects {
        return None;
    }
    let delta = match checked_product(reference, ctx.offset_int_size as usize) {
        Some(value) => value,
        None => return None,
    };
    if delta > usize::MAX - ctx.object_table_end {
        return None;
    }
    let entry_start = ctx.object_table_end + delta;
    let entry = match checked_span(
        entry_start,
        ctx.offset_int_size as usize,
        data.len(),
    ) {
        Some(value) => value,
        None => return None,
    };
    let offset = get_sized_int(data, entry.start, ctx.offset_int_size);
    Some(ResolvedReference { reference, offset })
}

pub open spec fn simple_object_spec(marker: u8) -> Option<WireObject> {
    match marker {
        0x00 => Some(WireObject::Null),
        0x08 => Some(WireObject::False),
        0x09 => Some(WireObject::True),
        _ => None,
    }
}

/// APPLE: null/boolean branch, CFBinaryPList.c:1085-1097.
pub fn decode_simple(marker: u8) -> (result: Option<WireObject>)
    ensures
        result == simple_object_spec(marker),
{
    match marker {
        0x00 => Some(WireObject::Null),
        0x08 => Some(WireObject::False),
        0x09 => Some(WireObject::True),
        _ => None,
    }
}

pub open spec fn integer_interpretation_spec(width: usize) -> IntegerInterpretation {
    if width == 8 {
        IntegerInterpretation::Signed64
    } else if width == 16 {
        IntegerInterpretation::Unsigned128Low64
    } else {
        IntegerInterpretation::Unsigned
    }
}

pub fn integer_interpretation(width: usize) -> (result: IntegerInterpretation)
    ensures
        result == integer_interpretation_spec(width),
{
    if width == 8 {
        IntegerInterpretation::Signed64
    } else if width == 16 {
        IntegerInterpretation::Unsigned128Low64
    } else {
        IntegerInterpretation::Unsigned
    }
}

pub open spec fn integer_object_spec(
    data: Seq<u8>,
    ctx: WireContext,
    head: ObjectHead,
) -> Option<WireObject> {
    if !head_usable_spec(data, ctx, head) || head.marker & 0xf0 != 0x10 {
        None
    } else {
        let width = marker_power_width_spec(head.marker);
        if width > 16 {
            None
        } else {
            match checked_span_spec(head.payload_start, width, ctx.object_table_end) {
                None => None,
                Some(payload) => Some(WireObject::Integer {
                    payload,
                    width: width as u8,
                    bits: sized_int_spec(data, payload.start as int, width as nat),
                    interpretation: integer_interpretation_spec(width),
                }),
            }
        }
    }
}

/// APPLE: integer branch, CFBinaryPList.c:1098-1127.
pub fn decode_integer(
    data: &Vec<u8>,
    ctx: WireContext,
    head: ObjectHead,
) -> (result: Option<WireObject>)
    ensures
        result == integer_object_spec(data@, ctx, head),
{
    if !head_usable(data, ctx, head) || head.marker & 0xf0 != 0x10 {
        return None;
    }
    let width = marker_power_width(head.marker);
    if width > 16 {
        return None;
    }
    let payload = match checked_span(head.payload_start, width, ctx.object_table_end) {
        Some(value) => value,
        None => return None,
    };
    let bits = get_sized_int(data, payload.start, width as u8);
    let interpretation = integer_interpretation(width);
    Some(WireObject::Integer {
        payload,
        width: width as u8,
        bits,
        interpretation,
    })
}

pub open spec fn real_object_spec(
    data: Seq<u8>,
    ctx: WireContext,
    head: ObjectHead,
) -> Option<WireObject> {
    if !head_usable_spec(data, ctx, head) || head.marker & 0xf0 != 0x20 {
        None
    } else {
        match head.marker & 0x0f {
            2 => match checked_span_spec(head.payload_start, 4, ctx.object_table_end) {
                Some(payload) => Some(WireObject::Real32 {
                    payload,
                    bits: sized_int_spec(data, payload.start as int, 4),
                }),
                None => None,
            },
            3 => match checked_span_spec(head.payload_start, 8, ctx.object_table_end) {
                Some(payload) => Some(WireObject::Real64 {
                    payload,
                    bits: sized_int_spec(data, payload.start as int, 8),
                }),
                None => None,
            },
            _ => None,
        }
    }
}

/// APPLE: real branch, CFBinaryPList.c:1128-1167. Raw IEEE bits are returned
/// before `CFConvertFloat*SwappedToHost`/`CFNumberCreate`.
pub fn decode_real(
    data: &Vec<u8>,
    ctx: WireContext,
    head: ObjectHead,
) -> (result: Option<WireObject>)
    ensures
        result == real_object_spec(data@, ctx, head),
{
    if !head_usable(data, ctx, head) || head.marker & 0xf0 != 0x20 {
        return None;
    }
    match head.marker & 0x0f {
        2 => {
            let payload = match checked_span(head.payload_start, 4, ctx.object_table_end) {
                Some(value) => value,
                None => return None,
            };
            let bits = get_sized_int(data, payload.start, 4);
            Some(WireObject::Real32 { payload, bits })
        },
        3 => {
            let payload = match checked_span(head.payload_start, 8, ctx.object_table_end) {
                Some(value) => value,
                None => return None,
            };
            let bits = get_sized_int(data, payload.start, 8);
            Some(WireObject::Real64 { payload, bits })
        },
        _ => None,
    }
}

pub open spec fn date_object_spec(
    data: Seq<u8>,
    ctx: WireContext,
    head: ObjectHead,
) -> Option<WireObject> {
    if !head_usable_spec(data, ctx, head) || head.marker != 0x33 {
        None
    } else {
        match checked_span_spec(head.payload_start, 8, ctx.object_table_end) {
            Some(payload) => Some(WireObject::Date {
                payload,
                bits: sized_int_spec(data, payload.start as int, 8),
            }),
            None => None,
        }
    }
}

/// APPLE: date branch, CFBinaryPList.c:1168-1189. Raw IEEE-754 bits are
/// preserved before `CFConvertFloat64SwappedToHost`/`CFDateCreate`.
pub fn decode_date(
    data: &Vec<u8>,
    ctx: WireContext,
    head: ObjectHead,
) -> (result: Option<WireObject>)
    ensures
        result == date_object_spec(data@, ctx, head),
{
    if !head_usable(data, ctx, head) || head.marker != 0x33 {
        return None;
    }
    let payload = match checked_span(head.payload_start, 8, ctx.object_table_end) {
        Some(value) => value,
        None => return None,
    };
    let bits = get_sized_int(data, payload.start, 8);
    Some(WireObject::Date { payload, bits })
}

pub open spec fn data_object_spec(
    data: Seq<u8>,
    ctx: WireContext,
    head: ObjectHead,
) -> Option<WireObject> {
    if !head_usable_spec(data, ctx, head) || head.marker & 0xf0 != 0x40 {
        None
    } else {
        match counted_span_spec(data, ctx, head, 1) {
            Some(counted) => Some(WireObject::Data {
                payload: counted.span,
                count: counted.count,
            }),
            None => None,
        }
    }
}

/// APPLE: data branch, CFBinaryPList.c:1190-1215.
pub fn decode_data(
    data: &Vec<u8>,
    ctx: WireContext,
    head: ObjectHead,
) -> (result: Option<WireObject>)
    ensures
        result == data_object_spec(data@, ctx, head),
{
    if !head_usable(data, ctx, head) || head.marker & 0xf0 != 0x40 {
        return None;
    }
    match counted_span(data, ctx, head, 1) {
        Some(counted) => Some(WireObject::Data {
            payload: counted.span,
            count: counted.count,
        }),
        None => None,
    }
}

pub open spec fn ascii_object_spec(
    data: Seq<u8>,
    ctx: WireContext,
    head: ObjectHead,
) -> Option<WireObject> {
    if !head_usable_spec(data, ctx, head) || head.marker & 0xf0 != 0x50 {
        None
    } else {
        match counted_span_spec(data, ctx, head, 1) {
            Some(counted) if ascii_span_spec(data, counted.span) => Some(WireObject::Ascii {
                payload: counted.span,
                count: counted.count,
            }),
            _ => None,
        }
    }
}

/// APPLE: ASCII string branch, CFBinaryPList.c:1216-1242.
pub fn decode_ascii(
    data: &Vec<u8>,
    ctx: WireContext,
    head: ObjectHead,
) -> (result: Option<WireObject>)
    ensures
        result == ascii_object_spec(data@, ctx, head),
{
    if !head_usable(data, ctx, head) || head.marker & 0xf0 != 0x50 {
        return None;
    }
    let counted = match counted_span(data, ctx, head, 1) {
        Some(value) => value,
        None => return None,
    };
    if !ascii_span(data, counted.span) {
        return None;
    }
    Some(WireObject::Ascii {
        payload: counted.span,
        count: counted.count,
    })
}

pub open spec fn utf16_object_spec(
    data: Seq<u8>,
    ctx: WireContext,
    head: ObjectHead,
) -> Option<WireObject> {
    if !head_usable_spec(data, ctx, head) || head.marker & 0xf0 != 0x60 {
        None
    } else {
        match counted_span_spec(data, ctx, head, 2) {
            Some(counted) => Some(WireObject::Utf16Be {
                payload: counted.span,
                units: counted.count,
            }),
            None => None,
        }
    }
}

/// APPLE: UTF-16BE string branch through the byte-span and swap loop,
/// CFBinaryPList.c:1243-1279. The span retains the original big-endian units.
pub fn decode_utf16(
    data: &Vec<u8>,
    ctx: WireContext,
    head: ObjectHead,
) -> (result: Option<WireObject>)
    ensures
        result == utf16_object_spec(data@, ctx, head),
{
    if !head_usable(data, ctx, head) || head.marker & 0xf0 != 0x60 {
        return None;
    }
    match counted_span(data, ctx, head, 2) {
        Some(counted) => Some(WireObject::Utf16Be {
            payload: counted.span,
            units: counted.count,
        }),
        None => None,
    }
}

pub open spec fn uid_width_spec(marker: u8) -> u8 {
    match marker & 0x0f {
        0 => 1,
        1 => 2,
        2 => 3,
        3 => 4,
        4 => 5,
        5 => 6,
        6 => 7,
        7 => 8,
        8 => 9,
        9 => 10,
        10 => 11,
        11 => 12,
        12 => 13,
        13 => 14,
        14 => 15,
        _ => 16,
    }
}

pub fn uid_width(marker: u8) -> (width: u8)
    ensures
        width == uid_width_spec(marker),
        1 <= width <= 16,
{
    match marker & 0x0f {
        0 => 1,
        1 => 2,
        2 => 3,
        3 => 4,
        4 => 5,
        5 => 6,
        6 => 7,
        7 => 8,
        8 => 9,
        9 => 10,
        10 => 11,
        11 => 12,
        12 => 13,
        13 => 14,
        14 => 15,
        _ => 16,
    }
}

pub open spec fn uid_object_spec(
    data: Seq<u8>,
    ctx: WireContext,
    head: ObjectHead,
) -> Option<WireObject> {
    if !head_usable_spec(data, ctx, head) || head.marker & 0xf0 != 0x80 {
        None
    } else {
        let width = uid_width_spec(head.marker);
        match checked_span_spec(head.payload_start, width as usize, ctx.object_table_end) {
            None => None,
            Some(payload) => {
                let value = sized_int_spec(data, payload.start as int, width as nat);
                if value > UINT32_MAX_64 {
                    None
                } else {
                    Some(WireObject::Uid {
                        payload,
                        width,
                        value: value as u32,
                    })
                }
            },
        }
    }
}

/// APPLE: keyed-archiver UID branch, CFBinaryPList.c:1280-1298.
pub fn decode_uid(
    data: &Vec<u8>,
    ctx: WireContext,
    head: ObjectHead,
) -> (result: Option<WireObject>)
    ensures
        result == uid_object_spec(data@, ctx, head),
{
    if !head_usable(data, ctx, head) || head.marker & 0xf0 != 0x80 {
        return None;
    }
    let width = uid_width(head.marker);
    let payload = match checked_span(head.payload_start, width as usize, ctx.object_table_end) {
        Some(value) => value,
        None => return None,
    };
    let value = get_sized_int(data, payload.start, width);
    if value > UINT32_MAX_64 {
        return None;
    }
    Some(WireObject::Uid {
        payload,
        width,
        value: value as u32,
    })
}

pub open spec fn sequence_object_spec(
    data: Seq<u8>,
    ctx: WireContext,
    head: ObjectHead,
    is_set: bool,
) -> Option<WireObject> {
    let expected_class: u8 = if is_set { 0xc0 } else { 0xa0 };
    if !head_usable_spec(data, ctx, head) || head.marker & 0xf0 != expected_class {
        None
    } else {
        match counted_span_spec(data, ctx, head, ctx.object_ref_size as usize) {
            None => None,
            Some(counted) => match checked_product_spec(counted.count, 8) {
                None => None,
                Some(_) => if is_set {
                    Some(WireObject::Set {
                        references: counted.span,
                        count: counted.count,
                    })
                } else {
                    Some(WireObject::Array {
                        references: counted.span,
                        count: counted.count,
                    })
                },
            },
        }
    }
}

/// APPLE: array/set wire sizing before recursive construction,
/// CFBinaryPList.c:1299-1321. The eight-byte allocation product models
/// `sizeof(CFPropertyListRef)` on the pinned LP64 target.
pub fn decode_sequence(
    data: &Vec<u8>,
    ctx: WireContext,
    head: ObjectHead,
    is_set: bool,
) -> (result: Option<WireObject>)
    ensures
        result == sequence_object_spec(data@, ctx, head, is_set),
{
    let expected_class: u8 = if is_set { 0xc0 } else { 0xa0 };
    if !head_usable(data, ctx, head) || head.marker & 0xf0 != expected_class {
        return None;
    }
    let counted = match counted_span(data, ctx, head, ctx.object_ref_size as usize) {
        Some(value) => value,
        None => return None,
    };
    if checked_product(counted.count, 8).is_none() {
        return None;
    }
    if is_set {
        Some(WireObject::Set {
            references: counted.span,
            count: counted.count,
        })
    } else {
        Some(WireObject::Array {
            references: counted.span,
            count: counted.count,
        })
    }
}

pub open spec fn dictionary_object_spec(
    data: Seq<u8>,
    ctx: WireContext,
    head: ObjectHead,
) -> Option<WireObject> {
    if !head_usable_spec(data, ctx, head) || head.marker & 0xf0 != 0xd0 {
        None
    } else {
        match decode_count_spec(data, head.marker, head.payload_start, head.end_inclusive) {
            None => None,
            Some(count) => match checked_product_spec(count.count, 2) {
                None => None,
                Some(reference_count) => match checked_product_spec(
                    reference_count as u64,
                    ctx.object_ref_size as usize,
                ) {
                    None => None,
                    Some(reference_bytes) => match checked_span_spec(
                        count.payload_start,
                        reference_bytes,
                        ctx.object_table_end,
                    ) {
                        None => None,
                        Some(references) => match checked_product_spec(reference_count as u64, 8) {
                            None => None,
                            Some(_) => match checked_product_spec(
                                count.count,
                                ctx.object_ref_size as usize,
                            ) {
                                None => None,
                                Some(key_bytes) => match checked_span_spec(
                                    references.start,
                                    key_bytes,
                                    references.end,
                                ) {
                                    None => None,
                                    Some(keys) => Some(WireObject::Dictionary {
                                        keys,
                                        values: Span {
                                            start: keys.end,
                                            end: references.end,
                                        },
                                        count: count.count,
                                    }),
                                },
                            },
                        },
                    },
                },
            },
        }
    }
}

/// APPLE: dictionary wire sizing before recursive key/value construction,
/// CFBinaryPList.c:1439-1462. Keys occupy the first `count` references and
/// values occupy the second `count` references.
pub fn decode_dictionary(
    data: &Vec<u8>,
    ctx: WireContext,
    head: ObjectHead,
) -> (result: Option<WireObject>)
    ensures
        result == dictionary_object_spec(data@, ctx, head),
{
    if !head_usable(data, ctx, head) || head.marker & 0xf0 != 0xd0 {
        return None;
    }
    let count = match decode_count(
        data,
        head.marker,
        head.payload_start,
        head.end_inclusive,
    ) {
        Some(value) => value,
        None => return None,
    };
    let reference_count = match checked_product(count.count, 2) {
        Some(value) => value,
        None => return None,
    };
    let reference_bytes = match checked_product(
        reference_count as u64,
        ctx.object_ref_size as usize,
    ) {
        Some(value) => value,
        None => return None,
    };
    let references = match checked_span(
        count.payload_start,
        reference_bytes,
        ctx.object_table_end,
    ) {
        Some(value) => value,
        None => return None,
    };
    if checked_product(reference_count as u64, 8).is_none() {
        return None;
    }
    let key_bytes = match checked_product(count.count, ctx.object_ref_size as usize) {
        Some(value) => value,
        None => return None,
    };
    let keys = match checked_span(references.start, key_bytes, references.end) {
        Some(value) => value,
        None => return None,
    };
    Some(WireObject::Dictionary {
        keys,
        values: Span { start: keys.end, end: references.end },
        count: count.count,
    })
}

pub open spec fn wire_object_spec(
    data: Seq<u8>,
    ctx: WireContext,
    start: usize,
) -> Option<WireObject> {
    match object_head_spec(data, ctx, start) {
        None => None,
        Some(head) => match marker_class_spec(head.marker) {
            MarkerClass::Simple => simple_object_spec(head.marker),
            MarkerClass::Integer => integer_object_spec(data, ctx, head),
            MarkerClass::Real => real_object_spec(data, ctx, head),
            MarkerClass::Date => date_object_spec(data, ctx, head),
            MarkerClass::Data => data_object_spec(data, ctx, head),
            MarkerClass::Ascii => ascii_object_spec(data, ctx, head),
            MarkerClass::Utf16 => utf16_object_spec(data, ctx, head),
            MarkerClass::Uid => uid_object_spec(data, ctx, head),
            MarkerClass::Array => sequence_object_spec(data, ctx, head, false),
            MarkerClass::Set => sequence_object_spec(data, ctx, head, true),
            MarkerClass::Dictionary => dictionary_object_spec(data, ctx, head),
            MarkerClass::Unsupported => None,
        },
    }
}

/// APPLE: allocation-independent immutable/unfiltered dispatch of
/// `__CFBinaryPlistCreateObjectFiltered`, CFBinaryPList.c:1061-1559.
pub fn decode_wire_object(
    data: &Vec<u8>,
    ctx: WireContext,
    start: usize,
) -> (result: Option<WireObject>)
    ensures
        result == wire_object_spec(data@, ctx, start),
{
    let head = match read_object_head(data, ctx, start) {
        Some(value) => value,
        None => return None,
    };
    match classify_marker(head.marker) {
        MarkerClass::Simple => decode_simple(head.marker),
        MarkerClass::Integer => decode_integer(data, ctx, head),
        MarkerClass::Real => decode_real(data, ctx, head),
        MarkerClass::Date => decode_date(data, ctx, head),
        MarkerClass::Data => decode_data(data, ctx, head),
        MarkerClass::Ascii => decode_ascii(data, ctx, head),
        MarkerClass::Utf16 => decode_utf16(data, ctx, head),
        MarkerClass::Uid => decode_uid(data, ctx, head),
        MarkerClass::Array => decode_sequence(data, ctx, head, false),
        MarkerClass::Set => decode_sequence(data, ctx, head, true),
        MarkerClass::Dictionary => decode_dictionary(data, ctx, head),
        MarkerClass::Unsupported => None,
    }
}

fn main() {
    let bytes = vec![
        0x62u8, 0x70u8, 0x6cu8, 0x69u8, 0x73u8, 0x74u8, 0x30u8, 0x30u8,
        0x09u8,
    ];
    let ctx = WireContext {
        object_table_end: 9,
        object_ref_size: 1,
        num_objects: 1,
        offset_int_size: 1,
    };
    let _object = decode_wire_object(&bytes, ctx, 8);
}

}
