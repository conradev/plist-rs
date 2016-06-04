use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::io::{Read, Seek};
use std::time::SystemTime;
use fnv::FnvHasher;

use reader::binary::from_binary_reader;
use reader::xml::from_xml_reader;
use reader::from_reader;
use result::Result;

/// Represents a property list value.
#[derive(Debug, Clone, PartialEq)]
pub enum Plist {
    /// An array or vector of plist objects
    Array(Array),
    /// A dictionary or hash map of plist objects, keyed by string
    Dict(Dictionary),
    /// A boolean value
    Boolean(bool),
    /// A data value
    Data(Vec<u8>),
    /// A date value
    DateTime(SystemTime),
    /// A floating point value
    Real(f64),
    /// An integer value
    Integer(i64),
    /// A string value
    String(String),
}

pub type Array = Vec<Plist>;
pub type Dictionary = HashMap<String, Plist, BuildHasherDefault<FnvHasher>>;

impl Plist {
    /// Decodes a binary property list value from a reader.
    pub fn from_binary_reader<R: Read + Seek>(input: &mut R) -> Result<Self> {
        from_binary_reader(input)
    }

    /// Decodes an XML property list value from a reader.
    pub fn from_xml_reader<R: Read>(input: &mut R) -> Result<Self> {
        from_xml_reader(input)
    }

    /// Decodes a binary or XML property list value from a reader, based on
    /// the presence of the binary plist magic bytes.
    pub fn from_reader<R: Read + Seek>(input: &mut R) -> Result<Self> {
        from_reader(input)
    }
}
