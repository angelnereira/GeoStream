//! `POST /v1/tokens` (docs/06 §4, OpenAPI `mintToken`, FR-AUTH-002).
//!
//! The application key travels in `Authorization: Bearer` and is never logged; the JWT is
//! built by `gs-service`, which owns the upserts of subject and device. This module only
//! adapts HTTP to that use case and maps its errors to Problem Details.
#![forbid(unsafe_code)]

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use gs_service::gs_auth::SigningKey;
use gs_service::gs_auth::api_key::parse_credential;
use gs_service::gs_domain::error::{AsErrorCode, ErrorCode};
use gs_service::migrate::PgPool;
use gs_service::tokens::{TokenError, TokenRequest, TokenTtlBounds, mint_device_token};
use zeroize::Zeroizing;

use axum::response::IntoResponse;

use crate::{Problem, ProblemResponse, REQUEST_ID_HEADER, request_id_from};

/// State shared by the token endpoint.
#[derive(Clone)]
pub struct TokenState {
    /// Database connection pool.
    pub pool: PgPool,
    /// TTL bounds from `GS_TOKEN_*` (docs/17 §2.4).
    pub ttl_bounds: TokenTtlBounds,
    /// Token issuer (`GS_JWT_ISSUER`).
    pub issuer: String,
    /// Active signing key and its kid (docs/06 §4.4).
    pub signing_key: SigningKeyRef,
}

/// Active signing key, shared and cheap to clone.
#[derive(Clone)]
pub struct SigningKeyRef {
    inner: Arc<SigningKey>,
    /// Key id of the active key.
    pub kid: String,
}

impl SigningKeyRef {
    /// Wraps a signing key and its kid.
    pub fn new(key: SigningKey, kid: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(key),
            kid: kid.into(),
        }
    }

    /// Returns the shared signing key.
    pub fn key(&self) -> &SigningKey {
        &self.inner
    }
}

impl std::fmt::Debug for TokenState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenState")
            .field("issuer", &self.issuer)
            .field("kid", &self.signing_key.kid)
            .field("pool", &"<pg pool>")
            .finish()
    }
}

/// Mounts the `/v1` routes of F2.
pub fn v1_router(state: TokenState) -> axum::Router {
    axum::Router::new()
        .route("/v1/tokens", axum::routing::post(mint_token_handler))
        .with_state(state)
}

/// `POST /v1/tokens` (docs/06 §4).
pub async fn mint_token_handler(
    State(state): State<TokenState>,
    headers: HeaderMap,
    body: Result<Json<TokenRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request_id = request_id_from(&headers);

    // The credential is extracted without logging it (docs/10 §8.2).
    let credential = match principal_extract_credential(&headers) {
        Ok(credential) => Zeroizing::new(credential),
        Err(_) => {
            let problem = Problem::new(ErrorCode::Unauthenticated, request_id)
                .with_detail("missing or malformed Authorization header");
            return ProblemResponse::new(problem).into_response();
        }
    };

    let request = match body {
        Ok(Json(request)) => request,
        Err(_) => {
            return ProblemResponse::new(
                Problem::new(ErrorCode::ValidationFailed, request_id)
                    .with_detail("request body must be a JSON TokenRequest"),
            )
            .into_response();
        }
    };

    match mint_device_token(
        &state.pool,
        credential.as_str(),
        &request,
        state.ttl_bounds,
        &state.issuer,
        state.signing_key.key(),
        &state.signing_key.kid,
    )
    .await
    {
        Ok(minted) => {
            let mut response = (StatusCode::OK, Json(minted.response)).into_response();
            if let Ok(value) = axum::http::HeaderValue::from_str(&request_id) {
                response.headers_mut().insert(REQUEST_ID_HEADER, value);
            }
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                axum::http::HeaderValue::from_static("no-store"),
            );
            response
        }
        Err(error) => token_problem(&error, &request_id).into_response(),
    }
}

/// Renders a token error as Problem Details (docs/07 §5).
fn token_problem(error: &TokenError, request_id: &str) -> ProblemResponse {
    ProblemResponse::new(
        Problem::new(error.error_code(), request_id).with_detail(error.to_string()),
    )
}

/// Extracts the bearer credential of a request.
///
/// Returns the credential as an owned string; callers keep it in a `Zeroizing` value.
pub(crate) fn principal_extract_credential(headers: &HeaderMap) -> Result<String, String> {
    let header_value = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or("missing Authorization header")?;

    let mut parts = header_value.splitn(2, ' ');
    let scheme = parts.next().unwrap_or_default();
    let token = parts.next().unwrap_or_default().trim();
    if !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() {
        return Err("unsupported Authorization scheme".to_string());
    }

    // A malformed credential is rejected with the same 401 as an unknown one.
    parse_credential(token).map_err(|_| "invalid credential".to_string())?;
    Ok(token.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn header_map(value: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(value) = value {
            headers.insert(header::AUTHORIZATION, HeaderValue::from_str(value).unwrap());
        }
        headers
    }

    #[test]
    fn test_credential_extraction_rejects_malformed_headers() {
        // covers: docs/07 §3, docs/10 §8.2
        assert!(principal_extract_credential(&header_map(None)).is_err());
        assert!(principal_extract_credential(&header_map(Some("Basic abc"))).is_err());
        assert!(principal_extract_credential(&header_map(Some("Bearer "))).is_err());
        assert!(principal_extract_credential(&header_map(Some("Bearer nope"))).is_err());
    }

    #[test]
    fn test_credential_extraction_accepts_a_well_formed_key() {
        // covers: docs/06 §3.1
        // 43 base64url characters, exactly the shape produced by gs-auth.
        let credential =
            "gsk_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let extracted =
            principal_extract_credential(&header_map(Some(&format!("Bearer {credential}"))))
                .expect("credential");
        assert_eq!(extracted, credential);
    }

    #[test]
    fn test_token_errors_map_to_the_documented_codes() {
        // covers: docs/06 §4.1, docs/07 §5
        let cases = [
            (TokenError::Unauthenticated, ErrorCode::Unauthenticated, 401),
            (TokenError::MissingScope, ErrorCode::InsufficientScope, 403),
            (
                TokenError::AccountDisabled("tenant".to_string()),
                ErrorCode::AccountDisabled,
                403,
            ),
            (
                TokenError::Validation("x".to_string()),
                ErrorCode::ValidationFailed,
                422,
            ),
            (TokenError::InvalidTtl, ErrorCode::ValidationFailed, 422),
        ];
        for (error, code, status) in cases {
            assert_eq!(error.error_code(), code);
            assert_eq!(code.http_status(), Some(status));
            let problem = token_problem(&error, "req_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a");
            assert_eq!(problem.problem.status, status);
            assert_eq!(problem.problem.code, code.as_str());
        }
    }
}
