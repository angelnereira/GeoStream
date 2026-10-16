//! Generated protocol code, frame codec and domain-to-wire conversion.
#![forbid(unsafe_code)]

#[allow(clippy::all, clippy::pedantic, clippy::nursery)]
pub mod v1 {
    include!(concat!(env!("OUT_DIR"), "/geostream.v1.rs"));
    include!(concat!(env!("OUT_DIR"), "/geostream.v1.serde.rs"));
}

pub use pbjson_types;
pub use v1::*;
