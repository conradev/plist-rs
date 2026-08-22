#![cfg(all(
    feature = "binary",
    feature = "xml",
    feature = "backend-pure",
    feature = "legacy-api"
))]

mod common;

use std::io::Cursor;

use common::binary_plist;
use plist::{ErrorKind, Format, Limits, Parser, Plist};

#[test]
fn legacy_binary_and_auto_readers_rewind_to_byte_zero() {
    let fixture = include_bytes!("types-binary.plist");

    let mut binary = Cursor::new(fixture.as_slice());
    binary.set_position(fixture.len() as u64);
    let direct = Plist::from_binary_reader(&mut binary).unwrap();

    let mut automatic = Cursor::new(fixture.as_slice());
    automatic.set_position(fixture.len() as u64);
    let detected = Plist::from_reader(&mut automatic).unwrap();

    assert_eq!(direct, detected);
}

#[test]
fn legacy_xml_reader_starts_at_the_current_position() {
    let prefix = b"ignored prefix";
    let mut input = prefix.to_vec();
    input.extend_from_slice(br#"<plist version="1.0"><string>legacy</string></plist>"#);
    let mut cursor = Cursor::new(input);
    cursor.set_position(prefix.len() as u64);

    assert_eq!(
        Plist::from_xml_reader(&mut cursor).unwrap(),
        Plist::String("legacy".to_owned())
    );
}

#[test]
fn legacy_public_enum_variants_and_aliases_remain_usable() {
    let array: plist::Array = vec![Plist::Boolean(true), Plist::Integer(7)];
    let mut dictionary = plist::Dictionary::default();
    dictionary.insert("items".to_owned(), Plist::Array(array.clone()));

    let value = Plist::Dict(dictionary);
    match value {
        Plist::Dict(dictionary) => {
            assert_eq!(dictionary.get("items"), Some(&Plist::Array(array)));
        }
        _ => panic!("legacy dictionary variant changed"),
    }
}

#[test]
fn shared_binary_dags_cannot_expand_past_the_projection_budget() {
    const DEPTH: usize = 10;
    let mut objects = Vec::with_capacity(DEPTH + 1);
    for index in 0..DEPTH {
        let child = (index + 1) as u8;
        objects.push(vec![0xa2, child, child]);
    }
    objects.push(vec![0x09]);
    let input = binary_plist(&objects, 0);

    let error = Parser::new()
        .format(Format::Binary)
        .limits(Limits::new().with_max_objects(64))
        .parse_plist(&input)
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::LimitExceeded);
    assert_eq!(error.format(), Some(Format::Binary));

    let mut wide_root = vec![0xaf, 0x10, 16];
    wide_root.extend([1; 16]);
    let wide_input = binary_plist(&[wide_root, vec![0x09]], 0);
    let error = Parser::new()
        .format(Format::Binary)
        .limits(Limits::new().with_max_objects(2))
        .parse_plist(&wide_input)
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::LimitExceeded);
}
