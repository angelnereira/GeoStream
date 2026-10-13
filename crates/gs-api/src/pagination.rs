//! Query parameters of list operations (docs/07 §7, FR-API-003).
//!
//! `limit` and `cursor` come from the query string; the cursor is opaque and is validated
//! against the filters of the request, and `limit` must be inside 1..=100.
#![forbid(unsafe_code)]

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use gs_service::gs_domain::pagination::{
    PageError, PageRequest, validate_limit as validate_domain_limit,
};

/// Query parameters of a list operation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct PaginationQuery {
    /// Requested page length (default 50, maximum 100).
    pub limit: Option<u32>,
    /// Opaque cursor of the previous page.
    pub cursor: Option<String>,
}

/// Errors produced while parsing the pagination parameters.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PaginationQueryError {
    /// The same parameter appears twice.
    #[error("pagination parameter '{0}' must appear at most once")]
    Duplicate(String),

    /// A parameter is not a number or is out of range.
    #[error("invalid pagination parameter: {0}")]
    Invalid(String),
}

impl IntoResponse for PaginationQueryError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "status": 422,
            "code": "VALIDATION_FAILED",
            "detail": self.to_string(),
        });
        (StatusCode::UNPROCESSABLE_ENTITY, axum::Json(body)).into_response()
    }
}

/// Parses `limit` and `cursor` from the raw query string (docs/07 §7).
///
/// A repeated parameter is rejected rather than silently taking the last value, so a client
/// cannot smuggle a second cursor.
pub fn parse_pagination_query(
    raw_query: Option<&str>,
) -> Result<PageRequest, PaginationQueryError> {
    let mut limit: Option<u32> = None;
    let mut cursor: Option<String> = None;

    for pair in raw_query.unwrap_or_default().split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = match pair.split_once('=') {
            Some((key, value)) => (key, value),
            None => (pair, ""),
        };
        match key {
            "limit" => {
                if limit.is_some() {
                    return Err(PaginationQueryError::Duplicate("limit".to_string()));
                }
                let parsed: u32 = percent_decode(value)
                    .parse()
                    .map_err(|_| PaginationQueryError::Invalid("limit".to_string()))?;
                limit = Some(parsed);
            }
            "cursor" => {
                if cursor.is_some() {
                    return Err(PaginationQueryError::Duplicate("cursor".to_string()));
                }
                cursor = Some(percent_decode(value));
            }
            _ => {}
        }
    }

    let limit = validate_domain_limit(limit).map_err(|error| match error {
        PageError::InvalidLimit => PaginationQueryError::Invalid("limit".to_string()),
        PageError::InvalidCursor(_) => PaginationQueryError::Invalid("cursor".to_string()),
    })?;

    Ok(PageRequest { limit, cursor })
}

/// Rejects malformed cursors and cursors built for other filters (docs/07 §7).
pub fn validate_cursor(cursor: Option<&str>, filters: &[(&str, &str)]) -> Result<(), PageError> {
    gs_service::gs_domain::pagination::resolve_cursor(cursor, filters).map(|_| ())
}

/// Parses the pagination parameters of a request whose raw query is `raw_query`.
pub fn pagination_from_query(raw_query: Option<&str>) -> Result<PageRequest, PaginationQueryError> {
    parse_pagination_query(raw_query)
}

/// Silently drops percent-encoding; cursors are base64url, so no `+` ambiguity exists.
fn percent_decode(value: &str) -> String {
    if !value.contains('%') {
        return value.to_string();
    }
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults_and_explicit_parameters() {
        // covers: FR-API-003, docs/07 §7
        let parsed = parse_pagination_query(None).expect("empty query");
        assert_eq!(parsed.limit, 50);
        assert!(parsed.cursor.is_none());

        let parsed = parse_pagination_query(Some("limit=10&cursor=abc")).expect("query");
        assert_eq!(parsed.limit, 10);
        assert_eq!(parsed.cursor.as_deref(), Some("abc"));
    }

    #[test]
    fn test_repeated_and_invalid_parameters_are_rejected() {
        // covers: docs/07 §7
        assert_eq!(
            parse_pagination_query(Some("limit=1&limit=2")),
            Err(PaginationQueryError::Duplicate("limit".to_string()))
        );
        assert_eq!(
            parse_pagination_query(Some("cursor=a&cursor=b")),
            Err(PaginationQueryError::Duplicate("cursor".to_string()))
        );
        assert_eq!(
            parse_pagination_query(Some("limit=zero")),
            Err(PaginationQueryError::Invalid("limit".to_string()))
        );
        assert_eq!(
            parse_pagination_query(Some("limit=101")),
            Err(PaginationQueryError::Invalid("limit".to_string()))
        );
        assert_eq!(
            parse_pagination_query(Some("limit=0")),
            Err(PaginationQueryError::Invalid("limit".to_string()))
        );
    }

    #[test]
    fn test_unknown_parameters_are_ignored() {
        // covers: docs/07 §7
        let parsed = parse_pagination_query(Some("order=asc&limit=5")).expect("query");
        assert_eq!(parsed.limit, 5);
    }

    #[test]
    fn test_cursor_is_validated_against_the_filters() {
        // covers: docs/07 §7
        let cursor = gs_service::gs_domain::PageCursor::new(
            time::OffsetDateTime::from_unix_timestamp(1_768_478_400).expect("instant"),
            0x0192_b6c0_a1e4_7c3f_9d2a_7b5e_8f1c_0d3a,
            gs_service::gs_domain::filters_fingerprint(&[("tenant_id", "ten_1")]),
        )
        .encode();

        assert!(
            validate_cursor(Some(&cursor), &[("tenant_id", "ten_1")]).is_ok(),
            "a cursor built for the same filters must be accepted"
        );
        assert!(matches!(
            validate_cursor(Some(&cursor), &[("tenant_id", "ten_2")]),
            Err(PageError::InvalidCursor(_))
        ));
        assert!(matches!(
            validate_cursor(Some("malformed!!!"), &[("tenant_id", "ten_1")]),
            Err(PageError::InvalidCursor(_))
        ));
        assert_eq!(validate_cursor(None, &[("tenant_id", "ten_1")]), Ok(()));
    }

    #[tokio::test]
    async fn test_axum_extractor_parses_the_query_string() {
        // covers: docs/07 §7
        let parsed = pagination_from_query(Some("limit=7&cursor=x")).expect("query");
        assert_eq!(parsed.limit, 7);
        assert_eq!(parsed.cursor.as_deref(), Some("x"));
    }
}
