//! Use cases: sessions, members, invitations, sharing, tokens and the authorization cache.
#![forbid(unsafe_code)]

pub mod admin;
pub mod auth;
pub mod tokens;
pub mod migrate {
    pub use gs_store::PgPool;
    pub use gs_store::StoreError;
    pub use gs_store::migrate::*;
}

pub use admin::{
    AdminError, CreatedApiKey, DEFAULT_APPLICATION_DEVICE_SCOPES, GeneratedSigningKeyFiles,
};
pub use auth::{
    ApplicationPrincipal, AuthError, VerifiedPrincipal, require_scope, verify_application_key,
    verify_application_principal, verify_device_token_principal,
};
pub use tokens::{
    MintedToken, REQUIRED_SCOPE, TokenError, TokenRequest, TokenResponse, TokenTtlBounds,
    mint_device_token,
};

// `gs-service` is the only crate of the HTTP layer that may depend on these crates
// (docs/04 §4.1), so the layers above name their types through these re-exports instead of
// adding dependencies the dependency checker would reject.
pub use gs_auth;
pub use gs_domain;
pub use gs_store;
