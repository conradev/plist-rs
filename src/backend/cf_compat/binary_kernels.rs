//! Allocation-free scalar kernels shared by production decoding and formal
//! cross-language verification.

/// Reads a Core Foundation sized integer. Widths greater than eight retain
/// only the low 64 bits, matching unsigned C arithmetic in the pinned target.
///
/// This function deliberately stays dependency-free and total over every byte
/// slice. `verification/saw` compiles this exact production source rather than
/// a proof-only copy.
#[inline]
pub(crate) fn wide_be_u64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0_u64, |value, byte| {
        value.wrapping_shl(8) | u64::from(*byte)
    })
}

/// Applies the implicit integer conversion made by the pinned `_readInt`:
/// its declared width is `uint64_t`, while `_getSizedInt` accepts `uint8_t`.
/// This value kernel needs only the converted prefix; its caller separately
/// validates and advances over the complete declared payload.
#[inline]
pub(crate) fn cf_sized_int_value(bytes: &[u8], declared_width: usize) -> u64 {
    let folded_width = usize::from(declared_width as u8);
    debug_assert!(bytes.len() >= folded_width);
    wide_be_u64(&bytes[..folded_width])
}
