//! Optional typed frontends over the lossless document model.

#[cfg(feature = "serde")]
pub(crate) mod serde;

#[cfg(feature = "serde-lite")]
pub(crate) mod serde_lite;
