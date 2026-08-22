#![cfg(all(
    feature = "binary",
    feature = "backend-pure",
    any(feature = "serde", feature = "serde-lite")
))]

mod common;

use common::binary_plist;
use plist::{BackendKind, ErrorKind, Format, Limits, Parser};

fn shared_binary_dag() -> Vec<u8> {
    const DEPTH: usize = 10;
    let mut objects = Vec::with_capacity(DEPTH + 1);
    for index in 0..DEPTH {
        let child = (index + 1) as u8;
        objects.push(vec![0xa2, child, child]);
    }
    objects.push(vec![0x09]);
    binary_plist(&objects, 0)
}

fn constrained_parser() -> Parser {
    Parser::new()
        .format(Format::Binary)
        .limits(Limits::new().with_max_objects(64))
}

#[cfg(feature = "serde")]
#[test]
fn serde_rejects_shared_dag_expansion_past_the_parse_limit() {
    use serde::de::{SeqAccess, Visitor};
    use serde::{Deserialize, Deserializer};
    use std::fmt;

    #[derive(Debug)]
    struct Drain;

    impl<'de> Deserialize<'de> for Drain {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            struct DrainVisitor;

            impl<'de> Visitor<'de> for DrainVisitor {
                type Value = Drain;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("a Boolean or recursively nested array")
                }

                fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E> {
                    Ok(Drain)
                }

                fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
                where
                    A: SeqAccess<'de>,
                {
                    while sequence.next_element::<Drain>()?.is_some() {}
                    Ok(Drain)
                }
            }

            deserializer.deserialize_any(DrainVisitor)
        }
    }

    let input = shared_binary_dag();
    let document = constrained_parser()
        .parse(&input)
        .expect("compact DAG parses");
    let error = document
        .deserialize::<Drain>()
        .expect_err("typed expansion must remain bounded");
    assert_eq!(error.kind(), ErrorKind::LimitExceeded);
    assert_eq!(error.format(), Some(Format::Binary));
}

#[cfg(feature = "serde")]
#[test]
fn serde_deserialization_errors_keep_document_context() {
    use serde::de::Visitor;
    use serde::{Deserialize, Deserializer};
    use std::fmt;

    #[derive(Debug)]
    struct Reject;

    impl<'de> Deserialize<'de> for Reject {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            struct RejectVisitor;

            impl<'de> Visitor<'de> for RejectVisitor {
                type Value = Reject;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("a rejected Boolean")
                }

                fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    Err(E::custom("deliberate typed rejection"))
                }
            }

            deserializer.deserialize_any(RejectVisitor)
        }
    }

    let input = binary_plist(&[vec![0x09]], 0);
    let document = Parser::new()
        .format(Format::Binary)
        .parse(&input)
        .expect("Boolean parses");
    let error = document
        .deserialize::<Reject>()
        .expect_err("visitor deliberately rejects the value");
    assert_eq!(error.format(), Some(Format::Binary));
    assert_eq!(error.backend(), Some(BackendKind::Pure));
}

#[cfg(feature = "serde")]
#[test]
fn serde_serialization_rejects_shared_dag_expansion_past_the_parse_limit() {
    use serde_test::assert_ser_tokens_error;

    let input = shared_binary_dag();
    let document = constrained_parser()
        .parse(&input)
        .expect("compact DAG parses");
    assert_ser_tokens_error(
        &document.root(),
        &[],
        "property-list resource limit exceeded: Serde serialization materialization exceeds the configured object limit of 64 (binary)",
    );
}

#[cfg(feature = "serde-lite")]
#[test]
fn serde_lite_rejects_shared_dag_expansion_past_the_parse_limit() {
    let input = shared_binary_dag();
    let document = constrained_parser()
        .parse(&input)
        .expect("compact DAG parses");
    let error = document
        .to_serde_lite_intermediate()
        .expect_err("owned intermediate expansion must remain bounded");
    assert_eq!(error.kind(), ErrorKind::LimitExceeded);
    assert_eq!(error.format(), Some(Format::Binary));
}

#[cfg(feature = "serde-lite")]
#[test]
fn serde_lite_conversion_errors_keep_document_context() {
    let input = binary_plist(&[vec![0x41, 0xff]], 0);
    let document = Parser::new()
        .format(Format::Binary)
        .parse(&input)
        .expect("data object parses");
    let error = document
        .to_serde_lite_intermediate()
        .expect_err("serde-lite cannot represent plist data");
    assert_eq!(error.kind(), ErrorKind::UnsupportedValue);
    assert_eq!(error.format(), Some(Format::Binary));
    assert_eq!(error.backend(), Some(BackendKind::Pure));
}
