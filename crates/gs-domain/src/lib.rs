//! Domain types, IDs, validation, state machines and pure rules. No I/O.
#![forbid(unsafe_code)]

pub mod ids;
pub mod location;

pub use ids::*;
pub use location::*;
