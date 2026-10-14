//! Domain types, IDs, validation, state machines and pure rules. No I/O.
#![forbid(unsafe_code)]

pub mod error;
pub mod ids;
pub mod invitation;
pub mod location;
pub mod membership;
pub mod pagination;
pub mod session;
pub mod subprotocol;

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
pub use subprotocol::{
    SUBPROTOCOL_JSON, SUBPROTOCOL_PROTO, Subprotocol, negotiate as negotiate_subprotocol,
};
