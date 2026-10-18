//! Application scopes for API keys and device tokens (docs/06_IDENTITY_AUTH.md §3.4).
#![forbid(unsafe_code)]

use thiserror::Error;

/// Every application scope defined in docs/06 §3.4.
pub const APPLICATION_SCOPES: &[&str] = &[
    "tokens:mint",
    "sessions:read",
    "sessions:write",
    "members:write",
    "invitations:write",
    "locations:read",
    "stream:connect",
    "subjects:manage",
    "webhooks:manage",
    "geofences:manage",
    // E-track (docs/06 §3.4). None of these is granted by default: each is
    // conceded one by one, explicitly.
    "pools:manage",
    "pools:query",
    "pools:density",
    "tools:use",
    "providers:manage",
    "history:read",
    "analytics:read",
    "tags:manage",
    "lpwan:manage",
];

/// Every device scope defined in docs/06 §6.
pub const DEVICE_SCOPES: &[&str] = &[
    "sessions:create",
    "invitations:redeem",
    "stream:connect",
    "sharing:write",
    "ingest:write",
    // E-track (docs/06 §6).
    "pools:density",
    "tools:use",
    "tags:manage",
    "edge:report",
    "edge:lookup",
];

/// Device scopes an application may request by default (docs/06 §4.1).
pub const DEFAULT_DEVICE_TOKEN_SCOPES: &[&str] = &[
    "sessions:create",
    "invitations:redeem",
    "stream:connect",
    "sharing:write",
];

/// Scope granted only explicitly: reading coordinates (docs/06 §3.4).
pub const LOCATIONS_READ_SCOPE: &str = "locations:read";

/// Default scopes of a new API key: all of them except `locations:read` (docs/06 §3.4).
pub const DEFAULT_API_KEY_SCOPES: &[&str] = &[
    "tokens:mint",
    "sessions:read",
    "sessions:write",
    "members:write",
    "invitations:write",
    "stream:connect",
    "subjects:manage",
    "webhooks:manage",
    "geofences:manage",
];

/// Errors produced when validating a scope list against docs/06 §3.4.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ScopeError {
    /// The scope is not part of the catalog of docs/06 §3.4.
    #[error("unknown application scope '{scope}' (docs/06 §3.4)")]
    UnknownScope { scope: String },

    /// The scope list contains an empty entry.
    #[error("empty scope is not allowed")]
    EmptyScope,

    /// The scope list repeats a scope.
    #[error("duplicate application scope '{scope}'")]
    DuplicateScope { scope: String },
}

/// Returns true when the scope belongs to the catalog of docs/06 §3.4.
pub fn is_application_scope(scope: &str) -> bool {
    APPLICATION_SCOPES.contains(&scope)
}

/// Returns true when the scope belongs to the catalog of docs/06 §6.
pub fn is_device_scope(scope: &str) -> bool {
    DEVICE_SCOPES.contains(&scope)
}

/// Default scopes of a new API key as owned strings (docs/06 §3.4).
pub fn default_api_key_scopes() -> Vec<String> {
    DEFAULT_API_KEY_SCOPES
        .iter()
        .map(|scope| (*scope).to_string())
        .collect()
}

/// Validates application scopes (docs/06 §3.4): no empty entries, no duplicates, known scopes.
pub fn validate_scopes<S: AsRef<str>>(
    scopes: impl IntoIterator<Item = S>,
) -> Result<(), ScopeError> {
    validate_against(scopes, APPLICATION_SCOPES)
}

/// Validates device scopes (docs/06 §6): no empty entries, no duplicates, known scopes.
pub fn validate_device_scopes<S: AsRef<str>>(
    scopes: impl IntoIterator<Item = S>,
) -> Result<(), ScopeError> {
    validate_against(scopes, DEVICE_SCOPES)
}

/// Validates a scope list against a catalog of docs/06.
fn validate_against<S: AsRef<str>>(
    scopes: impl IntoIterator<Item = S>,
    catalog: &[&str],
) -> Result<(), ScopeError> {
    let mut seen: Vec<String> = Vec::new();
    for scope in scopes {
        let scope = scope.as_ref();
        if scope.is_empty() {
            return Err(ScopeError::EmptyScope);
        }
        if !catalog.contains(&scope) {
            return Err(ScopeError::UnknownScope {
                scope: scope.to_string(),
            });
        }
        let owned = scope.to_string();
        if seen.contains(&owned) {
            return Err(ScopeError::DuplicateScope { scope: owned });
        }
        seen.push(owned);
    }
    Ok(())
}
