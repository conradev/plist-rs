#![cfg(all(feature = "binary", feature = "xml", feature = "backend-pure"))]

mod common;

use std::io::Cursor;

use common::{binary_ascii, slice_contains, str_is_within, xml};
use plist::{Format, Parser};

#[test]
fn borrowed_xml_strings_are_spans_into_the_callers_input() {
    let input = xml("<string>borrowed XML</string>");
    let document = Parser::new().format(Format::Xml).parse(&input).unwrap();
    let string = document.root().as_str().unwrap();

    assert_eq!(document.source().as_ptr(), input.as_ptr());
    assert_eq!(string, "borrowed XML");
    assert!(str_is_within(&input, string));
}

#[test]
fn borrowed_binary_payloads_are_spans_into_the_callers_input() {
    let string_input = binary_ascii("binary span");
    let string_document = Parser::new()
        .format(Format::Binary)
        .parse(&string_input)
        .unwrap();
    let string = string_document.root().as_str().unwrap();
    assert!(str_is_within(&string_input, string));

    let data_input = common::binary_singleton(&[0x44, 1, 2, 3, 4]);
    let data_document = Parser::new()
        .format(Format::Binary)
        .parse(&data_input)
        .unwrap();
    let data = data_document.root().as_data().unwrap();
    assert_eq!(data, &[1, 2, 3, 4]);
    assert!(slice_contains(&data_input, data));
}

#[test]
fn normalized_xml_payloads_allocate_only_the_normalized_value() {
    let string_input = xml("<string>a&amp;b</string>");
    let string_document = Parser::new()
        .format(Format::Xml)
        .parse(&string_input)
        .unwrap();
    let string = string_document.root().as_str().unwrap();
    assert_eq!(string, "a&b");
    assert!(!str_is_within(&string_input, string));

    let data_input = xml("<data>AQIDBA==</data>");
    let data_document = Parser::new()
        .format(Format::Xml)
        .parse(&data_input)
        .unwrap();
    let data = data_document.root().as_data().unwrap();
    assert_eq!(data, &[1, 2, 3, 4]);
    assert!(!slice_contains(&data_input, data));
}

#[test]
fn into_owned_copies_the_source_once_and_rebases_borrowed_spans() {
    let input = xml("<string>owned span</string>");
    let borrowed = Parser::new().format(Format::Xml).parse(&input).unwrap();
    let owned = borrowed.into_owned();

    assert_eq!(owned.source(), input.as_slice());
    assert_ne!(owned.source().as_ptr(), input.as_ptr());
    let string = owned.root().as_str().unwrap();
    assert_eq!(string, "owned span");
    assert!(str_is_within(owned.source(), string));
}

#[test]
fn reader_frontend_owns_one_source_buffer_with_views_into_it() {
    let input = binary_ascii("reader span");
    let owned = Parser::new()
        .format(Format::Binary)
        .read(Cursor::new(input.as_slice()))
        .unwrap();

    assert_eq!(owned.source(), input.as_slice());
    assert_ne!(owned.source().as_ptr(), input.as_ptr());
    let string = owned.root().as_str().unwrap();
    assert!(str_is_within(owned.source(), string));
}

#[test]
fn parse_owned_reuses_the_callers_box_allocation() {
    let source = binary_ascii("boxed span").into_boxed_slice();
    let source_pointer = source.as_ptr();
    let owned = Parser::new()
        .format(Format::Binary)
        .parse_owned(source)
        .unwrap();

    assert_eq!(owned.source().as_ptr(), source_pointer);
    let string = owned.root().as_str().unwrap();
    assert!(str_is_within(owned.source(), string));
}
