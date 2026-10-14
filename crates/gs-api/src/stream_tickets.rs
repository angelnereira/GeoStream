//! `POST /v1/stream/tickets` (docs/06 §5, docs/07 §9.1, FR-AUTH-006).
//!
//! The endpoint exchanges an application key or a device token for a single-use ticket
//! that authenticates one WebSocket upgrade (docs/08 §3). Both principals need the
//! `stream:connect` scope. The response is `201` with `{ ticket, expires_at, stream_url }`,
//! carries `Cache-Control: no-store` and never logs the ticket (docs/10 §8.2).
#![forbid(unsafe_code)]

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use time::OffsetDateTime;

use gs_service::gs_domain::error::{AsErrorCode, ErrorCode};
use gs_service::tickets::{
    STREAM_CONNECT_SCOPE, TicketCaller, TicketConfig, TicketError, issue_stream_ticket,
};

use crate::problem::{Problem, ProblemResponse};
use crate::router::REQUEST_ID_HEADER;
use crate::security::request_id_from;
use crate::sessions::{CallerPrincipal, SessionApiState, authenticate_caller};

/// Response of `POST /v1/stream/tickets` (OpenAPI `StreamTicketResponse`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StreamTicketResponse {
    /// The single-use ticket, `gst_…` (docs/06 §5).
    pub ticket: String,
    /// Instant at which the ticket stops being redeemable, RFC 3339.
    pub expires_at: String,
    /// WebSocket URL to connect to, without the query string (docs/06 §5).
    pub stream_url: String,
}

/// Builds the WebSocket URL the client connects to (docs/06 §5, docs/08 §2).
///
/// `https://` maps to `wss://` and `http://` to `ws://`, which `GS_ENV=dev` allows
/// (docs/17 §3). The query string (`?ticket=…`) is appended by the client.
pub fn stream_url_of(public_base_url: &str) -> String {
    let base = public_base_url.trim_end_matches('/');
    let ws_base = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        base.to_string()
    };
    format!("{ws_base}/v1/stream")
}

/// Builds the routes of the stream credentials (docs/07 §9.1).
pub fn stream_tickets_routes() -> axum::Router<SessionApiState> {
    axum::Router::new().route(
        "/v1/stream/tickets",
        axum::routing::post(create_stream_ticket_handler),
    )
}

/// `POST /v1/stream/tickets` (OpenAPI `createStreamTicket`).
pub async fn create_stream_ticket_handler(
    State(state): State<SessionApiState>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state, &headers, &request_id).await {
        Ok(caller) => caller,
        Err(response) => return *response,
    };

    // `x-gs-auth`: application `stream:connect`, device `stream:connect` (docs/06 §8.2).
    if !caller_has_scope(&caller, STREAM_CONNECT_SCOPE) {
        let problem = Problem::new(ErrorCode::InsufficientScope, &request_id)
            .with_detail("principal requires the stream:connect scope");
        return with_standard_headers(
            (StatusCode::FORBIDDEN, ProblemResponse::new(problem)).into_response(),
            &request_id,
        );
    }

    let ticket_caller = match &caller {
        CallerPrincipal::Application(principal) => TicketCaller::Application(principal),
        CallerPrincipal::Device(principal) => TicketCaller::Device(principal),
    };
    let config = TicketConfig {
        ttl: state.stream_ticket.ttl,
        app_max_connection_age: state.stream_ticket.app_max_connection_age,
    };

    let now = OffsetDateTime::now_utc();
    match issue_stream_ticket(
        state.ephemeral.as_ref(),
        &state.redis_key_prefix,
        &ticket_caller,
        &config,
        now,
    )
    .await
    {
        Ok(issued) => {
            let body = StreamTicketResponse {
                ticket: issued.ticket.as_str().to_string(),
                expires_at: format_rfc3339(issued.expires_at),
                stream_url: stream_url_of(&state.stream_ticket.public_base_url),
            };
            let response = (StatusCode::CREATED, Json(body)).into_response();
            with_standard_headers(response, &request_id)
        }
        Err(error) => ticket_problem(&error, &request_id),
    }
}

/// Returns true when the authenticated principal carries `scope` (docs/06 §8.1 step 2).
fn caller_has_scope(caller: &CallerPrincipal, scope: &str) -> bool {
    match caller {
        CallerPrincipal::Application(principal) => {
            principal.api_key.scopes.iter().any(|s| s == scope)
        }
        CallerPrincipal::Device(principal) => principal.claims.has_scope(scope),
    }
}

/// Attaches `Cache-Control: no-store` and the correlation id (docs/07 §1).
fn with_standard_headers(mut response: Response, request_id: &str) -> Response {
    if let Ok(value) = axum::http::HeaderValue::from_str(request_id) {
        response.headers_mut().insert(REQUEST_ID_HEADER, value);
    }
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

/// Renders a ticket error as Problem Details (docs/07 §5).
fn ticket_problem(error: &TicketError, request_id: &str) -> Response {
    let status = StatusCode::from_u16(error.error_code().http_status().unwrap_or(500))
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let problem = Problem::new(error.error_code(), request_id).with_detail(error.to_string());
    let response = (status, ProblemResponse::new(problem)).into_response();
    with_standard_headers(response, request_id)
}

/// Formats an instant as RFC 3339 (docs/00 §4.5).
fn format_rfc3339(instant: OffsetDateTime) -> String {
    instant
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_url_maps_https_to_wss() {
        // covers: docs/06 §5, docs/08 §2
        assert_eq!(
            stream_url_of("https://geostream.example.com"),
            "wss://geostream.example.com/v1/stream"
        );
        assert_eq!(
            stream_url_of("https://geostream.example.com/"),
            "wss://geostream.example.com/v1/stream"
        );
    }

    #[test]
    fn test_stream_url_maps_http_to_ws() {
        // covers: docs/17 §3 (`ws://` solo en dev)
        assert_eq!(
            stream_url_of("http://localhost:8080"),
            "ws://localhost:8080/v1/stream"
        );
    }

    #[test]
    fn test_stream_url_keeps_unknown_schemes() {
        // covers: docs/06 §5
        assert_eq!(
            stream_url_of("geostream.example.com"),
            "geostream.example.com/v1/stream"
        );
    }

    #[test]
    fn test_ticket_errors_map_to_http_status() {
        // covers: docs/08 §3, docs/09 §6
        let request_id = "req_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a";
        let cases = [
            (TicketError::Unauthenticated, 401),
            (TicketError::AccountDisabled("tenant".to_string()), 403),
            (TicketError::TemporarilyUnavailable, 503),
            (TicketError::Internal("grant".to_string()), 500),
        ];
        for (error, status) in cases {
            let response = ticket_problem(&error, request_id);
            assert_eq!(response.status(), status);
        }
    }
}
