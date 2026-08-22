#![cfg(all(
    target_os = "macos",
    feature = "binary",
    feature = "xml",
    feature = "backend-pure",
    feature = "legacy-api"
))]

use std::path::Path;
use std::process::Command;

#[cfg(feature = "backend-cf-compat")]
use plist::BackendKind;
use plist::{Format, Parser, Plist};

fn plutil_convert(format: &str, path: &Path) -> Option<Vec<u8>> {
    let output = Command::new("/usr/bin/plutil")
        .args(["-convert", format, "-o", "-", "--"])
        .arg(path)
        .output()
        .ok()?;
    assert!(
        output.status.success(),
        "plutil failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Some(output.stdout)
}

#[test]
fn public_fixture_semantics_match_macos_plutil_in_both_encodings() {
    if !Path::new("/usr/bin/plutil").exists() {
        return;
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let binary_path = root.join("types-binary.plist");
    let xml_path = root.join("types-xml.plist");

    let binary = std::fs::read(&binary_path).unwrap();
    let xml_from_apple = plutil_convert("xml1", &binary_path).unwrap();
    let expected = Parser::new()
        .format(Format::Binary)
        .parse_plist(&binary)
        .unwrap();
    let converted = Parser::new()
        .format(Format::Xml)
        .parse_plist(&xml_from_apple)
        .unwrap();
    assert_eq!(expected, converted);
    #[cfg(feature = "backend-cf-compat")]
    {
        let compatible = Parser::new()
            .format(Format::Xml)
            .backend(BackendKind::CoreFoundation)
            .parse_plist(&xml_from_apple)
            .unwrap();
        assert_eq!(expected, compatible);
    }

    let xml = std::fs::read(&xml_path).unwrap();
    let binary_from_apple = plutil_convert("binary1", &xml_path).unwrap();
    let expected = Parser::new().format(Format::Xml).parse_plist(&xml).unwrap();
    let converted = Parser::new()
        .format(Format::Binary)
        .parse_plist(&binary_from_apple)
        .unwrap();
    assert_eq!(expected, converted);
    #[cfg(feature = "backend-cf-compat")]
    {
        let compatible = Parser::new()
            .format(Format::Binary)
            .backend(BackendKind::CoreFoundation)
            .parse_plist(&binary_from_apple)
            .unwrap();
        assert_eq!(expected, compatible);
    }

    // Keep a direct legacy projection in this differential suite so the oracle
    // also covers the compatibility frontend, not only Document traversal.
    assert!(matches!(converted, Plist::Dict(_)));
}
