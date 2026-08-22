#![cfg(all(
    feature = "serde",
    feature = "binary",
    feature = "xml",
    feature = "backend-pure",
    feature = "backend-cf-compat"
))]

mod common;

use common::{binary_plist, binary_singleton, xml};
use plist::{BackendKind, Format, Parser};
use serde_test::{assert_ser_tokens, Token};

#[test]
fn lossless_special_values_keep_their_serde_shape() {
    let date_source = xml("<date>2001-01-01T00:00:00Z</date>");
    let date = Parser::new().parse(&date_source).unwrap();
    assert_ser_tokens(
        &date.root(),
        &[
            Token::NewtypeStruct {
                name: "$plist::Date",
            },
            Token::F64(0.0),
        ],
    );

    let data_source = xml("<data>AQID</data>");
    let data = Parser::new().parse(&data_source).unwrap();
    assert_ser_tokens(&data.root(), &[Token::Bytes(&[1, 2, 3])]);

    let parser = Parser::new()
        .format(Format::Binary)
        .backend(BackendKind::CoreFoundation);
    let uid_source = binary_singleton(&[0x80, 42]);
    let uid = parser.parse(&uid_source).unwrap();
    assert_ser_tokens(
        &uid.root(),
        &[
            Token::NewtypeStruct {
                name: "$plist::Uid",
            },
            Token::U32(42),
        ],
    );

    let set_source = binary_plist(&[vec![0xc1, 1], vec![0x51, b'x']], 0);
    let set = parser.parse(&set_source).unwrap();
    assert_ser_tokens(
        &set.root(),
        &[
            Token::NewtypeStruct {
                name: "$plist::Set",
            },
            Token::Seq { len: Some(1) },
            Token::Str("x"),
            Token::SeqEnd,
        ],
    );
}

#[test]
fn primitive_dictionary_keys_serialize_without_string_coercion() {
    let source = binary_plist(&[vec![0xd1, 1, 2], vec![0x10, 7], vec![0x09]], 0);
    let document = Parser::new()
        .format(Format::Binary)
        .backend(BackendKind::CoreFoundation)
        .parse(&source)
        .unwrap();
    assert_ser_tokens(
        &document.root(),
        &[
            Token::Map { len: Some(1) },
            Token::U64(7),
            Token::Bool(true),
            Token::MapEnd,
        ],
    );
}
