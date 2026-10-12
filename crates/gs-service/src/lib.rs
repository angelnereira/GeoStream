//! Use cases: sessions, members, invitations, sharing, tokens and the authorization cache.
#![forbid(unsafe_code)]

pub mod admin;
pub mod migrate {
    pub use gs_store::PgPool;
    pub use gs_store::StoreError;
    pub use gs_store::migrate::*;
}

pub use admin::{
    AdminError, CreatedApiKey, DEFAULT_APPLICATION_DEVICE_SCOPES, GeneratedSigningKeyFiles,
};
