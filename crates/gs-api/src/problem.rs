//! Problem Details with the fixed titles of docs/07 §5 (FR-API-001).
//!
//! `type` is `https://geostream.dev/problems/` + the lowercased `code`. `detail` is optional
//! and must never carry sensitive data. `errors` only appears on `VALIDATION_FAILED`.
#![forbid(unsafe_code)]

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use gs_service::gs_domain::error::ErrorCode;

/// URI prefix of every problem type (docs/07 §5).
pub const PROBLEM_TYPE_PREFIX: &str = "https://geostream.dev/problems/";

/// Maximum length of `detail` (docs/07 §5).
pub const DETAIL_MAX_LEN: usize = 200;

/// Field level error of a `VALIDATION_FAILED` problem (docs/09 §2.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldError {
    /// Field path, bounded to the paths of docs/09 §3.1.
    pub field: String,
    /// Machine readable reason.
    pub reason: String,
}

/// Problem Details body (docs/07 §5, RFC 9457 shape).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    /// Problem type URI.
    #[serde(rename = "type")]
    pub problem_type: String,
    /// Fixed title of the `code`.
    pub title: String,
    /// HTTP status.
    pub status: u16,
    /// Machine readable error code of docs/09 §6.
    pub code: String,
    /// Optional human readable detail, never with sensitive data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Correlation id echoed from `X-Request-Id`.
    pub request_id: String,
    /// Field errors, only on `VALIDATION_FAILED`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<FieldError>,
}

impl Problem {
    /// Builds a problem from an error code, a status and the request id.
    pub fn new(code: ErrorCode, request_id: impl Into<String>) -> Self {
        let status = code.http_status().unwrap_or(500);
        Self {
            problem_type: format!("{}{}", PROBLEM_TYPE_PREFIX, code.as_str().to_lowercase()),
            title: title_for(code).to_string(),
            status,
            code: code.as_str().to_string(),
            detail: None,
            request_id: request_id.into(),
            errors: Vec::new(),
        }
    }

    /// Adds a safe detail, truncating anything above the documented limit.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        let mut detail = detail.into();
        if detail.len() > DETAIL_MAX_LEN {
            detail.truncate(DETAIL_MAX_LEN);
        }
        self.detail = Some(detail);
        self
    }

    /// Adds field errors (only valid for `VALIDATION_FAILED`).
    pub fn with_errors(mut self, errors: Vec<FieldError>) -> Self {
        self.errors = errors;
        self
    }
}

/// Response wrapper that renders `application/problem+json` with `X-Request-Id`.
#[derive(Debug, Clone)]
pub struct ProblemResponse {
    /// Problem body.
    pub problem: Problem,
}

impl ProblemResponse {
    /// Wraps a problem.
    pub const fn new(problem: Problem) -> Self {
        Self { problem }
    }
}

impl IntoResponse for ProblemResponse {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.problem.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut response = (status, Json(self.problem.clone())).into_response();
        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/problem+json"),
        );
        response
    }
}

impl From<Problem> for ProblemResponse {
    fn from(problem: Problem) -> Self {
        Self::new(problem)
    }
}

/// Fixed English titles of every error code (docs/07 §5).
#[derive(Debug, Clone, Copy)]
pub struct TitleCatalog;

impl TitleCatalog {
    /// Title of a code.
    pub const fn title(code: ErrorCode) -> &'static str {
        title_for(code)
    }
}

/// Title of an error code (docs/07 §5).
pub const fn title_for(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::Unauthenticated => "Unauthenticated",
        ErrorCode::TokenExpired => "Token expired",
        ErrorCode::PermissionDenied => "Permission denied",
        ErrorCode::InsufficientScope => "Insufficient scope",
        ErrorCode::AccountDisabled => "Account disabled",
        ErrorCode::NotFound => "Not found",
        ErrorCode::Conflict => "Conflict",
        ErrorCode::ValidationFailed => "Validation failed",
        ErrorCode::InvalidLocation => "Invalid location",
        ErrorCode::RateLimited => "Rate limit exceeded",
        ErrorCode::PayloadTooLarge => "Payload too large",
        ErrorCode::SessionClosed => "Session closed",
        ErrorCode::SessionExpired => "Session expired",
        ErrorCode::SessionFull => "Session is full",
        ErrorCode::MemberRevoked => "Member revoked",
        ErrorCode::NotSharing => "Not sharing",
        ErrorCode::NotPublisher => "Not publisher",
        ErrorCode::InvitationInvalid => "Invalid invitation",
        ErrorCode::IdempotencyKeyReused => "Idempotency key reused",
        ErrorCode::PreconditionFailed => "Precondition failed",
        ErrorCode::ProtocolVersionUnsupported => "Protocol version not supported",
        ErrorCode::ProtocolError => "Protocol error",
        ErrorCode::TooManySubscriptions => "Too many subscriptions",
        ErrorCode::TemporarilyUnavailable => "Temporarily unavailable",
        ErrorCode::Internal => "Internal error",
        ErrorCode::FeatureDisabled => "Feature disabled",
        ErrorCode::ConsentRequired => "Consent required",
        ErrorCode::ProviderUnavailable => "Provider unavailable",
    }
}

/// Builds the problem type URI of a code.
pub fn problem_type_of(code: ErrorCode) -> String {
    format!("{}{}", PROBLEM_TYPE_PREFIX, code.as_str().to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn problem_of(code: ErrorCode) -> Problem {
        Problem::new(code, "req_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a")
    }

    #[test]
    fn test_type_and_title_follow_docs_07_5() {
        // covers: FR-API-001, docs/07 §5
        let problem = problem_of(ErrorCode::SessionFull);
        assert_eq!(
            problem.problem_type,
            "https://geostream.dev/problems/session_full"
        );
        assert_eq!(problem.title, "Session is full");
        assert_eq!(problem.code, "SESSION_FULL");
        assert_eq!(problem.status, 409);

        // NOT_SHARING is a 409 in the catalog of docs/09 §6, not a 403.
        let problem = problem_of(ErrorCode::NotSharing);
        assert_eq!(
            problem.problem_type,
            "https://geostream.dev/problems/not_sharing"
        );
        assert_eq!(problem.title, "Not sharing");
        assert_eq!(problem.status, 409);

        let problem = problem_of(ErrorCode::AccountDisabled);
        assert_eq!(
            problem.problem_type,
            "https://geostream.dev/problems/account_disabled"
        );
        assert_eq!(problem.status, 403);
    }

    #[test]
    fn test_status_codes_match_the_catalog() {
        // covers: FR-API-001, docs/09 §6
        assert_eq!(problem_of(ErrorCode::Unauthenticated).status, 401);
        assert_eq!(problem_of(ErrorCode::TokenExpired).status, 401);
        assert_eq!(problem_of(ErrorCode::InsufficientScope).status, 403);
        assert_eq!(problem_of(ErrorCode::AccountDisabled).status, 403);
        assert_eq!(problem_of(ErrorCode::ValidationFailed).status, 422);
        assert_eq!(problem_of(ErrorCode::RateLimited).status, 429);
        assert_eq!(problem_of(ErrorCode::PayloadTooLarge).status, 413);
        assert_eq!(problem_of(ErrorCode::Internal).status, 500);
    }

    #[test]
    fn test_detail_is_bounded_and_errors_are_optional() {
        // covers: FR-API-001, docs/07 §5
        let problem = problem_of(ErrorCode::ValidationFailed)
            .with_detail("x".repeat(400))
            .with_errors(vec![FieldError {
                field: "position.latitude".to_string(),
                reason: "out of range".to_string(),
            }]);
        let detail = problem.detail.clone().unwrap_or_default();
        assert_eq!(detail.len(), DETAIL_MAX_LEN);
        assert_eq!(problem.errors.len(), 1);

        // Without errors the field is omitted rather than serialized as an empty array.
        let plain = problem_of(ErrorCode::NotFound);
        let json = serde_json::to_value(&plain).expect("serialize");
        assert!(json.get("errors").is_none());
        assert!(json.get("detail").is_none());
        assert_eq!(
            json["request_id"],
            json!("req_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a")
        );
    }

    #[tokio::test]
    async fn test_problem_response_uses_the_problem_content_type() {
        // covers: FR-API-001, docs/07 §1
        use axum::body::to_bytes;

        let response = ProblemResponse::new(problem_of(ErrorCode::NotFound)).into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/problem+json")
        );

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(json["code"], json!("NOT_FOUND"));
    }
}
