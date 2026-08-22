#![no_std]

// Compile the exact dependency-free source module used by production parsing.
// The run script also verifies the complete file's pinned SHA-256.
#[path = "../../src/backend/cf_compat/binary_kernels.rs"]
mod binary_kernels;

#[no_mangle]
pub unsafe extern "C" fn saw_production_sized_int_n(data: *const u8, width: u8) -> u64 {
    let bytes = core::slice::from_raw_parts(data, usize::from(width));
    binary_kernels::wide_be_u64(bytes)
}

#[no_mangle]
pub unsafe extern "C" fn saw_production_sized_int_8(data: *const u8) -> u64 {
    let bytes = core::slice::from_raw_parts(data, 8);
    binary_kernels::wide_be_u64(bytes)
}

#[inline]
unsafe fn saw_put_be64(output: *mut u8, value: u64) {
    let mut index = 0_usize;
    while index < 8 {
        *output.add(index) = (value >> (56 - 8 * index)) as u8;
        index += 1;
    }
}

/// Successful-path value/cursor projection using the exact production value
/// kernel. The parser independently validates and advances the declared span.
#[no_mangle]
pub unsafe extern "C" fn saw_production_read_int_success_projection(
    data: *const u8,
    marker: u8,
    output: *mut u8,
) -> u8 {
    let declared_width = 1_usize << usize::from(marker & 0x0f);
    // The implicit C conversion can inspect at most 128 bytes: widths of 256
    // and above convert to zero before `_getSizedInt` reads the input.
    let bytes = core::slice::from_raw_parts(data, 128);
    let value = binary_kernels::cf_sized_int_value(bytes, declared_width);
    saw_put_be64(output, value);
    saw_put_be64(output.add(8), 1 + declared_width as u64);
    0
}
