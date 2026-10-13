//! Domain types, IDs, validation, state machines and pure rules. No I/O.
#![forbid(unsafe_code)]

pub mod error;
pub mod ids;
pub mod invitation;
pub mod location;
pub mod membership;
pub mod pagination;
pub mod session;

pub use error::*;
pub use ids::*;
pub use invitation::*;
pub use location::*;
pub use membership::*;
pub use pagination::{
    CursorError, DEFAULT_LIMIT, MAX_LIMIT, MIN_LIMIT, Page, PageCursor, PageError, PageRequest,
    filters_fingerprint, next_cursor_for, resolve_cursor, validate_limit,
};
pub use session::*;
