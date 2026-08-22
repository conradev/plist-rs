#![allow(dead_code)]

/// Builds a small, structurally valid binary property list from already encoded
/// object-table entries. References inside container objects are one-byte object
/// indices, which is sufficient for every focused integration test.
pub fn binary_plist(objects: &[Vec<u8>], root: usize) -> Vec<u8> {
    assert!(!objects.is_empty());
    assert!(objects.len() < 256);
    assert!(root < objects.len());

    let mut bytes = b"bplist00".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for object in objects {
        offsets.push(bytes.len());
        bytes.extend_from_slice(object);
    }

    let offset_table_offset = bytes.len();
    let offset_width = width_for(offset_table_offset);
    for offset in offsets {
        push_wide_be(&mut bytes, offset as u64, offset_width);
    }

    bytes.extend_from_slice(&[0; 6]);
    bytes.push(offset_width as u8);
    bytes.push(1); // one-byte object references
    bytes.extend_from_slice(&(objects.len() as u64).to_be_bytes());
    bytes.extend_from_slice(&(root as u64).to_be_bytes());
    bytes.extend_from_slice(&(offset_table_offset as u64).to_be_bytes());
    bytes
}

pub fn binary_singleton(object: &[u8]) -> Vec<u8> {
    binary_plist(&[object.to_vec()], 0)
}

pub fn binary_ascii(value: &str) -> Vec<u8> {
    assert!(value.is_ascii());
    assert!(value.len() < 15);
    let mut object = vec![0x50 | value.len() as u8];
    object.extend_from_slice(value.as_bytes());
    binary_singleton(&object)
}

pub fn xml(value: &str) -> Vec<u8> {
    format!(r#"<?xml version="1.0"?><plist version="1.0">{value}</plist>"#).into_bytes()
}

pub fn slice_contains(outer: &[u8], inner: &[u8]) -> bool {
    let outer_start = outer.as_ptr() as usize;
    let outer_end = outer_start.saturating_add(outer.len());
    let inner_start = inner.as_ptr() as usize;
    let inner_end = inner_start.saturating_add(inner.len());
    outer_start <= inner_start && inner_end <= outer_end
}

pub fn str_is_within(outer: &[u8], inner: &str) -> bool {
    slice_contains(outer, inner.as_bytes())
}

fn width_for(value: usize) -> usize {
    if value <= u8::MAX as usize {
        1
    } else if value <= u16::MAX as usize {
        2
    } else if value <= u32::MAX as usize {
        4
    } else {
        8
    }
}

fn push_wide_be(output: &mut Vec<u8>, value: u64, width: usize) {
    output.extend_from_slice(&value.to_be_bytes()[8 - width..]);
}
