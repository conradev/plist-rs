#![cfg(all(
    feature = "binary",
    feature = "backend-pure",
    feature = "backend-cf-compat"
))]

use plist::{BackendKind, Format, Parser};

fn parser(backend: BackendKind) -> Parser {
    Parser::new().backend(backend).format(Format::Binary)
}

#[test]
fn fingerprints_preserve_recursive_cf_equality() {
    let objects = vec![
        collection(0xc0, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12], 1),
        vec![0x10, 1],
        real32(1.0),
        ascii("A"),
        utf16("A"),
        collection(0xa0, &[1, 3], 1),
        collection(0xa0, &[2, 4], 1),
        date(0.0),
        date(-0.0),
        real32(f32::NAN),
        real64(f64::NAN),
        dictionary(&[3], &[1], 1),
        dictionary(&[4], &[2], 1),
    ];
    let input = binary_plist(&objects, 0, 1);

    let document = parser(BackendKind::CoreFoundation).parse(&input).unwrap();
    // Each pair is equal under the pinned CF callbacks despite using a
    // different binary representation. This covers numeric cross-type
    // equality, ASCII/UTF-16 equality, signed-zero dates, NaNs, arrays, and
    // dictionaries.
    assert_eq!(document.root().as_set().unwrap().len(), 6);
}

#[test]
fn equal_cross_encoding_dictionary_keys_remain_first_wins() {
    let objects = vec![
        dictionary(&[1, 2], &[3, 4], 1),
        ascii("key"),
        utf16("key"),
        vec![0x10, 1],
        vec![0x10, 2],
    ];
    let input = binary_plist(&objects, 0, 1);

    for backend in [BackendKind::Pure, BackendKind::CoreFoundation] {
        let document = parser(backend).parse(&input).unwrap();
        let dictionary = document.root().as_dictionary().unwrap();
        assert_eq!(dictionary.len(), 1);
        assert_eq!(
            dictionary
                .get("key")
                .unwrap()
                .as_integer()
                .unwrap()
                .as_i64(),
            Some(1)
        );
    }
}

#[test]
fn thousands_of_identity_only_values_do_not_form_one_candidate_bucket() {
    const COUNT: usize = 4_096;

    let reference_width = width_for(COUNT + 3);
    let uid_start = 3;
    let bool_index = uid_start + COUNT;
    let uid_references: Vec<_> = (uid_start..bool_index).collect();
    let value_references = vec![bool_index; COUNT];

    let mut objects = Vec::with_capacity(bool_index + 1);
    objects.push(collection(0xa0, &[1, 2], reference_width));
    objects.push(collection(0xc0, &uid_references, reference_width));
    objects.push(dictionary(
        &uid_references,
        &value_references,
        reference_width,
    ));
    objects.extend((0..COUNT).map(|_| vec![0x80, 7]));
    objects.push(vec![0x09]);

    let input = binary_plist(&objects, 0, reference_width);
    let document = parser(BackendKind::CoreFoundation).parse(&input).unwrap();
    let mut root = document.root().as_array().unwrap().iter();
    assert_eq!(root.next().unwrap().as_set().unwrap().len(), COUNT);
    assert_eq!(root.next().unwrap().as_dictionary().unwrap().len(), COUNT);
}

#[test]
fn long_common_prefix_dictionary_keys_are_normalized_by_fingerprint() {
    const COUNT: usize = 1_024;

    let reference_width = width_for(COUNT + 2);
    let value_index = COUNT + 1;
    let key_references: Vec<_> = (1..=COUNT).collect();
    let value_references = vec![value_index; COUNT];
    let mut objects = Vec::with_capacity(COUNT + 2);
    objects.push(dictionary(
        &key_references,
        &value_references,
        reference_width,
    ));
    for index in 0..COUNT {
        objects.push(utf16(&format!("{}{:04x}", "x".repeat(64), index)));
    }
    objects.push(vec![0x09]);

    let input = binary_plist(&objects, 0, reference_width);
    for backend in [BackendKind::Pure, BackendKind::CoreFoundation] {
        let document = parser(backend).parse(&input).unwrap();
        assert_eq!(document.root().as_dictionary().unwrap().len(), COUNT);
    }
}

fn ascii(value: &str) -> Vec<u8> {
    assert!(value.is_ascii());
    let mut object = Vec::new();
    push_count(&mut object, 0x50, value.len());
    object.extend_from_slice(value.as_bytes());
    object
}

fn utf16(value: &str) -> Vec<u8> {
    let units: Vec<_> = value.encode_utf16().collect();
    let mut object = Vec::new();
    push_count(&mut object, 0x60, units.len());
    for unit in units {
        object.extend_from_slice(&unit.to_be_bytes());
    }
    object
}

fn real32(value: f32) -> Vec<u8> {
    let mut object = vec![0x22];
    object.extend_from_slice(&value.to_bits().to_be_bytes());
    object
}

fn real64(value: f64) -> Vec<u8> {
    let mut object = vec![0x23];
    object.extend_from_slice(&value.to_bits().to_be_bytes());
    object
}

fn date(value: f64) -> Vec<u8> {
    let mut object = vec![0x33];
    object.extend_from_slice(&value.to_bits().to_be_bytes());
    object
}

fn collection(marker: u8, references: &[usize], reference_width: usize) -> Vec<u8> {
    let mut object = Vec::new();
    push_count(&mut object, marker, references.len());
    for &reference in references {
        push_wide_be(&mut object, reference as u64, reference_width);
    }
    object
}

fn dictionary(keys: &[usize], values: &[usize], reference_width: usize) -> Vec<u8> {
    assert_eq!(keys.len(), values.len());
    let mut object = Vec::new();
    push_count(&mut object, 0xd0, keys.len());
    for &reference in keys.iter().chain(values) {
        push_wide_be(&mut object, reference as u64, reference_width);
    }
    object
}

fn push_count(output: &mut Vec<u8>, marker: u8, count: usize) {
    if count < 15 {
        output.push(marker | count as u8);
        return;
    }

    output.push(marker | 0x0f);
    let width = width_for(count);
    output.push(0x10 | width.trailing_zeros() as u8);
    push_wide_be(output, count as u64, width);
}

fn binary_plist(objects: &[Vec<u8>], root: usize, reference_width: usize) -> Vec<u8> {
    assert!(!objects.is_empty());
    assert!(root < objects.len());
    assert!(width_for(objects.len() - 1) <= reference_width);

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
    bytes.push(reference_width as u8);
    bytes.extend_from_slice(&(objects.len() as u64).to_be_bytes());
    bytes.extend_from_slice(&(root as u64).to_be_bytes());
    bytes.extend_from_slice(&(offset_table_offset as u64).to_be_bytes());
    bytes
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
