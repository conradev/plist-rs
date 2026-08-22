#![cfg(all(
    feature = "binary",
    feature = "backend-pure",
    feature = "backend-cf-compat"
))]

mod common;

use common::{binary_plist, binary_singleton};
use plist::{BackendKind, Error, ErrorKind, Format, Limits, Parser};

const BACKENDS: [BackendKind; 2] = [BackendKind::Pure, BackendKind::CoreFoundation];

fn parse_error(input: &[u8], backend: BackendKind, limits: Limits) -> Error {
    Parser::new()
        .format(Format::Binary)
        .backend(backend)
        .limits(limits)
        .parse(input)
        .unwrap_err()
}

fn assert_binary_error(input: &[u8], backend: BackendKind, kind: ErrorKind, offset: usize) {
    let error = parse_error(input, backend, Limits::new());
    assert_eq!(error.kind(), kind, "unexpected category: {error}");
    assert_eq!(error.format(), Some(Format::Binary), "{error}");
    assert_eq!(error.backend(), Some(backend), "{error}");
    assert_eq!(error.offset(), Some(offset), "{error}");
    assert!(
        error.message().is_some(),
        "errors retain explanatory context"
    );
}

#[test]
fn magic_versions_and_trailer_have_distinct_categories() {
    for backend in BACKENDS {
        assert_binary_error(b"not-a-plist", backend, ErrorKind::InvalidMagic, 0);
        assert_binary_error(b"bplist0", backend, ErrorKind::UnsupportedVersion, 6);

        let mut invalid_magic = binary_singleton(&[0x09]);
        invalid_magic[0] = b'B';
        assert_binary_error(&invalid_magic, backend, ErrorKind::InvalidMagic, 0);

        let mut invalid_trailer = binary_singleton(&[0x09]);
        let trailer_start = invalid_trailer.len() - 32;
        invalid_trailer[trailer_start + 6] = 0;
        assert_binary_error(
            &invalid_trailer,
            backend,
            ErrorKind::InvalidTrailer,
            trailer_start + 6,
        );
    }

    let mut pure_version = binary_singleton(&[0x09]);
    pure_version[7] = b'1';
    assert_binary_error(
        &pure_version,
        BackendKind::Pure,
        ErrorKind::UnsupportedVersion,
        6,
    );

    let mut cf_version = binary_singleton(&[0x09]);
    cf_version[6] = b'1';
    assert_binary_error(
        &cf_version,
        BackendKind::CoreFoundation,
        ErrorKind::UnsupportedVersion,
        6,
    );
}

#[test]
fn bad_references_and_cycles_are_invalid_references() {
    let bad_reference = binary_plist(&[vec![0xa1, 2], vec![0x09]], 0);
    let cycle = binary_plist(&[vec![0xa1, 0]], 0);

    for backend in BACKENDS {
        assert_binary_error(&bad_reference, backend, ErrorKind::InvalidReference, 9);
        assert_binary_error(&cycle, backend, ErrorKind::InvalidReference, 8);

        let mut bad_root = binary_singleton(&[0x09]);
        let trailer_start = bad_root.len() - 32;
        bad_root[trailer_start + 16..trailer_start + 24].copy_from_slice(&1_u64.to_be_bytes());
        assert_binary_error(
            &bad_root,
            backend,
            ErrorKind::InvalidReference,
            trailer_start + 16,
        );
    }
}

#[test]
fn marker_and_scalar_failures_use_their_stable_categories() {
    let cases: &[(Vec<u8>, ErrorKind, usize)] = &[
        (binary_singleton(&[0x70]), ErrorKind::UnsupportedObject, 8),
        (binary_singleton(&[0x15]), ErrorKind::InvalidInteger, 8),
        (binary_singleton(&[0x13, 0]), ErrorKind::InvalidInteger, 9),
        (
            binary_singleton(&[0x4f, 0x20]),
            ErrorKind::InvalidInteger,
            9,
        ),
        (binary_singleton(&[0x20]), ErrorKind::InvalidReal, 8),
        (binary_singleton(&[0x23, 0]), ErrorKind::InvalidReal, 9),
        (binary_singleton(&[0x32]), ErrorKind::InvalidDate, 8),
        (binary_singleton(&[0x33, 0]), ErrorKind::InvalidDate, 9),
        (binary_singleton(&[0x42, 0xaa]), ErrorKind::InvalidData, 9),
        (binary_singleton(&[0x52, b'x']), ErrorKind::InvalidString, 9),
    ];

    for backend in BACKENDS {
        for (input, kind, offset) in cases {
            assert_binary_error(input, backend, *kind, *offset);
        }
    }

    // The public profile enforces marker 0x5's nominal ASCII restriction.
    // CoreFoundation instead maps every eight-bit payload byte to the same
    // Unicode scalar, so 0xff is accepted there as U+00FF.
    assert_binary_error(
        &binary_singleton(&[0x51, 0xff]),
        BackendKind::Pure,
        ErrorKind::InvalidString,
        9,
    );
}

#[test]
fn invalid_dictionary_key_types_are_reported_at_the_key() {
    // Object 1 is an array, which is neither a public string key nor a
    // CoreFoundation-compatible primitive key.
    let input = binary_plist(&[vec![0xd1, 1, 2], vec![0xa0], vec![0x51, b'v']], 0);
    for backend in BACKENDS {
        assert_binary_error(&input, backend, ErrorKind::InvalidKey, 11);
    }
}

#[test]
fn every_binary_resource_limit_is_enforced_by_both_profiles() {
    let singleton = binary_singleton(&[0x09]);
    let object_limit_offset = singleton.len() - 32 + 8;
    let nested = binary_plist(&[vec![0xa1, 1], vec![0x09]], 0);
    let container = binary_plist(&[vec![0xa1, 1], vec![0x09]], 0);
    let string = binary_singleton(&[0x51, b'x']);
    let data = binary_singleton(&[0x41, 0xff]);

    for backend in BACKENDS {
        // Input size is rejected by Parser before backend dispatch. The other
        // five limits exercise binary.rs and therefore assert full context.
        let input_error = parse_error(
            &singleton,
            backend,
            Limits::new().with_max_input_bytes(singleton.len() - 1),
        );
        assert_eq!(input_error.kind(), ErrorKind::LimitExceeded);
        assert_eq!(input_error.backend(), Some(backend));

        let cases: &[(&[u8], Limits, usize)] = &[
            (
                &singleton,
                Limits::new().with_max_objects(0),
                object_limit_offset,
            ),
            (&nested, Limits::new().with_max_depth(0), 10),
            (&container, Limits::new().with_max_container_len(0), 8),
            (&string, Limits::new().with_max_string_bytes(0), 8),
            (&data, Limits::new().with_max_data_bytes(0), 8),
        ];
        for &(input, limits, offset) in cases {
            let error = parse_error(input, backend, limits);
            assert_eq!(error.kind(), ErrorKind::LimitExceeded, "{error}");
            assert_eq!(error.format(), Some(Format::Binary), "{error}");
            assert_eq!(error.backend(), Some(backend), "{error}");
            assert_eq!(error.offset(), Some(offset), "{error}");
        }
    }
}
