extern crate plist;

use std::fs::File;
use plist::Plist;

#[test]
fn test_equality() {
    let mut xf = File::open("tests/types-xml.plist").unwrap();
    let mut bf = File::open("tests/types-binary.plist").unwrap();

    let xml = Plist::from_reader(&mut xf).unwrap();
    let binary = Plist::from_reader(&mut bf).unwrap();
    assert_eq!(xml, binary);
}
