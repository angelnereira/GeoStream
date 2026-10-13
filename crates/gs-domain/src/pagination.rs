//! Cursor pagination (docs/07 §7, FR-API-003).
//!
//! A page request carries `limit` (1..=100, default 50) and an opaque `cursor`. The cursor
//! encodes the position (`created_at`, `id`) and the fingerprint of the filters, so a cursor
//! replayed against other filters or a tampered cursor is rejected with 422 and a manipulated
//! cursor can never widen what the authorization filters make visible.
#![forbid(unsafe_code)]

use crate::error::{AsErrorCode, ErrorCode};
use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64URL;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Default page length (docs/07 §7).
pub const DEFAULT_LIMIT: u32 = 50;

/// Maximum page length (docs/07 §7).
pub const MAX_LIMIT: u32 = 100;

/// Minimum page length (docs/07 §7).
pub const MIN_LIMIT: u32 = 1;

/// A page request: `limit` plus the opaque cursor of the previous page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRequest {
    /// Requested page length.
    pub limit: u32,
    /// Opaque cursor of the previous page, when present.
    pub cursor: Option<String>,
}

/// Stable order of a page: `created_at DESC, id DESC` (docs/07 §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageCursor {
    /// `created_at` of the last item of the previous page.
    pub created_at: OffsetDateTime,
    /// `id` of the last item of the previous page.
    pub id: u128,
    /// Fingerprint of the filters the cursor was built with.
    pub filters: u64,
}

impl PageCursor {
    /// Builds a cursor for the last item of a page and the active filters.
    pub const fn new(created_at: OffsetDateTime, id: u128, filters: u64) -> Self {
        Self {
            created_at,
            id,
            filters,
        }
    }

    /// Encodes the cursor as base64url JSON (docs/07 §7).
    pub fn encode(&self) -> String {
        let payload = EncodedCursor {
            created_at: format_rfc3339(self.created_at),
            id: hex_uuid(self.id),
            filters: self.filters,
        };
        let json = serde_json::to_vec(&payload).unwrap_or_default();
        BASE64URL.encode(json)
    }

    /// Decodes a cursor, rejecting anything malformed (docs/07 §7).
    pub fn decode(value: &str) -> Result<Self, CursorError> {
        let bytes = BASE64URL
            .decode(value.as_bytes())
            .map_err(|_| CursorError::Malformed)?;
        let payload: EncodedCursor =
            serde_json::from_slice(&bytes).map_err(|_| CursorError::Malformed)?;
        let created_at = OffsetDateTime::parse(
            &payload.created_at,
            &time::format_description::well_known::Rfc3339,
        )
        .map_err(|_| CursorError::Malformed)?;
        let id = u128::from_str_radix(&payload.id, 16).map_err(|_| CursorError::Malformed)?;
        Ok(Self {
            created_at,
            id,
            filters: payload.filters,
        })
    }

    /// Checks that the cursor was built for the same filters (docs/07 §7).
    pub fn matches_filters(&self, filters: u64) -> Result<(), CursorError> {
        if self.filters == filters {
            Ok(())
        } else {
            Err(CursorError::FiltersChanged)
        }
    }

    /// Renders the id as the canonical lowercase hex used by the public ids.
    pub fn id_hex(&self) -> String {
        hex_uuid(self.id)
    }
}

/// Wire form of a cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct EncodedCursor {
    /// `created_at` in RFC 3339.
    created_at: String,
    /// Item id in hexadecimal.
    id: String,
    /// Fingerprint of the filters.
    filters: u64,
}

/// Errors of cursor handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorError {
    /// The cursor is not a valid envelope.
    Malformed,
    /// The cursor was produced for different filters.
    FiltersChanged,
}

impl fmt::Display for CursorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed => f.write_str("malformed pagination cursor"),
            Self::FiltersChanged => f.write_str("cursor does not match the request filters"),
        }
    }
}

impl std::error::Error for CursorError {}

/// Errors of validating a page request.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PageError {
    /// `limit` is outside 1..=100.
    #[error("limit must be between 1 and {MAX_LIMIT}")]
    InvalidLimit,

    /// The cursor is malformed or was built for other filters.
    #[error("invalid cursor: {0}")]
    InvalidCursor(CursorError),
}

impl AsErrorCode for PageError {
    fn error_code(&self) -> ErrorCode {
        ErrorCode::ValidationFailed
    }
}

/// A page of results: the items plus the cursor of the next page (docs/07 §7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page<T> {
    /// Items of this page.
    pub data: Vec<T>,
    /// Cursor of the next page, or `null` when the listing ends.
    pub next_cursor: Option<String>,
}

impl<T> Page<T> {
    /// Builds a page without a next cursor.
    pub fn single(data: Vec<T>) -> Self {
        Self {
            data,
            next_cursor: None,
        }
    }

    /// Builds a page with the next cursor.
    pub fn with_next(data: Vec<T>, next_cursor: impl Into<String>) -> Self {
        Self {
            data,
            next_cursor: Some(next_cursor.into()),
        }
    }

    /// Whether the listing has more pages.
    pub fn has_more(&self) -> bool {
        self.next_cursor.is_some()
    }
}

/// Fingerprint of the filters of a listing: the cursor is only valid for the same filters.
///
/// The fingerprint is a bounded 64-bit value derived from the ordered filter key/value pairs;
/// it is not a security boundary, since authorization filters are always re-applied.
pub fn filters_fingerprint(filters: &[(&str, &str)]) -> u64 {
    let mut sorted: Vec<(&str, &str)> = filters.to_vec();
    sorted.sort_unstable();
    let material = sorted
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");

    let digest = crate::invitation::hash_invitation_code(&material);
    u64::from_str_radix(&digest[..16], 16).unwrap_or(0)
}

/// Validates a page request and returns the limit to use (docs/07 §7).
pub fn validate_limit(limit: Option<u32>) -> Result<u32, PageError> {
    match limit {
        Some(limit) if (MIN_LIMIT..=MAX_LIMIT).contains(&limit) => Ok(limit),
        Some(_) => Err(PageError::InvalidLimit),
        None => Ok(DEFAULT_LIMIT),
    }
}

/// Resolves the cursor of a page request against the active filters (docs/07 §7).
pub fn resolve_cursor(
    cursor: Option<&str>,
    filters: &[(&str, &str)],
) -> Result<Option<PageCursor>, PageError> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    let decoded = PageCursor::decode(cursor).map_err(PageError::InvalidCursor)?;
    decoded
        .matches_filters(filters_fingerprint(filters))
        .map_err(PageError::InvalidCursor)?;
    Ok(Some(decoded))
}

/// Formats an instant as RFC 3339 (docs/00 §4.5).
fn format_rfc3339(instant: OffsetDateTime) -> String {
    instant
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| instant.to_string())
}

/// Renders a 128-bit id as lowercase hex without separators.
fn hex_uuid(id: u128) -> String {
    format!("{id:032x}")
}

/// Builds the next cursor of a page from its last item (docs/07 §7).
pub fn next_cursor_for(
    last: Option<(OffsetDateTime, u128)>,
    filters: &[(&str, &str)],
    has_more_items: bool,
) -> Option<String> {
    if !has_more_items {
        return None;
    }
    let (created_at, id) = last?;
    Some(PageCursor::new(created_at, id, filters_fingerprint(filters)).encode())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn instant() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_768_478_400).expect("instant")
    }

    fn cursor() -> PageCursor {
        PageCursor::new(instant(), 0x0192_b6c0_a1e4_7c3f_9d2a_7b5e_8f1c_0d3a, 42)
    }

    #[test]
    fn test_cursor_roundtrip() {
        // covers: FR-API-003, docs/07 §7
        let encoded = cursor().encode();
        let decoded = PageCursor::decode(&encoded).expect("decode");
        assert_eq!(decoded.created_at, instant());
        assert_eq!(decoded.id, 0x0192_b6c0_a1e4_7c3f_9d2a_7b5e_8f1c_0d3a);
        assert_eq!(decoded.filters, 42);
        assert_eq!(decoded.id_hex().len(), 32);
    }

    #[test]
    fn test_malformed_cursors_are_rejected() {
        // covers: FR-API-003, docs/07 §7
        let malformed: Vec<String> = vec![
            String::new(),
            "not-base64!!!".to_string(),
            BASE64URL.encode("{}"),
            BASE64URL.encode("{\"created_at\":\"yesterday\",\"id\":\"zz\",\"filters\":1}"),
            BASE64URL
                .encode("{\"created_at\":\"2026-10-12T12:00:00Z\",\"id\":\"x\",\"filters\":1}"),
        ];
        for candidate in &malformed {
            assert!(
                PageCursor::decode(candidate).is_err(),
                "cursor must be rejected: {candidate}"
            );
        }
    }

    #[test]
    fn test_cursor_of_other_filters_is_rejected() {
        // covers: FR-API-003, docs/07 §7
        let encoded = cursor().encode();
        let decoded = PageCursor::decode(&encoded).expect("decode");
        assert!(decoded.matches_filters(42).is_ok());
        assert_eq!(
            decoded.matches_filters(43),
            Err(CursorError::FiltersChanged)
        );
        assert_eq!(
            resolve_cursor(Some(&encoded), &[("tenant_id", "ten_1")]),
            Err(PageError::InvalidCursor(CursorError::FiltersChanged))
        );
    }

    #[test]
    fn test_limit_defaults_and_range() {
        // covers: docs/07 §7
        assert_eq!(validate_limit(None), Ok(DEFAULT_LIMIT));
        assert_eq!(validate_limit(Some(1)), Ok(1));
        assert_eq!(validate_limit(Some(MAX_LIMIT)), Ok(MAX_LIMIT));
        assert_eq!(validate_limit(Some(0)), Err(PageError::InvalidLimit));
        assert_eq!(
            validate_limit(Some(MAX_LIMIT + 1)),
            Err(PageError::InvalidLimit)
        );
    }

    #[test]
    fn test_page_shape_matches_docs_07_7() {
        // covers: FR-API-003, docs/07 §7
        let page: Page<serde_json::Value> = Page::with_next(vec![json!({"id": 1})], "cursor-2");
        let rendered = serde_json::to_value(&page).expect("serialize");
        assert_eq!(rendered["next_cursor"], json!("cursor-2"));
        assert!(page.has_more());

        let last: Page<serde_json::Value> = Page::single(vec![json!({"id": 2})]);
        assert!(!last.has_more());
        assert_eq!(
            serde_json::to_value(&last).expect("serialize")["next_cursor"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn test_walking_a_listing_yields_every_item_once() {
        // covers: FR-API-003, docs/07 §7 acceptance criterion
        // 120 items, limit=50 → 3 pages without duplicates or omissions.
        let filters = [("tenant_id", "ten_1")];
        let mut items: Vec<(OffsetDateTime, u128)> = (0..120)
            .map(|index| (instant() - time::Duration::seconds(index), index as u128))
            .collect();
        items.sort_by(|left, right| right.0.cmp(&left.0).then(right.1.cmp(&left.1)));

        let mut seen = Vec::new();
        let mut cursor: Option<String> = None;
        let mut pages = 0;
        loop {
            let limit = validate_limit(Some(50)).expect("limit");
            let decoded = resolve_cursor(cursor.as_deref(), &filters).expect("cursor");
            // The cursor points at the last item of the previous page: continue after it.
            let start = decoded
                .map(|value| {
                    items
                        .iter()
                        .position(|item| item == &(value.created_at, value.id))
                        .expect("cursor position")
                        + 1
                })
                .unwrap_or(0);
            let page: Vec<&(OffsetDateTime, u128)> =
                items.iter().skip(start).take(limit as usize).collect();
            seen.extend(page.iter().map(|item| item.1));
            pages += 1;

            let last = page.last().copied();
            let has_more_items = start + page.len() < items.len();
            cursor = next_cursor_for(last.copied(), &filters, has_more_items);
            if !has_more_items {
                break;
            }
            assert!(
                pages < 10,
                "a listing of 120 items with limit 50 must not take more pages"
            );
        }

        assert_eq!(pages, 3);
        assert_eq!(seen.len(), 120);
        let mut unique = seen.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), 120, "no duplicated items may appear");
        assert_eq!(unique.first(), Some(&0));
        assert_eq!(unique.last(), Some(&119));
    }

    #[test]
    fn test_filters_fingerprint_ignores_the_order() {
        // covers: docs/07 §7
        assert_eq!(
            filters_fingerprint(&[("a", "1"), ("b", "2")]),
            filters_fingerprint(&[("b", "2"), ("a", "1")])
        );
        assert_ne!(
            filters_fingerprint(&[("a", "1"), ("b", "2")]),
            filters_fingerprint(&[("a", "1"), ("b", "3")])
        );
    }
}
