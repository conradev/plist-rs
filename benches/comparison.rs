use std::hint::black_box;
use std::io::Cursor;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use plist::{BackendKind, Format, Parser, Plist};

const BINARY: &[u8] = include_bytes!("large-input-binary.plist");
const XML: &[u8] = include_bytes!("large-input-xml.plist");

fn parser_benchmarks(criterion: &mut Criterion) {
    for (name, format, source) in [
        ("binary", Format::Binary, BINARY),
        ("xml", Format::Xml, XML),
    ] {
        let mut group = criterion.benchmark_group(format!("parse/{name}"));
        group.throughput(Throughput::Bytes(source.len() as u64));

        let pure = Parser::new().format(format).backend(BackendKind::Pure);
        group.bench_with_input(
            BenchmarkId::new("pure/document", source.len()),
            source,
            |bencher, source| {
                bencher.iter(|| {
                    pure.parse(black_box(source))
                        .expect("valid benchmark plist")
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("pure/owned-plist", source.len()),
            source,
            |bencher, source| {
                bencher.iter(|| {
                    pure.parse_plist(black_box(source))
                        .expect("valid benchmark plist")
                });
            },
        );

        #[cfg(feature = "backend-cf-compat")]
        {
            let compatibility = Parser::new()
                .format(format)
                .backend(BackendKind::CoreFoundation);
            group.bench_with_input(
                BenchmarkId::new("core-foundation/document", source.len()),
                source,
                |bencher, source| {
                    bencher.iter(|| {
                        compatibility
                            .parse(black_box(source))
                            .expect("valid benchmark plist")
                    });
                },
            );
        }

        group.finish();
    }
}

fn legacy_benchmarks(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("legacy-api");
    group.throughput(Throughput::Bytes(BINARY.len() as u64));
    group.bench_function("binary", |bencher| {
        bencher.iter(|| {
            let mut input = Cursor::new(black_box(BINARY));
            Plist::from_binary_reader(&mut input).expect("valid benchmark plist")
        });
    });

    group.throughput(Throughput::Bytes(XML.len() as u64));
    group.bench_function("xml", |bencher| {
        bencher.iter(|| {
            let mut input = Cursor::new(black_box(XML));
            Plist::from_xml_reader(&mut input).expect("valid benchmark plist")
        });
    });
    group.finish();
}

fn normalization_benchmarks(criterion: &mut Criterion) {
    const ENTRIES: usize = 4_096;

    let source = long_prefix_dictionary(ENTRIES);
    let mut group = criterion.benchmark_group("binary/normalization");
    group.throughput(Throughput::Elements(ENTRIES as u64));

    let pure = Parser::new()
        .format(Format::Binary)
        .backend(BackendKind::Pure);
    group.bench_function("pure/long-prefix-dictionary", |bencher| {
        bencher.iter(|| {
            pure.parse(black_box(&source))
                .expect("valid adversarial dictionary")
        });
    });

    #[cfg(feature = "backend-cf-compat")]
    {
        let compatibility = Parser::new()
            .format(Format::Binary)
            .backend(BackendKind::CoreFoundation);
        group.bench_function("core-foundation/long-prefix-dictionary", |bencher| {
            bencher.iter(|| {
                compatibility
                    .parse(black_box(&source))
                    .expect("valid adversarial dictionary")
            });
        });
    }

    group.finish();
}

fn long_prefix_dictionary(count: usize) -> Vec<u8> {
    let reference_width = width_for(count + 1);
    let value_index = count + 1;
    let mut root = Vec::new();
    push_count(&mut root, 0xd0, count);
    for reference in 1..=count {
        push_wide_be(&mut root, reference as u64, reference_width);
    }
    for _ in 0..count {
        push_wide_be(&mut root, value_index as u64, reference_width);
    }

    let prefix = "x".repeat(64);
    let mut objects = Vec::with_capacity(count + 2);
    objects.push(root);
    for index in 0..count {
        let value = format!("{prefix}{index:08x}");
        let mut object = Vec::new();
        push_count(&mut object, 0x60, value.encode_utf16().count());
        for unit in value.encode_utf16() {
            object.extend_from_slice(&unit.to_be_bytes());
        }
        objects.push(object);
    }
    objects.push(vec![0x09]);
    binary_plist(&objects, reference_width)
}

fn binary_plist(objects: &[Vec<u8>], reference_width: usize) -> Vec<u8> {
    let mut bytes = b"bplist00".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for object in objects {
        offsets.push(bytes.len());
        bytes.extend_from_slice(object);
    }

    let offset_table_offset = bytes.len();
    let offset_width = width_for(offset_table_offset);
    for offset in offsets {
        push_wide_be(&mut bytes, offset as u64, offset_width);
    }
    bytes.extend_from_slice(&[0; 6]);
    bytes.push(offset_width as u8);
    bytes.push(reference_width as u8);
    bytes.extend_from_slice(&(objects.len() as u64).to_be_bytes());
    bytes.extend_from_slice(&0_u64.to_be_bytes());
    bytes.extend_from_slice(&(offset_table_offset as u64).to_be_bytes());
    bytes
}

fn push_count(output: &mut Vec<u8>, marker: u8, count: usize) {
    if count < 15 {
        output.push(marker | count as u8);
    } else {
        output.push(marker | 0x0f);
        let width = width_for(count);
        output.push(0x10 | width.trailing_zeros() as u8);
        push_wide_be(output, count as u64, width);
    }
}

fn width_for(value: usize) -> usize {
    if value <= u8::MAX as usize {
        1
    } else if value <= u16::MAX as usize {
        2
    } else if value <= u32::MAX as usize {
        4
    } else {
        8
    }
}

fn push_wide_be(output: &mut Vec<u8>, value: u64, width: usize) {
    output.extend_from_slice(&value.to_be_bytes()[8 - width..]);
}

criterion_group!(
    benches,
    parser_benchmarks,
    legacy_benchmarks,
    normalization_benchmarks
);
criterion_main!(benches);
