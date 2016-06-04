#![feature(test)]

extern crate test;

#[macro_use]
#[cfg(any(target_os = "macos", target_os = "ios"))]
extern crate objc;

mod rust {
    extern crate plist;

    use std::fs::File;
    use std::io::{Read, Cursor};
    use test::Bencher;

    use self::plist::Plist;

    #[bench]
    fn bench_xml(b: &mut Bencher) {
        let mut xf = File::open("benches/large-input-xml.plist").unwrap();

        let mut buf = Vec::new();
        xf.read_to_end(&mut buf).unwrap();
        let mut cursor = Cursor::new(buf);

        b.iter(|| {
            cursor.set_position(0);
            Plist::from_xml_reader(&mut cursor).unwrap()
        });
    }

    #[bench]
    fn bench_binary(b: &mut Bencher) {
        let mut bf = File::open("benches/large-input-binary.plist").unwrap();

        let mut buf = Vec::new();
        bf.read_to_end(&mut buf).unwrap();
        let mut cursor = Cursor::new(buf);

        b.iter(|| Plist::from_binary_reader(&mut cursor).unwrap());
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod foundation {
    extern crate cocoa;

    use test::Bencher;

    use self::cocoa::base::{id, nil, class};
    use self::cocoa::foundation::NSString;

    #[bench]
    fn bench_xml(b: &mut Bencher) {
        unsafe {
            let path = NSString::alloc(nil).init_str("benches/large-input-xml.plist");
            let buf: id = msg_send![class("NSData"), dataWithContentsOfFile: path];
            b.iter(|| {
                let _obj: id =
                    msg_send![class("NSPropertyListSerialization"), propertyListWithData:buf options:0usize format:nil error:nil];
            });
        }
    }

    #[bench]
    fn bench_binary(b: &mut Bencher) {
        unsafe {
            let path = NSString::alloc(nil).init_str("benches/large-input-binary.plist");
            let buf: id = msg_send![class("NSData"), dataWithContentsOfFile: path];
            b.iter(|| {
                let _obj: id =
                    msg_send![class("NSPropertyListSerialization"), propertyListWithData:buf options:0usize format:nil error:nil];
            });
        }
    }
}

#[cfg(feature = "libplist")]
mod libplist {
    extern crate libc;

    use std::fs::File;
    use std::io::Read;
    use std::ptr;
    use test::Bencher;

    use self::libc::{uint32_t, c_void};

    #[link(name = "plist")]
    extern {
        fn plist_from_xml(data: *const u8, length: uint32_t, plist: *mut *mut c_void);
        fn plist_from_bin(data: *const u8, length: uint32_t, plist: *mut *mut c_void);
    }

    #[bench]
    fn bench_xml(b: &mut Bencher) {
        let mut bf = File::open("benches/large-input-xml.plist").unwrap();

        let mut buf = Vec::new();
        bf.read_to_end(&mut buf).unwrap();

        unsafe {
            b.iter(|| {
                let mut plist: *mut c_void = ptr::null_mut();
                plist_from_xml(buf.as_ptr(), buf.len() as u32, &mut plist as *mut *mut c_void);
                assert!(plist != ptr::null_mut());
            });
        }
    }

    #[bench]
    fn bench_binary(b: &mut Bencher) {
        let mut bf = File::open("benches/large-input-binary.plist").unwrap();

        let mut buf = Vec::new();
        bf.read_to_end(&mut buf).unwrap();

        unsafe {
            b.iter(|| {
                let mut plist: *mut c_void = ptr::null_mut();
                plist_from_bin(buf.as_ptr(), buf.len() as u32, &mut plist as *mut *mut c_void);
                assert!(plist != ptr::null_mut());
            });
        }
    }
}
