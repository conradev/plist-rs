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

/* CFBinaryPList.c
 * Copyright (c) 2000-2014, Apple Inc. All rights reserved.
 * Responsibility: Tony Parker
 */

/*
 * Modified 2026-08-22 by the plist-rs contributors: translated the
 * allocation-free `_getSizedInt` loop into source-faithful Rust and added
 * C-ABI adapters for cross-language symbolic verification.
 */

#![no_std]

#[inline]
fn cf_port_get_sized_int(data: &[u8], val_size: u8) -> u64 {
    let mut res = 0_u64;
    let mut idx = 0_usize;
    while idx < usize::from(val_size) {
        res = res.wrapping_shl(8).wrapping_add(u64::from(data[idx]));
        idx += 1;
    }
    res
}

#[no_mangle]
pub unsafe extern "C" fn saw_port_sized_int_n(data: *const u8, width: u8) -> u64 {
    let bytes = core::slice::from_raw_parts(data, usize::from(width));
    cf_port_get_sized_int(bytes, width)
}

#[no_mangle]
pub unsafe extern "C" fn saw_port_sized_int_8(data: *const u8) -> u64 {
    let bytes = core::slice::from_raw_parts(data, 8);
    cf_port_get_sized_int(bytes, 8)
}

#[inline]
unsafe fn saw_put_be64(output: *mut u8, value: u64) {
    let mut index = 0_usize;
    while index < 8 {
        *output.add(index) = (value >> (56 - 8 * index)) as u8;
        index += 1;
    }
}

/// Source-faithful successful-path value/cursor projection of `_readInt`.
#[no_mangle]
pub unsafe extern "C" fn saw_port_read_int_success_projection(
    data: *const u8,
    marker: u8,
    output: *mut u8,
) -> u8 {
    let declared_width = 1_usize << usize::from(marker & 0x0f);
    let folded_width = declared_width as u8;
    let bytes = core::slice::from_raw_parts(data, usize::from(folded_width));
    let value = cf_port_get_sized_int(bytes, folded_width);
    saw_put_be64(output, value);
    saw_put_be64(output.add(8), 1 + declared_width as u64);
    0
}
