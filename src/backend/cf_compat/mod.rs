//! Core Foundation-compatible parsing backends.

#[cfg(feature = "binary")]
mod binary;
#[allow(dead_code)]
#[cfg(feature = "xml")]
mod xml;

#[cfg(all(feature = "backend-cf-compat", feature = "binary"))]
pub(crate) use self::binary::parse_cf as parse_binary_cf;
#[cfg(all(feature = "backend-pure", feature = "binary"))]
pub(crate) use self::binary::parse_pure as parse_binary_pure;

#[cfg(all(feature = "backend-cf-compat", feature = "xml"))]
pub(crate) use self::xml::parse_xml_cf;
#[cfg(all(feature = "backend-pure", feature = "xml"))]
pub(crate) use self::xml::parse_xml_pure;
