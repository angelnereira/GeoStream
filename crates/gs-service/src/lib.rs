//! Use cases: sessions, members, invitations, sharing, tokens and the authorization cache.
#![forbid(unsafe_code)]

pub mod migrate {
    pub use gs_store::StoreError;
    pub use gs_store::migrate::*;
}
