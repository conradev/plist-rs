use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::io::Read;
use std::iter::Peekable;
use std::time::{Duration, UNIX_EPOCH};
use chrono::DateTime;
use fnv::FnvHasher;
use rustc_serialize::base64::FromBase64;
use xml::reader::{EventReader, ParserConfig, Result as XmlResult, XmlEvent};

use plist::Plist;
use result::{Result, Error};

fn xml_event<R: Iterator<Item = XmlResult<XmlEvent>>, P>(input: &mut Peekable<R>,
                                                         mut predicate: P)
                                                         -> Result<()>
    where P: FnMut(&XmlEvent) -> Option<bool>
{
    match input.next() {
        Some(Ok(e)) => {
            match predicate(&e) {
                Some(true) => Ok(()),
                Some(false) => Err(Error::UnexpectedXmlEvent(e)),
                None => xml_event(input, predicate),
            }
        }
        Some(Err(e)) => Err(Error::from(e)),
        None => Err(Error::UnexpectedXmlEof),
    }
}

fn xml_start<R: Iterator<Item = XmlResult<XmlEvent>>>(input: &mut Peekable<R>,
                                                      local_name: &str)
                                                      -> Result<()> {
    xml_event(input, |e| {
        match *e {
            XmlEvent::StartElement { ref name, .. } => Some(&name.local_name[..] == local_name),
            XmlEvent::Characters(ref _string) => None,
            _ => Some(false),
        }
    })
}

fn xml_end<R: Iterator<Item = XmlResult<XmlEvent>>>(input: &mut Peekable<R>,
                                                    local_name: &str)
                                                    -> Result<String> {
    let mut string = None;
    try!(xml_event(input, |e| {
        match *e {
            XmlEvent::EndElement { ref name } => Some(&name.local_name[..] == local_name),
            XmlEvent::Characters(ref s) => {
                // TODO: Don't clone
                string = Some(s.clone());
                None
            }
            _ => Some(false),
        }
    }));

    match string {
        Some(s) => Ok(s),
        None => Ok("".to_string()),
    }
}

fn xml_content<R: Iterator<Item = XmlResult<XmlEvent>>>(input: &mut Peekable<R>,
                                                        local_name: &str)
                                                        -> Result<String> {
    try!(xml_start(input, local_name));
    xml_end(input, local_name)
}

fn xml_boolean<R: Iterator<Item = XmlResult<XmlEvent>>>(input: &mut Peekable<R>) -> Result<Plist> {
    match input.next() {
        Some(Ok(e)) => {
            match e {
                XmlEvent::StartElement { ref name, .. } => {
                    match &name.local_name[..] {
                        "true" => {
                            try!(xml_end(input, "true"));
                            return Ok(Plist::Boolean(true));
                        }
                        "false" => {
                            try!(xml_end(input, "false"));
                            return Ok(Plist::Boolean(false));
                        }
                        _ => (),
                    }
                }
                XmlEvent::Characters(ref _string) => return xml_boolean(input),
                _ => (),
            };
            Err(Error::UnexpectedXmlEvent(e))
        }
        Some(Err(e)) => Err(Error::from(e)),
        None => Err(Error::UnexpectedXmlEof),
    }
}

fn xml_integer<R: Iterator<Item = XmlResult<XmlEvent>>>(input: &mut Peekable<R>) -> Result<Plist> {
    let string = try!(xml_content(input, "integer"));
    let integer = try!(i64::from_str_radix(string.as_ref(), 10));
    Ok(Plist::Integer(integer))
}

fn xml_real<R: Iterator<Item = XmlResult<XmlEvent>>>(input: &mut Peekable<R>) -> Result<Plist> {
    let string = try!(xml_content(input, "real"));
    let real = try!(string.parse());
    Ok(Plist::Real(real))
}

fn xml_date<R: Iterator<Item = XmlResult<XmlEvent>>>(input: &mut Peekable<R>) -> Result<Plist> {
    let string = try!(xml_content(input, "date"));
    let secs = try!(DateTime::parse_from_rfc3339(string.as_ref())).timestamp() as u64;
    Ok(Plist::DateTime(UNIX_EPOCH + Duration::from_secs(secs)))
}

fn xml_data<R: Iterator<Item = XmlResult<XmlEvent>>>(input: &mut Peekable<R>) -> Result<Plist> {
    let string = try!(xml_content(input, "data"));
    let stripped = string.split_whitespace()
        .fold(String::with_capacity(string.len()), |mut x, y| {
            x.push_str(y);
            x
        });
    Ok(Plist::Data(try!(stripped.from_base64())))
}

fn xml_string<R: Iterator<Item = XmlResult<XmlEvent>>>(input: &mut Peekable<R>) -> Result<Plist> {
    Ok(Plist::String(try!(xml_content(input, "string"))))
}

fn xml_array<R: Iterator<Item = XmlResult<XmlEvent>>>(input: &mut Peekable<R>) -> Result<Plist> {
    try!(xml_start(input, "array"));

    let mut array = Vec::new();
    loop {
        match xml_object(input) {
            Ok(o) => array.push(o),
            Err(Error::UnexpectedXmlEvent(e)) => {
                let valid = if let XmlEvent::EndElement { ref name } = e {
                    &name.local_name[..] == "array"
                } else {
                    false
                };
                if valid {
                    break;
                } else {
                    return Err(Error::UnexpectedXmlEvent(e));
                }
            }
            Err(e) => return Err(e),
        };
    }

    Ok(Plist::Array(array))
}

fn xml_dict<R: Iterator<Item = XmlResult<XmlEvent>>>(input: &mut Peekable<R>) -> Result<Plist> {
    try!(xml_start(input, "dict"));

    let fnv = BuildHasherDefault::<FnvHasher>::default();
    let mut dict = HashMap::with_hasher(fnv);
    loop {
        match xml_content(input, "key") {
            Ok(key) => {
                let value = try!(xml_object(input));
                dict.insert(key, value);
            }
            Err(Error::UnexpectedXmlEvent(e)) => {
                let valid = if let XmlEvent::EndElement { ref name } = e {
                    &name.local_name[..] == "dict"
                } else {
                    false
                };
                if valid {
                    break;
                } else {
                    return Err(Error::UnexpectedXmlEvent(e));
                }
            }
            Err(e) => return Err(e),
        };
    }

    Ok(Plist::Dict(dict))
}

fn xml_object<R: Iterator<Item = XmlResult<XmlEvent>>>(input: &mut Peekable<R>) -> Result<Plist> {
    let object_func: Option<fn(&mut Peekable<R>) -> Result<Plist>> = match input.peek() {
        Some(&Ok(XmlEvent::StartElement { ref name, .. })) => {
            Some(match &name.local_name[..] {
                "true" => xml_boolean,
                "false" => xml_boolean,
                "integer" => xml_integer,
                "real" => xml_real,
                "date" => xml_date,
                "data" => xml_data,
                "string" => xml_string,
                "array" => xml_array,
                "dict" => xml_dict,
                s => return Err(Error::XmlObjectNotSupported(s.to_string())),
            })
        }
        _ => None,
    };

    if let Some(func) = object_func {
        return func(input);
    }

    match input.next() {
        Some(Ok(XmlEvent::Characters(_string))) => xml_object(input),
        Some(Ok(e)) => Err(Error::UnexpectedXmlEvent(e)),
        Some(Err(e)) => Err(Error::from(e)),
        None => Err(Error::UnexpectedXmlEof),
    }
}


pub fn from_xml_reader<R: Read>(input: &mut R) -> Result<Plist> {
    let config = ParserConfig {
        trim_whitespace: false,
        whitespace_to_characters: true,
        cdata_to_characters: false,
        ignore_comments: true,
        coalesce_characters: true,
    };
    let mut events = EventReader::new_with_config(input, config).into_iter().peekable();

    try!(xml_event(&mut events, |e| {
        Some(if let &XmlEvent::StartDocument { .. } = e {
            true
        } else {
            false
        })
    }));
    try!(xml_start(&mut events, "plist"));
    let object = try!(xml_object(&mut events));
    try!(xml_end(&mut events, "plist"));
    Ok(object)
}
