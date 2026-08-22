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
 * Modified 2026-08-22 by the plist-rs contributors: added proof-only C ABI
 * adapters and a successful `_readInt` value/cursor projection. The run
 * script supplies get_sized_int.extracted.inc,
 * extracted byte-for-byte from the pinned APSL CFBinaryPList.c after checking
 * both the complete source hash and the extracted-function hash.
 */

typedef __UINT8_TYPE__ uint8_t;
typedef __UINT16_TYPE__ uint16_t;
typedef __UINT32_TYPE__ uint32_t;
typedef __UINT64_TYPE__ uint64_t;
typedef __INTPTR_TYPE__ CFIndex;

#define CF_INLINE static inline
#define CFSwapInt16BigToHost(value) __builtin_bswap16(value)
#define CFSwapInt32BigToHost(value) __builtin_bswap32(value)
#define CFSwapInt64BigToHost(value) __builtin_bswap64(value)

#include "get_sized_int.extracted.inc"

uint64_t saw_apple_get_sized_int_n(const uint8_t *data, uint8_t width) {
    return _getSizedInt(data, width);
}

uint64_t saw_apple_get_sized_int_8(const uint8_t *data) {
    return _getSizedInt(data, 8);
}

static void saw_put_be64(uint8_t *output, uint64_t value) {
    for (uint8_t index = 0; index < 8; ++index) {
        output[index] = (uint8_t)(value >> (56 - 8 * index));
    }
}

/* Successful-path value/cursor projection of pinned `_readInt`. The proof
 * domain supplies the already-validated complete payload, so the pointer
 * overflow and extent checks intentionally remain outside this adapter. */
uint8_t saw_apple_read_int_success_projection(
    const uint8_t *data,
    uint8_t marker,
    uint8_t output[16]
) {
    uint64_t cnt = 1 << (marker & 0x0f);
    uint64_t value = _getSizedInt(data, cnt);
    saw_put_be64(output, value);
    saw_put_be64(output + 8, 1 + cnt);
    return 0;
}
