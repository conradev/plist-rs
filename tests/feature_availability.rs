#![cfg(any(
    not(feature = "backend-pure"),
    all(feature = "backend-pure", not(feature = "binary")),
    all(feature = "backend-pure", not(feature = "xml"))
))]

use plist::{BackendKind, ErrorKind, Format, Parser};

#[cfg(not(feature = "backend-pure"))]
#[test]
fn disabled_default_backend_reports_requested_context() {
    let error = Parser::new()
        .format(Format::Xml)
        .parse(b"<plist version=\"1.0\"><true/></plist>")
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::BackendUnavailable);
    assert_eq!(error.format(), Some(Format::Xml));
    assert_eq!(error.backend(), Some(BackendKind::Pure));
}

#[cfg(all(feature = "backend-pure", not(feature = "binary")))]
#[test]
fn disabled_binary_format_reports_selected_backend() {
    let error = Parser::new()
        .format(Format::Binary)
        .parse(b"bplist00")
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::FormatUnavailable);
    assert_eq!(error.format(), Some(Format::Binary));
    assert_eq!(error.backend(), Some(BackendKind::Pure));
}

#[cfg(all(feature = "backend-pure", not(feature = "xml")))]
#[test]
fn disabled_xml_format_reports_selected_backend() {
    let error = Parser::new()
        .format(Format::Xml)
        .parse(b"<plist version=\"1.0\"><true/></plist>")
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::FormatUnavailable);
    assert_eq!(error.format(), Some(Format::Xml));
    assert_eq!(error.backend(), Some(BackendKind::Pure));
}
