#![cfg(all(
    feature = "binary",
    feature = "xml",
    feature = "backend-pure",
    feature = "backend-cf-compat"
))]

mod common;

use common::{binary_plist, binary_singleton};
use plist::{BackendKind, Format, Parser, ValueKind};

fn parser(backend: BackendKind, format: Format) -> Parser {
    Parser::new().backend(backend).format(format)
}

#[test]
fn cf_binary_accepts_the_historical_version_wildcard() {
    let mut input = binary_singleton(&[0x09]);
    input[7] = b'7';

    let pure = parser(BackendKind::Pure, Format::Binary);
    let cf = parser(BackendKind::CoreFoundation, Format::Binary);
    assert!(pure.parse(&input).is_err());
    assert_eq!(cf.parse(&input).unwrap().root().as_bool(), Some(true));
}

#[test]
fn cf_binary_eight_bit_string_maps_bytes_to_unicode_scalars() {
    let input = binary_singleton(&[0x52, 0x80, 0xff]);
    let pure = parser(BackendKind::Pure, Format::Binary);
    let cf = parser(BackendKind::CoreFoundation, Format::Binary);

    assert!(pure.parse(&input).is_err());
    assert_eq!(
        cf.parse(&input).unwrap().root().as_str(),
        Some("\u{80}\u{ff}")
    );
}

#[test]
fn cf_binary_extended_count_preserves_c_uint8_width_narrowing() {
    // CFBinaryPList.c's `_readInt` computes 2^8 in uint64_t, passes it through
    // `_getSizedInt`'s uint8_t width (therefore zero), then advances all 256
    // bytes. The final 0x01 proves that folding the declared width would differ.
    let mut object = vec![0x4f, 0x18];
    object.extend(std::iter::repeat(0).take(255));
    object.push(1);
    let input = binary_singleton(&object);

    let pure = parser(BackendKind::Pure, Format::Binary);
    let cf = parser(BackendKind::CoreFoundation, Format::Binary);
    assert!(pure.parse(&input).is_err());
    assert_eq!(cf.parse(&input).unwrap().root().as_data(), Some(&[][..]));
}

#[test]
fn cf_binary_exposes_null_uid_and_set_extensions() {
    let pure = parser(BackendKind::Pure, Format::Binary);
    let cf = parser(BackendKind::CoreFoundation, Format::Binary);

    let null = binary_singleton(&[0x00]);
    assert!(pure.parse(&null).is_err());
    assert_eq!(cf.parse(&null).unwrap().root().kind(), ValueKind::Null);

    let uid = binary_singleton(&[0x80, 0x2a]);
    assert!(pure.parse(&uid).is_err());
    assert_eq!(cf.parse(&uid).unwrap().root().as_uid(), Some(42));

    // Object 0 is a one-member set referring to object 1.
    let set = binary_plist(&[vec![0xc1, 1], vec![0x51, b'x']], 0);
    assert!(pure.parse(&set).is_err());
    let document = cf.parse(&set).unwrap();
    let members: Vec<_> = document.root().as_set().unwrap().iter().collect();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].as_str(), Some("x"));
}

#[test]
fn distinct_equal_valued_uids_keep_pointer_identity() {
    let cf = parser(BackendKind::CoreFoundation, Format::Binary);

    let distinct = binary_plist(&[vec![0xc2, 1, 2], vec![0x80, 7], vec![0x80, 7]], 0);
    let document = cf.parse(&distinct).unwrap();
    assert_eq!(document.root().as_set().unwrap().len(), 2);

    let shared = binary_plist(&[vec![0xc2, 1, 1], vec![0x80, 7]], 0);
    let document = cf.parse(&shared).unwrap();
    assert_eq!(document.root().as_set().unwrap().len(), 1);
}

#[test]
fn cf_binary_allows_primitive_non_string_dictionary_keys() {
    // Object 0 is { object 1: object 2 }, where object 1 is integer 7.
    let input = binary_plist(
        &[
            vec![0xd1, 1, 2],
            vec![0x10, 7],
            vec![0x55, b'v', b'a', b'l', b'u', b'e'],
        ],
        0,
    );
    let pure = parser(BackendKind::Pure, Format::Binary);
    let cf = parser(BackendKind::CoreFoundation, Format::Binary);

    assert!(pure.parse(&input).is_err());
    let document = cf.parse(&input).unwrap();
    let entries: Vec<_> = document.root().as_dictionary().unwrap().iter().collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0.as_integer().unwrap().as_i64(), Some(7));
    assert_eq!(entries[0].1.as_str(), Some("value"));
}

#[test]
fn duplicate_binary_dictionary_keys_keep_the_first_value() {
    // Object 0 contains two references to the same key object, followed by
    // distinct values. CFBinaryPList's CFDictionaryAddValue-style construction
    // retains the first entry when the later key compares equal.
    let input = binary_plist(
        &[
            vec![0xd2, 1, 1, 2, 3],
            vec![0x51, b'k'],
            vec![0x10, 1],
            vec![0x10, 2],
        ],
        0,
    );

    for backend in [BackendKind::Pure, BackendKind::CoreFoundation] {
        let document = parser(backend, Format::Binary).parse(&input).unwrap();
        let dictionary = document.root().as_dictionary().unwrap();
        assert_eq!(dictionary.len(), 1);
        assert_eq!(
            dictionary.get("k").unwrap().as_integer().unwrap().as_i64(),
            Some(1),
            "{backend:?} did not retain the first duplicate value"
        );
    }
}

#[test]
fn cf_xml_accepts_bare_roots_ignored_attributes_and_trailing_bytes() {
    let pure = parser(BackendKind::Pure, Format::Xml);
    let cf = parser(BackendKind::CoreFoundation, Format::Xml);

    let bare = b"<string>bare</string>";
    assert!(pure.parse(bare).is_err());
    assert_eq!(cf.parse(bare).unwrap().root().as_str(), Some("bare"));

    let attributes =
        br#"<plist version="1.0"><string implementation-detail="ignored">x</string></plist>"#;
    assert!(pure.parse(attributes).is_err());
    assert_eq!(cf.parse(attributes).unwrap().root().as_str(), Some("x"));

    let trailing = br#"<plist version="1.0"><true/></plist>not XML"#;
    assert!(pure.parse(trailing).is_err());
    assert_eq!(cf.parse(trailing).unwrap().root().as_bool(), Some(true));
}

#[test]
fn xml_profiles_preserve_their_distinct_data_rules() {
    let pure = parser(BackendKind::Pure, Format::Xml);
    let cf = parser(BackendKind::CoreFoundation, Format::Xml);

    let empty = br#"<plist version="1.0"><data/></plist>"#;
    assert_eq!(pure.parse(empty).unwrap().root().as_data(), Some(&[][..]));
    assert!(cf.parse(empty).is_err());

    // CoreFoundation ignores non-Base64 ASCII while the public grammar rejects it.
    let noisy = br#"<plist version="1.0"><data>AQ!ID</data></plist>"#;
    assert!(pure.parse(noisy).is_err());
    assert_eq!(
        cf.parse(noisy).unwrap().root().as_data(),
        Some(&[1, 2, 3][..])
    );
}
