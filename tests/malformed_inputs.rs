#![cfg(all(feature = "binary", feature = "xml", feature = "backend-pure"))]

mod common;

use common::binary_singleton;
use plist::{Format, Parser};

#[test]
fn truncated_and_corrupt_binary_inputs_are_rejected_without_panicking() {
    let valid = binary_singleton(&[0x09]);
    let mut bad_magic = valid.clone();
    bad_magic[0] = b'B';
    let mut bad_root = valid.clone();
    let trailer_start = bad_root.len() - 32;
    bad_root[trailer_start + 16..trailer_start + 24].copy_from_slice(&1_u64.to_be_bytes());
    let mut bad_offset = valid.clone();
    let offset_table = bad_offset.len() - 33;
    bad_offset[offset_table] = 0xff;

    let parser = Parser::new().format(Format::Binary);
    let cases: &[&[u8]] = &[
        b"",
        b"bplist00",
        &valid[..valid.len() - 1],
        &bad_magic,
        &bad_root,
        &bad_offset,
    ];
    for (index, input) in cases.iter().enumerate() {
        assert!(
            parser.parse(input).is_err(),
            "malformed case {index} parsed"
        );
    }
}

#[test]
fn malformed_xml_structures_and_values_are_rejected() {
    let parser = Parser::new().format(Format::Xml);
    let cases: &[&[u8]] = &[
        b"",
        br#"<plist version="1.0"><string>unterminated</plist>"#,
        br#"<plist version="1.0"><string>&unknown;</string></plist>"#,
        br#"<plist version="1.0"><dict><key>orphan</key></dict></plist>"#,
        br#"<plist version="1.0"><array><true></array></plist>"#,
        br#"<plist version="1.0"><integer>12x</integer></plist>"#,
        br#"<plist version="1.0"><data>AQI</data></plist>"#,
        br#"<plist version="1.0"><unknown/></plist>"#,
        br#"<plist version="1.0"><true/></plist><false/>"#,
    ];
    for (index, input) in cases.iter().enumerate() {
        assert!(
            parser.parse(input).is_err(),
            "malformed case {index} parsed"
        );
    }
}

#[test]
fn an_explicit_format_never_falls_back_to_the_other_decoder() {
    let binary = binary_singleton(&[0x09]);
    let xml = br#"<plist version="1.0"><true/></plist>"#;

    assert!(Parser::new().format(Format::Binary).parse(xml).is_err());
    assert!(Parser::new().format(Format::Xml).parse(&binary).is_err());
}
