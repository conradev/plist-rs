//! Internal parser-backend dispatcher.

#[cfg(any(feature = "backend-pure", feature = "backend-cf-compat"))]
mod cf_compat;

use crate::document::Parsed;
use crate::{BackendKind, Error, ErrorKind, Format, ParseOptions, Result};

pub(crate) fn parse(source: &[u8], options: &ParseOptions) -> Result<Parsed> {
    match options.backend() {
        BackendKind::Pure => parse_pure(source, options),
        BackendKind::CoreFoundation => parse_core_foundation(source, options),
    }
}

#[cfg(feature = "backend-pure")]
fn parse_pure(source: &[u8], options: &ParseOptions) -> Result<Parsed> {
    match options.format() {
        Format::Binary => parse_binary_pure(source, options),
        Format::Xml => parse_xml_pure(source, options),
        Format::Auto => {
            if source.get(..8) == Some(&b"bplist00"[..]) {
                parse_binary_pure(source, options)
            } else {
                parse_xml_pure(source, options)
            }
        }
    }
}

#[cfg(not(feature = "backend-pure"))]
fn parse_pure(_source: &[u8], options: &ParseOptions) -> Result<Parsed> {
    Err(unavailable_backend(BackendKind::Pure, options.format()))
}

#[cfg(feature = "backend-cf-compat")]
fn parse_core_foundation(source: &[u8], options: &ParseOptions) -> Result<Parsed> {
    match options.format() {
        Format::Binary => parse_binary_cf(source, options),
        Format::Xml => parse_xml_cf(source, options),
        Format::Auto => {
            if source.get(..7) == Some(&b"bplist0"[..]) {
                parse_binary_cf(source, options)
            } else {
                parse_xml_cf(source, options)
            }
        }
    }
}

#[cfg(not(feature = "backend-cf-compat"))]
fn parse_core_foundation(_source: &[u8], options: &ParseOptions) -> Result<Parsed> {
    Err(unavailable_backend(
        BackendKind::CoreFoundation,
        options.format(),
    ))
}

#[cfg(all(feature = "backend-pure", feature = "binary"))]
fn parse_binary_pure(source: &[u8], options: &ParseOptions) -> Result<Parsed> {
    cf_compat::parse_binary_pure(source, options)
}

#[cfg(all(feature = "backend-pure", not(feature = "binary")))]
fn parse_binary_pure(_source: &[u8], _options: &ParseOptions) -> Result<Parsed> {
    Err(unavailable_format(Format::Binary, BackendKind::Pure))
}

#[cfg(all(feature = "backend-pure", feature = "xml"))]
fn parse_xml_pure(source: &[u8], options: &ParseOptions) -> Result<Parsed> {
    cf_compat::parse_xml_pure(source, options)
}

#[cfg(all(feature = "backend-pure", not(feature = "xml")))]
fn parse_xml_pure(_source: &[u8], _options: &ParseOptions) -> Result<Parsed> {
    Err(unavailable_format(Format::Xml, BackendKind::Pure))
}

#[cfg(all(feature = "backend-cf-compat", feature = "binary"))]
fn parse_binary_cf(source: &[u8], options: &ParseOptions) -> Result<Parsed> {
    cf_compat::parse_binary_cf(source, options)
}

#[cfg(all(feature = "backend-cf-compat", not(feature = "binary")))]
fn parse_binary_cf(_source: &[u8], _options: &ParseOptions) -> Result<Parsed> {
    Err(unavailable_format(
        Format::Binary,
        BackendKind::CoreFoundation,
    ))
}

#[cfg(all(feature = "backend-cf-compat", feature = "xml"))]
fn parse_xml_cf(source: &[u8], options: &ParseOptions) -> Result<Parsed> {
    cf_compat::parse_xml_cf(source, options)
}

#[cfg(all(feature = "backend-cf-compat", not(feature = "xml")))]
fn parse_xml_cf(_source: &[u8], _options: &ParseOptions) -> Result<Parsed> {
    Err(unavailable_format(Format::Xml, BackendKind::CoreFoundation))
}

#[allow(dead_code)] // Used only by feature combinations that omit a backend.
fn unavailable_backend(backend: BackendKind, format: Format) -> Error {
    Error::new(ErrorKind::BackendUnavailable)
        .with_format(format)
        .with_backend(backend)
        .with_message("selected backend was disabled at compile time")
}

#[allow(dead_code)] // Used only by feature combinations that omit a format.
fn unavailable_format(format: Format, backend: BackendKind) -> Error {
    Error::new(ErrorKind::FormatUnavailable)
        .with_format(format)
        .with_backend(backend)
        .with_message("selected format was disabled at compile time")
}
