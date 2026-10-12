//! Unit tests for the application scope catalog (docs/06 §3.4).
#![forbid(unsafe_code)]

use gs_auth::scopes::{
    APPLICATION_SCOPES, DEFAULT_API_KEY_SCOPES, DEFAULT_DEVICE_TOKEN_SCOPES, DEVICE_SCOPES,
    LOCATIONS_READ_SCOPE, ScopeError, default_api_key_scopes, is_application_scope,
    is_device_scope, validate_device_scopes, validate_scopes,
};

#[test]
fn test_every_default_api_key_scope_belongs_to_the_catalog() {
    // covers: FR-AUTH-001, docs/06 §3.4
    assert_eq!(APPLICATION_SCOPES.len(), 10);
    for scope in DEFAULT_API_KEY_SCOPES {
        assert!(
            is_application_scope(scope),
            "default scope {scope} is not in the catalog"
        );
    }
    // Reading coordinates is never granted by default.
    assert!(
        !DEFAULT_API_KEY_SCOPES.contains(&LOCATIONS_READ_SCOPE),
        "locations:read must not be granted by default"
    );
    // The default is the catalog minus `locations:read`.
    let missing: Vec<&str> = APPLICATION_SCOPES
        .iter()
        .copied()
        .filter(|scope| !DEFAULT_API_KEY_SCOPES.contains(scope))
        .collect();
    assert_eq!(missing, vec![LOCATIONS_READ_SCOPE]);
}

#[test]
fn test_default_api_key_scopes_returns_the_documented_list() {
    // covers: FR-AUTH-001, docs/06 §3.4
    let scopes = default_api_key_scopes();
    assert_eq!(scopes.len(), DEFAULT_API_KEY_SCOPES.len());
    assert!(scopes.iter().all(|s| is_application_scope(s)));
}

#[test]
fn test_validate_scopes_accepts_known_scopes() {
    // covers: FR-AUTH-001, docs/06 §3.4
    assert!(validate_scopes(["tokens:mint", "sessions:read"]).is_ok());
}

#[test]
fn test_validate_scopes_rejects_unknown_scopes() {
    // covers: FR-AUTH-001, docs/06 §3.4
    assert_eq!(
        validate_scopes(["sessions:read", "not-a-scope"]),
        Err(ScopeError::UnknownScope {
            scope: "not-a-scope".to_string()
        })
    );
}

#[test]
fn test_validate_scopes_rejects_empty_and_duplicate_scopes() {
    // covers: FR-AUTH-001, docs/06 §3.4
    assert_eq!(validate_scopes([""]), Err(ScopeError::EmptyScope));
    assert_eq!(
        validate_scopes(["tokens:mint", "tokens:mint"]),
        Err(ScopeError::DuplicateScope {
            scope: "tokens:mint".to_string()
        })
    );
}

#[test]
fn test_device_scope_catalog_matches_docs_06_6() {
    // covers: FR-AUTH-002, docs/06 §6
    assert_eq!(DEVICE_SCOPES.len(), 5);
    for scope in DEVICE_SCOPES {
        assert!(is_device_scope(scope), "device scope {scope} missing");
    }
    // Device and application scopes are different catalogs.
    assert!(validate_scopes(["sessions:create"]).is_err());
    assert!(validate_device_scopes(["tokens:mint"]).is_err());
}

#[test]
fn test_validate_device_scopes_accepts_the_documented_list() {
    // covers: FR-AUTH-002, docs/06 §6
    assert!(validate_device_scopes(DEFAULT_DEVICE_TOKEN_SCOPES).is_ok());
    assert!(validate_device_scopes(["ingest:write"]).is_ok());
    assert!(validate_device_scopes(["sharing:write", "sharing:write"]).is_err());
}
