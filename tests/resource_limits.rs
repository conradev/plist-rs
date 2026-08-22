#![cfg(all(feature = "xml", feature = "backend-pure"))]

mod common;

use std::io::{self, Cursor, Read};

use common::xml;
use plist::{BackendKind, ErrorKind, Format, Limits, Parser};

fn limited(limits: Limits) -> Parser {
    Parser::new().format(Format::Xml).limits(limits)
}

fn assert_limit(input: &[u8], limits: Limits) {
    let error = limited(limits).parse(input).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::LimitExceeded, "{error}");
}

#[test]
fn input_byte_limit_applies_to_slices_and_readers() {
    let input = xml("<true/>");
    let limits = Limits::new().with_max_input_bytes(input.len() - 1);
    let slice_error = limited(limits).parse(&input).unwrap_err();
    assert_eq!(slice_error.kind(), ErrorKind::LimitExceeded);
    assert_eq!(slice_error.format(), Some(Format::Xml));
    assert_eq!(slice_error.backend(), Some(BackendKind::Pure));

    let error = limited(limits)
        .read(Cursor::new(input.as_slice()))
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::LimitExceeded);
    assert_eq!(error.format(), Some(Format::Xml));
    assert_eq!(error.backend(), Some(BackendKind::Pure));
}

#[test]
fn depth_object_and_container_limits_fail_closed() {
    assert_limit(
        &xml("<array><array><true/></array></array>"),
        Limits::new().with_max_depth(1),
    );
    assert_limit(
        &xml("<array><true/><false/></array>"),
        Limits::new().with_max_objects(2),
    );
    assert_limit(
        &xml("<array><true/><false/></array>"),
        Limits::new().with_max_container_len(1),
    );
}

#[test]
fn depth_limit_is_independent_of_empty_container_spelling() {
    let limits = Limits::new().with_max_depth(0);
    for root in ["<array/>", "<array></array>", "<dict/>", "<dict></dict>"] {
        limited(limits)
            .parse(&xml(root))
            .expect("an empty root container has no child edge");
    }

    for nested in [
        "<array><array/></array>",
        "<array><array></array></array>",
        "<array><dict/></array>",
        "<array><dict></dict></array>",
    ] {
        assert_limit(&xml(nested), limits);
    }
}

#[test]
fn decoded_string_and_data_limits_are_enforced() {
    assert_limit(
        &xml("<string>four</string>"),
        Limits::new().with_max_string_bytes(3),
    );
    assert_limit(
        &xml("<string>a&amp;b</string>"),
        Limits::new().with_max_string_bytes(2),
    );
    assert_limit(
        &xml("<data>AQID</data>"),
        Limits::new().with_max_data_bytes(2),
    );
}

#[test]
fn reader_errors_retain_parser_context() {
    struct FailingReader;

    impl Read for FailingReader {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("synthetic failure"))
        }
    }

    let error = limited(Limits::new()).read(FailingReader).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Io);
    assert_eq!(error.format(), Some(Format::Xml));
    assert_eq!(error.backend(), Some(BackendKind::Pure));
}
