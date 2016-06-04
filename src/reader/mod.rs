use std::io::{Read, Seek, SeekFrom};

use plist::Plist;
use result::{Result, Error};

pub mod binary;
pub mod xml;

use self::binary::from_binary_reader;
use self::xml::from_xml_reader;

pub fn from_reader<R: Read + Seek>(input: &mut R) -> Result<Plist> {
    match from_binary_reader(input) {
        Ok(p) => return Ok(p),
        Err(Error::InvalidMagicBytes) => (),
        Err(e) => return Err(e),
    };

    try!(input.seek(SeekFrom::Start(0)));
    from_xml_reader(input)
}
