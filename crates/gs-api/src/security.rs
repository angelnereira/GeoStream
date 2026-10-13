//! Principal extraction from `Authorization: Bearer` (docs/07 §3, docs/06 §3–§4).
//!
//! The credential is classified but not verified here: verification (constant-time hash for
//! API keys, EdDSA signature and claims for device tokens) is `gs-auth`'s job, and the state
//! checks of `docs/06` §4.3 step 4 belong to the authentication middleware that F3 adds on top.
//! The credential is kept in a `Zeroizing` string that is never rendered (docs/10 §8.2); once
//! DR-20261012-04 is resolved this becomes the `AccessToken` newtype of `gs-observability`.
#![forbid(unsafe_code)]

use std::fmt;

use gs_service::gs_domain::error::{AsErrorCode, ErrorCode};
use thiserror::Error;
use zeroize::Zeroizing;

/// Kind of credential presented in `Authorization` (docs/06 §3.1, §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    /// Application API key (`gsk_…`).
    ApplicationKey,
    /// Device JWT (`eyJ…`).
    DeviceToken,
}

impl CredentialKind {
    /// Classifies a bearer token.
    pub fn of(token: &str) -> Self {
        if token.starts_with("gsk_") {
            Self::ApplicationKey
        } else {
            Self::DeviceToken
        }
    }

    /// Principal kind label used in logs and metrics (docs/16 §4.2).
    pub const fn principal_kind(&self) -> &'static str {
        match self {
            Self::ApplicationKey => "application",
            Self::DeviceToken => "device",
        }
    }
}

/// Authenticated principal of a request.
#[derive(Clone)]
pub struct Principal {
    /// Kind of credential presented.
    pub kind: CredentialKind,
    /// Raw credential: never rendered, zeroized on drop.
    credential: Zeroizing<String>,
}

impl Principal {
    /// Builds a principal from a bearer token.
    pub fn from_bearer(token: &str) -> Self {
        Self {
            kind: CredentialKind::of(token),
            credential: Zeroizing::new(token.to_string()),
        }
    }

    /// Exposes the credential for the single verification step that needs it.
    pub fn credential(&self) -> &str {
        &self.credential
    }
}

impl fmt::Debug for Principal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Principal")
            .field("kind", &self.kind)
            .field("credential", &"<redacted>")
            .finish()
    }
}

/// Errors produced while extracting a principal.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PrincipalError {
    /// The `Authorization` header is missing.
    #[error("missing authorization header")]
    MissingHeader,

    /// The scheme is not `Bearer`.
    #[error("unsupported authorization scheme")]
    UnsupportedScheme,

    /// The bearer token is empty or malformed.
    #[error("invalid bearer credential")]
    InvalidCredential,
}

impl AsErrorCode for PrincipalError {
    fn error_code(&self) -> ErrorCode {
        ErrorCode::Unauthenticated
    }
}

/// Reads the correlation id of a request, generating one when absent (docs/07 §1).
pub fn request_id_from(headers: &axum::http::HeaderMap) -> String {
    headers
        .get(crate::REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 128)
        .map(str::to_string)
        .unwrap_or_else(|| gs_service::gs_domain::ids::RequestId::new().to_string())
}

/// Extracts the principal from an `Authorization` header value.
///
/// The header value itself is never logged; only the classification is (docs/10 §8.2).
pub fn extract_principal(header_value: &str) -> Result<Principal, PrincipalError> {
    let mut parts = header_value.splitn(2, ' ');
    let scheme = parts.next().unwrap_or_default();
    let token = parts.next().unwrap_or_default().trim();

    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err(PrincipalError::UnsupportedScheme);
    }
    if token.is_empty() {
        return Err(PrincipalError::InvalidCredential);
    }

    Ok(Principal::from_bearer(token))
}

/// Extracts the principal from a request, returning the problem response on failure.
///
/// The error is boxed to keep the `Result` small (clippy::result_large_err).
pub fn principal_from_header(
    header_value: Option<&str>,
    request_id: &str,
) -> Result<Principal, Box<crate::problem::ProblemResponse>> {
    let header_value = header_value.ok_or_else(|| {
        crate::problem::ProblemResponse::new(crate::problem::Problem::new(
            ErrorCode::Unauthenticated,
            request_id,
        ))
    })?;

    extract_principal(header_value).map_err(|error| {
        Box::new(crate::problem::ProblemResponse::new(
            crate::problem::Problem::new(error.error_code(), request_id)
                .with_detail(error.to_string()),
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const API_KEY: &str =
        "gsk_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a.secrettosecrettosecrettosecrettosecret1234";

    #[test]
    fn test_bearer_tokens_are_classified_without_revealing_them() {
        // covers: docs/06 §3.1, docs/10 §8.2
        let principal = extract_principal(&format!("Bearer {API_KEY}")).expect("principal");
        assert_eq!(principal.kind, CredentialKind::ApplicationKey);
        assert_eq!(principal.kind.principal_kind(), "application");
        assert_eq!(principal.credential(), API_KEY);

        let rendered = format!("{principal:?}");
        assert!(!rendered.contains(API_KEY), "credential leaked: {rendered}");
        assert!(
            rendered.contains("<redacted>"),
            "credential must be redacted: {rendered}"
        );

        let token = extract_principal("Bearer eyJhbGciOiJFZERTQSJ9.eyJzdWIiOiJzdWJfMSJ9.sig")
            .expect("principal");
        assert_eq!(token.kind, CredentialKind::DeviceToken);
        assert_eq!(token.kind.principal_kind(), "device");
    }

    #[test]
    fn test_header_errors_map_to_unauthorized() {
        // covers: FR-API-001, docs/07 §3
        assert_eq!(
            extract_principal("Basic abc").err(),
            Some(PrincipalError::UnsupportedScheme)
        );
        assert_eq!(
            extract_principal("Bearer ").err(),
            Some(PrincipalError::InvalidCredential)
        );
        assert_eq!(
            PrincipalError::MissingHeader.error_code(),
            ErrorCode::Unauthenticated
        );
    }

    #[test]
    fn test_missing_header_returns_the_problem_response() {
        // covers: FR-API-001
        let response = principal_from_header(None, "req_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a")
            .expect_err("missing header must fail");
        assert_eq!(response.problem.status, 401);
        assert_eq!(response.problem.code, "UNAUTHENTICATED");
        assert_eq!(
            response.problem.request_id,
            "req_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a"
        );
    }
}
