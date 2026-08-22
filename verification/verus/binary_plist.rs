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
 * Modified 2026-08-22 by the plist-rs contributors: translated the
 * pointer-free reader core to executable Rust and added Verus contracts and
 * proofs. See NOTICE and PORT_MAP.md for the complete modification record.
 */

//! Executable Verus port of the pointer-free binary-plist parsing core in
//! `opensource-apple/CF@3cc41a76.../CFBinaryPList.c`.
//!
//! The `APPLE:` comments map each implementation to the pinned C function and
//! line range. This file deliberately remains under APSL-2.0 because it is a
//! direct, structure-preserving port rather than a clean-room implementation.

use vstd::prelude::*;
use vstd::arithmetic::mul::{
    lemma_mul_inequality,
    lemma_mul_is_distributive_add_other_way,
};
use vstd::wrapping::u64_specs;

verus! {

pub const TRAILER_LEN: usize = 32;
pub const HEADER_LEN: usize = 8;
pub const LONG_MAX_64: u64 = 0x7fff_ffff_ffff_ffff;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trailer {
    pub offset_int_size: u8,
    pub object_ref_size: u8,
    pub num_objects: u64,
    pub top_object: u64,
    pub offset_table_offset: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrailerLayout {
    pub offset_table_size: u64,
    pub object_data_size: u64,
    pub total_size: u64,
    pub max_object_offset: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadInt {
    pub value: u64,
    pub next: usize,
    pub payload_width: usize,
    pub folded_width: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopLevelInfo {
    pub marker: u8,
    pub offset: u64,
    pub trailer: Trailer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeCheck {
    Valid { end: usize },
    Overflow,
    Outside,
}

/// Mathematical recurrence for `_getSizedInt`, including its unsigned
/// 64-bit wraparound for inputs wider than eight bytes.
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
///
/// This keeps the C loop's operation order: shift the previous unsigned
/// 64-bit result eight places, then add the next byte.
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

/// APPLE: the three `CFSwapInt64BigToHost` operations in
/// `__CFBinaryPlistGetTopLevelInfo`, CFBinaryPList.c:771-776.
pub fn extract_trailer(data: &Vec<u8>) -> (trail: Trailer)
    requires
        data.len() >= TRAILER_LEN,
    ensures
        trail == trailer_spec(data@),
        trail.offset_int_size == data@[data@.len() - 26],
        trail.object_ref_size == data@[data@.len() - 25],
        trail.num_objects == sized_int_spec(data@, data@.len() - 24, 8),
        trail.top_object == sized_int_spec(data@, data@.len() - 16, 8),
        trail.offset_table_offset == sized_int_spec(data@, data@.len() - 8, 8),
{
    let start = data.len() - TRAILER_LEN;
    Trailer {
        offset_int_size: data[start + 6],
        object_ref_size: data[start + 7],
        num_objects: get_sized_int(data, start + 8, 8),
        top_object: get_sized_int(data, start + 16, 8),
        offset_table_offset: get_sized_int(data, start + 24, 8),
    }
}

pub open spec fn trailer_spec(data: Seq<u8>) -> Trailer
    recommends
        data.len() >= TRAILER_LEN,
{
    Trailer {
        offset_int_size: data[data.len() - 26],
        object_ref_size: data[data.len() - 25],
        num_objects: sized_int_spec(data, data.len() - 24, 8),
        top_object: sized_int_spec(data, data.len() - 16, 8),
        offset_table_offset: sized_int_spec(data, data.len() - 8, 8),
    }
}

/// Exact capacity predicate used at CFBinaryPList.c:818-822. Notice that the
/// historical C compares capacity to `_numObjects`, not `_numObjects - 1`.
pub open spec fn width_can_address_spec(width: u8, inclusive_max: u64) -> bool {
    match width {
        0 => inclusive_max < 0x0000_0000_0000_0001,
        1 => inclusive_max < 0x0000_0000_0000_0100,
        2 => inclusive_max < 0x0000_0000_0001_0000,
        3 => inclusive_max < 0x0000_0000_0100_0000,
        4 => inclusive_max < 0x0000_0001_0000_0000,
        5 => inclusive_max < 0x0000_0100_0000_0000,
        6 => inclusive_max < 0x0001_0000_0000_0000,
        7 => inclusive_max < 0x0100_0000_0000_0000,
        _ => true,
    }
}

/// APPLE: CFBinaryPList.c:818-822.
pub fn width_can_address(width: u8, inclusive_max: u64) -> (ok: bool)
    ensures
        ok == width_can_address_spec(width, inclusive_max),
{
    if width >= 8 {
        true
    } else {
        // Expanded from C's `1ULL << (8 * width)`. Keeping the finite table
        // avoids making the proof depend on Verus's target-width shift model.
        let capacity = match width {
            0 => 0x0000_0000_0000_0001u64,
            1 => 0x0000_0000_0000_0100u64,
            2 => 0x0000_0000_0001_0000u64,
            3 => 0x0000_0000_0100_0000u64,
            4 => 0x0000_0001_0000_0000u64,
            5 => 0x0000_0100_0000_0000u64,
            6 => 0x0001_0000_0000_0000u64,
            _ => 0x0100_0000_0000_0000u64,
        };
        capacity > inclusive_max
    }
}

/// Safe exact-arithmetic form of C's checked unsigned multiplication at
/// CFBinaryPList.c:70-75. The parser only observes the no-overflow arm.
pub fn checked_mul_u64(x: u64, y: u64) -> (result: Option<u64>)
    ensures
        match result {
            Some(value) => value as int == x as int * y as int,
            None => x as int * y as int > u64::MAX as int,
        },
        result.is_some() <==> x as int * y as int <= u64::MAX as int,
{
    proof {
        assert(x as int * y as int <= u128::MAX as int) by (nonlinear_arith);
    }
    let exact = (x as u128) * (y as u128);
    if exact <= u64::MAX as u128 {
        Some(exact as u64)
    } else {
        None
    }
}

/// Safe exact-arithmetic form of C's checked unsigned addition at
/// CFBinaryPList.c:64-68. The parser only observes the no-overflow arm.
pub fn checked_add_u64(x: u64, y: u64) -> (result: Option<u64>)
    ensures
        match result {
            Some(value) => value as int == x as int + y as int,
            None => x as int + y as int > u64::MAX as int,
        },
        result.is_some() <==> x as int + y as int <= u64::MAX as int,
{
    proof {
        assert(x as int + y as int <= u128::MAX as int) by (nonlinear_arith);
    }
    let exact = (x as u128) + (y as u128);
    if exact <= u64::MAX as u128 {
        Some(exact as u64)
    } else {
        None
    }
}

/// Scalar acceptance predicate for CFBinaryPList.c:778-823 on the pinned
/// LP64 model. Header, pointer provenance, and offset-table bytes are handled
/// by separate routines.
pub open spec fn trailer_fields_valid_spec(datalen: u64, trail: Trailer) -> bool {
    datalen >= (TRAILER_LEN + HEADER_LEN + 1) as u64
        && trail.num_objects <= LONG_MAX_64
        && trail.offset_table_offset <= LONG_MAX_64
        && trail.num_objects >= 1
        && trail.top_object < trail.num_objects
        && trail.offset_table_offset >= 9
        && trail.offset_table_offset < datalen - TRAILER_LEN as u64
        && trail.offset_int_size >= 1
        && trail.object_ref_size >= 1
        && trail.num_objects as int * trail.offset_int_size as int <= u64::MAX as int
        && trail.offset_table_offset as int
            + trail.num_objects as int * trail.offset_int_size as int
            + TRAILER_LEN as int <= u64::MAX as int
        && datalen as int == trail.offset_table_offset as int
            + trail.num_objects as int * trail.offset_int_size as int
            + TRAILER_LEN as int
        && width_can_address_spec(trail.object_ref_size, trail.num_objects)
        && width_can_address_spec(trail.offset_int_size, trail.offset_table_offset)
}

/// APPLE: pointer-free/scalar body of `__CFBinaryPlistGetTopLevelInfo`,
/// CFBinaryPList.c:778-823.
pub fn validate_trailer_fields(datalen: u64, trail: Trailer) -> (result: Option<TrailerLayout>)
    ensures
        result.is_some() == trailer_fields_valid_spec(datalen, trail),
        match result {
            Some(layout) => {
                &&& layout.offset_table_size as int
                    == trail.num_objects as int * trail.offset_int_size as int
                &&& layout.object_data_size == trail.offset_table_offset - HEADER_LEN as u64
                &&& layout.total_size == datalen
                &&& layout.total_size as int
                    == trail.offset_table_offset as int
                        + trail.num_objects as int * trail.offset_int_size as int
                        + TRAILER_LEN as int
                &&& layout.max_object_offset == trail.offset_table_offset - 1
            },
            None => true,
        },
{
    if datalen < (TRAILER_LEN + HEADER_LEN + 1) as u64 {
        return None;
    }
    if trail.num_objects > LONG_MAX_64 || trail.offset_table_offset > LONG_MAX_64 {
        return None;
    }
    if trail.num_objects < 1 || trail.top_object >= trail.num_objects {
        return None;
    }
    if trail.offset_table_offset < 9 {
        return None;
    }
    if datalen - TRAILER_LEN as u64 <= trail.offset_table_offset {
        return None;
    }
    if trail.offset_int_size < 1 || trail.object_ref_size < 1 {
        return None;
    }

    let offset_table_size = match checked_mul_u64(
        trail.num_objects,
        trail.offset_int_size as u64,
    ) {
        Some(value) => value,
        None => return None,
    };
    if offset_table_size < 1 {
        return None;
    }

    let object_data_size = trail.offset_table_offset - HEADER_LEN as u64;
    let object_end = match checked_add_u64(HEADER_LEN as u64, object_data_size) {
        Some(value) => value,
        None => return None,
    };
    let table_end = match checked_add_u64(object_end, offset_table_size) {
        Some(value) => value,
        None => return None,
    };
    let total_size = match checked_add_u64(table_end, TRAILER_LEN as u64) {
        Some(value) => value,
        None => return None,
    };
    if datalen != total_size {
        return None;
    }
    if !width_can_address(trail.object_ref_size, trail.num_objects) {
        return None;
    }
    if !width_can_address(trail.offset_int_size, trail.offset_table_offset) {
        return None;
    }

    Some(TrailerLayout {
        offset_table_size,
        object_data_size,
        total_size,
        max_object_offset: trail.offset_table_offset - 1,
    })
}

pub open spec fn header_matches_spec(data: Seq<u8>) -> bool {
    data.len() >= 7
        && data[0] == 0x62
        && data[1] == 0x70
        && data[2] == 0x6c
        && data[3] == 0x69
        && data[4] == 0x73
        && data[5] == 0x74
        && data[6] == 0x30
}

/// APPLE: the deliberately version-permissive `memcmp("bplist0", ..., 7)`
/// at CFBinaryPList.c:764-770. Byte 7 is intentionally not inspected.
pub fn header_matches(data: &Vec<u8>) -> (matches: bool)
    ensures
        matches == header_matches_spec(data@),
{
    if data.len() < 7 {
        false
    } else {
        data[0] == 0x62
            && data[1] == 0x70
            && data[2] == 0x6c
            && data[3] == 0x69
            && data[4] == 0x73
            && data[5] == 0x74
            && data[6] == 0x30
    }
}

pub open spec fn all_offsets_bounded_spec(data: Seq<u8>, trail: Trailer) -> bool
    recommends
        trail.offset_table_offset as int
            + trail.num_objects as int * trail.offset_int_size as int <= data.len(),
{
    forall|idx: int| 0 <= idx < trail.num_objects as int ==>
        #[trigger] sized_int_spec(
            data,
            trail.offset_table_offset as int + idx * trail.offset_int_size as int,
            trail.offset_int_size as nat,
        )
            < trail.offset_table_offset
}

pub open spec fn top_level_valid_spec(data: Seq<u8>) -> bool {
    data.len() >= TRAILER_LEN + HEADER_LEN + 1
        && data.len() <= u64::MAX
        && header_matches_spec(data)
        && trailer_fields_valid_spec(data.len() as u64, trailer_spec(data))
        && trailer_spec(data).num_objects <= usize::MAX as u64
        && trailer_spec(data).offset_table_offset <= usize::MAX as u64
        && all_offsets_bounded_spec(data, trailer_spec(data))
        && {
            let trail = trailer_spec(data);
            let root_entry = trail.offset_table_offset as int
                + trail.top_object as int * trail.offset_int_size as int;
            let root = sized_int_spec(data, root_entry, trail.offset_int_size as nat);
            8 <= root < trail.offset_table_offset
        }
}

/// APPLE: `__CFBinaryPlistGetTopLevelInfo`, CFBinaryPList.c:759-847, under
/// the pinned LP64 target. This is a pointer-free port: indices into `Vec`
/// replace checked pointer arithmetic, while every scalar rejection and the
/// complete offset-table scan remain in C order.
pub fn get_top_level_info(data: &Vec<u8>) -> (result: Option<TopLevelInfo>)
    requires
        data.len() <= u64::MAX,
    ensures
        result.is_some() == top_level_valid_spec(data@),
        match result {
            Some(info) => {
                &&& info.trailer == trailer_spec(data@)
                &&& info.offset == sized_int_spec(
                    data@,
                    info.trailer.offset_table_offset as int
                        + info.trailer.top_object as int
                            * info.trailer.offset_int_size as int,
                    info.trailer.offset_int_size as nat,
                )
                &&& info.marker == data@[info.offset as int]
                &&& 8 <= info.offset < info.trailer.offset_table_offset
            },
            None => true,
        },
{
    if data.len() < TRAILER_LEN + HEADER_LEN + 1 {
        return None;
    }
    if !header_matches(data) {
        return None;
    }
    let trail = extract_trailer(data);
    let layout = match validate_trailer_fields(data.len() as u64, trail) {
        Some(value) => value,
        None => return None,
    };
    if trail.num_objects > usize::MAX as u64
        || trail.offset_table_offset > usize::MAX as u64
    {
        return None;
    }

    let table_start_u64 = trail.offset_table_offset;
    let count_u64 = trail.num_objects;
    let table_start = table_start_u64 as usize;
    let width = trail.offset_int_size as usize;
    let count = count_u64 as usize;
    proof {
        assert(table_start_u64 <= usize::MAX as u64);
        assert(count_u64 <= usize::MAX as u64);
        assert(table_start as u64 == table_start_u64);
        assert(count as u64 == count_u64);
        assert(table_start_u64 == trail.offset_table_offset);
        assert(count_u64 == trail.num_objects);
        assert(data.len() as int
            == trail.offset_table_offset as int
                + trail.num_objects as int * trail.offset_int_size as int
                + TRAILER_LEN as int);
        assert(trail.offset_table_offset as int
            + trail.num_objects as int * trail.offset_int_size as int <= data.len());
    }
    let mut idx: usize = 0;
    let mut bytesptr = table_start;
    while idx < count
        invariant
            data.len() <= u64::MAX,
            trail == trailer_spec(data@),
            trailer_fields_valid_spec(data.len() as u64, trail),
            layout.offset_table_size as int
                == trail.num_objects as int * trail.offset_int_size as int,
            layout.total_size == data.len() as u64,
            data.len() as int
                == trail.offset_table_offset as int
                    + trail.num_objects as int * trail.offset_int_size as int
                    + TRAILER_LEN as int,
            trail.offset_table_offset as int
                + trail.num_objects as int * trail.offset_int_size as int <= data.len(),
            idx <= count,
            count == trail.num_objects as usize,
            count as int == trail.num_objects as int,
            width == trail.offset_int_size as usize,
            width as int == trail.offset_int_size as int,
            1 <= width,
            table_start as int == trail.offset_table_offset as int,
            bytesptr as int == trail.offset_table_offset as int
                + idx as int * trail.offset_int_size as int,
            forall|prior: int| 0 <= prior < idx as int ==>
                #[trigger] sized_int_spec(
                    data@,
                    trail.offset_table_offset as int
                        + prior * trail.offset_int_size as int,
                    trail.offset_int_size as nat,
                )
                    < trail.offset_table_offset
            ,
        decreases count - idx
    {
        proof {
            assert(idx as int + 1 <= count as int);
            lemma_mul_inequality(
                idx as int + 1,
                count as int,
                width as int,
            );
            lemma_mul_is_distributive_add_other_way(
                width as int,
                idx as int,
                1,
            );
            assert(idx as int * width as int + width as int
                <= count as int * width as int);
            assert(bytesptr as int
                == trail.offset_table_offset as int + idx as int * width as int);
            assert(bytesptr as int + width as int
                == trail.offset_table_offset as int
                    + idx as int * width as int + width as int);
            assert(bytesptr as int + width as int
                <= trail.offset_table_offset as int
                    + count as int * width as int);
            assert(bytesptr as int + width as int <= data.len());
            assert(bytesptr + width <= data.len());
        }
        let old_idx = idx;
        let old_bytesptr = bytesptr;
        let off = get_sized_int(data, bytesptr, trail.offset_int_size);
        if off >= trail.offset_table_offset {
            proof {
                assert(!all_offsets_bounded_spec(data@, trail));
            }
            return None;
        }
        bytesptr += width;
        idx += 1;
        proof {
            assert(bytesptr as int == old_bytesptr as int + width as int);
            assert(idx as int == old_idx as int + 1);
            lemma_mul_is_distributive_add_other_way(
                trail.offset_int_size as int,
                old_idx as int,
                1,
            );
            assert(bytesptr as int == trail.offset_table_offset as int
                + idx as int * trail.offset_int_size as int);
        }
    }
    proof {
        assert(all_offsets_bounded_spec(data@, trail));
    }

    let top_u64 = trail.top_object;
    let top = top_u64 as usize;
    proof {
        assert(top_u64 == trail.top_object);
        assert(top_u64 < count_u64);
        assert(top_u64 <= usize::MAX as u64);
        assert(top as u64 == top_u64);
        assert(top_u64 == trail.top_object);
        assert(top as int == trail.top_object as int);
        assert(top < count);
        lemma_mul_inequality(top as int, count as int, width as int);
        assert(top as int * width as int <= count as int * width as int)
            ;
        assert(top as int * width as int <= usize::MAX as int);
    }
    let root_delta = top * width;
    proof {
        assert(root_delta as int == top as int * width as int);
        assert(table_start as int == trail.offset_table_offset as int);
        assert(table_start as int + root_delta as int
            == trail.offset_table_offset as int + top as int * width as int);
        assert(table_start as int + root_delta as int
            <= trail.offset_table_offset as int
                + count as int * width as int);
        assert(trail.offset_table_offset as int
            + count as int * width as int <= data.len());
        assert(data.len() <= usize::MAX);
        assert(table_start as int + root_delta as int <= usize::MAX as int);
    }
    let root_entry = table_start + root_delta;
    proof {
        assert(root_entry as int == table_start as int + root_delta as int);
        assert(top as int + 1 <= count as int);
        lemma_mul_inequality(top as int + 1, count as int, width as int);
        lemma_mul_is_distributive_add_other_way(width as int, top as int, 1);
        assert(top as int * width as int + width as int
            <= count as int * width as int);
        assert(root_entry as int + width as int
            == trail.offset_table_offset as int
                + top as int * width as int + width as int);
        assert(root_entry as int + width as int
            <= trail.offset_table_offset as int
                + count as int * width as int);
        assert(root_entry as int + width as int <= data.len());
        assert(root_entry + width <= data.len());
    }
    let off = get_sized_int(data, root_entry, trail.offset_int_size);
    if off < HEADER_LEN as u64 || trail.offset_table_offset <= off {
        return None;
    }
    let marker = data[off as usize];
    Some(TopLevelInfo { marker, offset: off, trailer: trail })
}

/// Exact safe-range classification for every object-table payload access.
pub open spec fn checked_range_spec(start: usize, len: usize, limit: usize) -> RangeCheck {
    if start as int + len as int > usize::MAX as int {
        RangeCheck::Overflow
    } else if start > limit || start as int + len as int > limit as int {
        RangeCheck::Outside
    } else {
        RangeCheck::Valid { end: (start + len) as usize }
    }
}

/// APPLE: safe integer form of the repeated `check_ptr_add` + extent checks,
/// e.g. CFBinaryPList.c:861-863 and 1102-1108.
pub fn checked_object_range(start: usize, len: usize, limit: usize) -> (result: RangeCheck)
    ensures
        result == checked_range_spec(start, len, limit),
{
    if len > usize::MAX - start {
        RangeCheck::Overflow
    } else {
        let end = start + len;
        if start > limit || end > limit {
            RangeCheck::Outside
        } else {
            RangeCheck::Valid { end }
        }
    }
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

/// Count encoded by the low marker nibble. APPLE: `_readInt`,
/// CFBinaryPList.c:855-869. A nibble is at most 15, so the C shift is defined.
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

/// Acceptance predicate for the pointer-free `_readInt` port.
pub open spec fn read_int_valid_spec(data: Seq<u8>, start: usize, end_inclusive: usize) -> bool {
    start <= end_inclusive
        && end_inclusive < data.len()
        && (data[start as int] & 0xf0) == 0x10
        && start as int + 1 + marker_power_width_spec(data[start as int]) as int
            <= end_inclusive as int + 1
}

/// APPLE: `_readInt`, CFBinaryPList.c:855-869.
///
/// `folded_width` intentionally records C's implicit `uint64_t` to `uint8_t`
/// conversion at the call to `_getSizedInt`. Thus marker widths 256 and above
/// advance over the full payload but fold zero bytes, exactly as the pinned C
/// translation unit does.
pub fn read_int(data: &Vec<u8>, start: usize, end_inclusive: usize) -> (result: Option<ReadInt>)
    requires
        end_inclusive < data.len(),
    ensures
        result.is_some() == read_int_valid_spec(data@, start, end_inclusive),
        match result {
            Some(read) => {
                &&& read.payload_width == marker_power_width_spec(data@[start as int])
                &&& read.folded_width == read.payload_width as u8
                &&& read.next == start + 1 + read.payload_width
                &&& read.value == sized_int_spec(
                    data@,
                    start as int + 1,
                    read.folded_width as nat,
                )
            },
            None => true,
        },
{
    if end_inclusive < start {
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

fn main() {
    let bytes = vec![0x01u8, 0x23u8, 0x45u8, 0x67u8];
    let _value = get_sized_int(&bytes, 0, 4);

    // Executable singleton fixture: `bplist00`, one `true` object at offset
    // eight, one offset-table byte, and the canonical 32-byte trailer.
    let plist = vec![
        0x62u8, 0x70u8, 0x6cu8, 0x69u8, 0x73u8, 0x74u8, 0x30u8, 0x30u8,
        0x09u8,
        0x08u8,
        0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x01u8, 0x01u8,
        0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x01u8,
        0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
        0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x09u8,
    ];
    let _info = get_top_level_info(&plist);
}

}
