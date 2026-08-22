#![cfg(all(
    feature = "binary",
    feature = "xml",
    feature = "backend-pure",
    feature = "legacy-api"
))]

extern crate plist;

use plist::Plist;
use std::fs::File;

#[test]
fn test_equality() {
    let mut xf = File::open("tests/types-xml.plist").unwrap();
    let mut bf = File::open("tests/types-binary.plist").unwrap();

    let xml = Plist::from_reader(&mut xf).unwrap();
    let binary = Plist::from_reader(&mut bf).unwrap();
    assert_eq!(xml, binary);
}
