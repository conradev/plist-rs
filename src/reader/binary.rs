use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::io::{Read, Seek, SeekFrom};
use std::mem;
use std::str;
use std::time::{Duration, UNIX_EPOCH};
use fnv::FnvHasher;

use plist::Plist;
use result::{Result, Error};

#[inline]
fn be_u16(buf: &[u8]) -> u16 {
    ((buf[0] as u16) << 8 | (buf[1] as u16))
}

#[inline]
fn be_u32(buf: &[u8]) -> u32 {
    ((buf[0] as u32) << 24 | (buf[1] as u32) << 16 | (buf[2] as u32) << 8 | (buf[3] as u32))
}

#[inline]
fn be_u64(buf: &[u8]) -> u64 {
    ((buf[0] as u64) << 56 | (buf[1] as u64) << 48 | (buf[2] as u64) << 40 |
     (buf[3] as u64) << 32 | (buf[4] as u64) << 24 | (buf[5] as u64) << 16 |
     (buf[6] as u64) << 8 | (buf[7] as u64))
}

#[allow(unsafe_code)]
#[inline]
fn be_f32(buf: &[u8]) -> f32 {
    unsafe { mem::transmute(be_u32(buf)) }
}

#[allow(unsafe_code)]
#[inline]
fn be_f64(buf: &[u8]) -> f64 {
    unsafe { mem::transmute(be_u64(buf)) }
}

#[inline]
fn validate_size(size: u8) -> Result<u8> {
    if (size & (!size + 1)) == size && size >> 4 == 0 {
        Ok(size)
    } else {
        return Err(Error::InvalidIntegerSize);
    }
}

#[inline]
fn sized_int(buf: &[u8], size: u8) -> u64 {
    match size {
        1 => buf[0] as u64,
        2 => be_u16(buf) as u64,
        4 => be_u32(buf) as u64,
        8 => be_u64(buf),
        _ => panic!("Invalid integer size"),
    }
}

#[inline]
fn sized_ints<R: Read>(input: &mut R, size: u8, count: usize) -> Result<Vec<u64>> {
    let len = size as usize * count;
    let mut buf = Vec::with_capacity(len);
    try!(input.take(len as u64).read_to_end(&mut buf));
    Ok(buf.chunks(size as usize)
        .map(|x| sized_int(x, size))
        .collect())
}

#[inline]
fn read_sized<R: Read>(input: &mut R) -> Result<([u8; 8], u8)> {
    let mut len = [0; 1];
    let mut buf = [0; 8];
    try!(input.read_exact(&mut len));

    let size = try!(validate_size(1u8 << (len[0] & 0xF)));
    try!(input.read_exact(&mut buf[0..size as usize]));
    Ok((buf, size))
}

#[inline]
fn read_int<R: Read>(input: &mut R) -> Result<u64> {
    let mut buf = [0; 1];
    try!(input.read_exact(&mut buf));
    if (buf[0] & 0xF) == 0xF {
        let (buf, len) = try!(read_sized(input));
        Ok(sized_int(&buf[..], len))
    } else {
        Ok(u64::from(buf[0] & 0xF))
    }
}

#[inline]
fn trailer<R: Read + Seek>(input: &mut R) -> Result<(usize, u8, Vec<u64>)> {
    let mut trailer = [0; 26];
    try!(input.seek(SeekFrom::End(-26)));
    try!(input.read_exact(&mut trailer));

    let offset_size = try!(validate_size(trailer[0]));
    let ref_size = try!(validate_size(trailer[1]));
    let obj_count = be_u64(&trailer[2..]);
    let root = be_u64(&trailer[10..]) as usize;
    let table_offset = be_u64(&trailer[18..]);

    try!(input.seek(SeekFrom::Start(table_offset)));
    let offsets = try!(sized_ints(input, offset_size, obj_count as usize));

    Ok((root, ref_size, offsets))
}

#[inline]
fn boolean<R: Read>(input: &mut R) -> Result<Plist> {
    let mut buf = [0; 1];
    try!(input.read_exact(&mut buf));
    match buf[0] & 0xF {
        0x8 => Ok(Plist::Boolean(false)),
        0x9 => Ok(Plist::Boolean(true)),
        _ => Err(Error::InvalidBoolean),
    }
}

#[inline]
fn integer<R: Read + Seek>(input: &mut R) -> Result<Plist> {
    try!(input.seek(SeekFrom::Current(1)));
    Ok(Plist::Integer(try!(read_int(input)) as i64))
}

#[inline]
fn real<R: Read>(input: &mut R) -> Result<Plist> {
    let (buf, len) = try!(read_sized(input));
    let real = match len {
        4 => be_f32(&buf) as f64,
        8 => be_f64(&buf),
        _ => return Err(Error::InvalidIntegerSize),
    };
    Ok(Plist::Real(real))
}

#[inline]
fn date<R: Read>(input: &mut R) -> Result<Plist> {
    let mut buf = [0; 9];
    try!(input.read_exact(&mut buf));
    let secs = be_f64(&buf[1..]);
    let ref_date = UNIX_EPOCH + Duration::from_secs(978307200);
    let duration = Duration::new(secs.trunc() as u64, (secs.fract() * 10e9) as u32);
    Ok(Plist::DateTime(ref_date + duration))
}

#[inline]
fn data<R: Read>(input: &mut R) -> Result<Plist> {
    let len = try!(read_int(input)) as usize;
    let mut buf = Vec::with_capacity(len);
    try!(input.take(len as u64).read_to_end(&mut buf));
    Ok(Plist::Data(buf))
}

#[inline]
fn string<R: Read>(input: &mut R) -> Result<Plist> {
    let len = try!(read_int(input)) as usize;
    let mut buf = Vec::with_capacity(len);
    try!(input.take(len as u64).read_to_end(&mut buf));
    Ok(Plist::String(try!(String::from_utf8(buf))))
}

#[inline]
fn utf16_string<R: Read>(input: &mut R) -> Result<Plist> {
    let len = try!(read_int(input)) as usize;
    let mut buf = Vec::with_capacity(len * 2);
    try!(input.take((len * 2) as u64).read_to_end(&mut buf));
    let points: Vec<u16> = buf.chunks(2).map(|x| be_u16(x)).collect();
    Ok(Plist::String(try!(String::from_utf16(&points[..]))))
}

#[inline]
fn array<R: Read + Seek>(input: &mut R, ref_size: u8, offsets: &Vec<u64>) -> Result<Plist> {
    let len = try!(read_int(input)) as usize;
    let values = try!(sized_ints(input, ref_size, len));

    let mut array = Vec::with_capacity(len);
    for v in values {
        let value = try!(object(input, v as usize, ref_size, offsets));
        array.push(value);
    }

    Ok(Plist::Array(array))
}

#[inline]
fn dict<R: Read + Seek>(input: &mut R, ref_size: u8, offsets: &Vec<u64>) -> Result<Plist> {
    let len = try!(read_int(input)) as usize;
    let keys = try!(sized_ints(input, ref_size, len));
    let values = try!(sized_ints(input, ref_size, len));

    let fnv = BuildHasherDefault::<FnvHasher>::default();
    let mut dict = HashMap::with_capacity_and_hasher(len, fnv);

    for (k, v) in keys.into_iter().zip(values.into_iter()) {
        let key = match try!(object(input, k as usize, ref_size, offsets)) {
            Plist::String(s) => s,
            _ => return Err(Error::InvalidKeyObject),
        };

        let value = try!(object(input, v as usize, ref_size, offsets));
        dict.insert(key, value);
    }

    Ok(Plist::Dict(dict))
}

fn object<R: Read + Seek>(input: &mut R,
                          obj: usize,
                          ref_size: u8,
                          offsets: &Vec<u64>)
                          -> Result<Plist> {
    let mut buf = [0; 1];
    let offset = SeekFrom::Start(offsets[obj]);
    try!(input.seek(offset));
    try!(input.read_exact(&mut buf));
    try!(input.seek(offset));

    let obj_type = buf[0] >> 4;
    match obj_type {
        0x0 => boolean(input),
        0x1 => integer(input),
        0x2 => real(input),
        0x3 => date(input),
        0x4 => data(input),
        0x5 => string(input),
        0x6 => utf16_string(input),
        0xA => array(input, ref_size, offsets),
        0xD => dict(input, ref_size, offsets),
        _ => Err(Error::ObjectNotSupported(obj_type)),
    }
}

pub fn from_binary_reader<R: Read + Seek>(input: &mut R) -> Result<Plist> {
    try!(input.seek(SeekFrom::Start(0)));

    let mut magic = [0; 6];
    try!(input.read_exact(&mut magic));
    if let Ok(s) = str::from_utf8(&magic) {
        if s != "bplist" {
            return Err(Error::InvalidMagicBytes);
        }
    } else {
        return Err(Error::InvalidMagicBytes);
    }

    let mut ver = [0; 2];
    try!(input.read_exact(&mut ver));
    if let Ok(s) = str::from_utf8(&ver) {
        if s != "00" {
            return Err(Error::VersionNotSupported(Some(s.to_string())));
        }
    } else {
        return Err(Error::VersionNotSupported(None));
    }

    if let Ok((root, ref_size, offsets)) = trailer(input) {
        object(input, root, ref_size, &offsets)
    } else {
        Err(Error::InvalidTrailer)
    }
}
