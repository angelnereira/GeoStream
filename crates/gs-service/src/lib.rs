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

// `gs-service` is the only crate of the HTTP layer that may depend on `gs-domain`
// (docs/04 §4.1), so the layers above name the domain types through this re-export instead
// of adding a dependency the dependency checker would reject.
pub use gs_domain;
