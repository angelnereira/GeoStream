//! WebSocket upgrade: the pre-upgrade checks of docs/08 §3.
//!
//! The route lives here, next to the REST router, because `gs-api` owns Problem Details
//! (docs/07 §5) and the upgrade rejections must be `application/problem+json`
//! (docs/08 §3). The connection itself belongs to `gs-stream`, which the dependency rules of
//! docs/04 §4.1 keep out of this crate: the binary wires the accepted socket to
//! [`SessionRunner`].
//!
//! Order of the checks: subprotocol, upgrade rate limit, connection limit, ticket and
//! finally the state of the tenant and application. A rejected attempt never consumes a
//! ticket, and no rejection reveals whether the ticket existed (docs/09 §6).
#![forbid(unsafe_code)]

use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::extract::FromRequestParts;
use axum::extract::ws::{WebSocket, WebSocketUpgrade};
use axum::extract::{RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use gs_service::gs_domain::Subprotocol;
use gs_service::gs_domain::error::{AsErrorCode, ErrorCode};
use gs_service::gs_domain::negotiate_subprotocol;
use gs_service::tickets::{
    TicketError, TicketGrant, redeem_stream_ticket, verify_ticket_principal_state,
};
use gs_service::{InMemoryEphemeralStore, migrate::PgPool};
use time::OffsetDateTime;

use crate::problem::{Problem, ProblemResponse};
use crate::rate_limit::{
    RateLimitExceeded, check_rate_limit, extract_client_ip, rate_limit_problem_response,
};
use crate::router::REQUEST_ID_HEADER;
use crate::security::request_id_from;

/// Query parameter that carries the ticket (docs/08 §2).
pub const TICKET_PARAM: &str = "ticket";

/// Metric of rejected upgrades (docs/16 §4.2).
const METRIC_UPGRADE_REJECTED_TOTAL: &str = "gs_stream_upgrade_rejected_total";

/// Session accepted by the upgrade, handed to the runner (docs/08 §3).
pub struct AcceptedWebSocket {
    /// Negotiated subprotocol.
    pub subprotocol: Subprotocol,
    /// Grant of the redeemed ticket (docs/06 §5).
    pub grant: TicketGrant,
    /// Correlation id of the upgrade request.
    pub request_id: String,
    /// Upgraded socket.
    pub socket: WebSocket,
}

/// Future returned by a [`SessionRunner`].
pub type SessionFuture = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

/// Runs an accepted WebSocket session.
///
/// Provided by the binary (`gs-stream`) so that `gs-api` does not depend on it
/// (docs/04 §4.1).
pub type SessionRunner = Arc<dyn Fn(AcceptedWebSocket) -> SessionFuture + Send + Sync>;

/// Live connection accounting, used for `GS_STREAM_MAX_CONNECTIONS` (docs/08 §3).
#[derive(Debug, Clone, Default)]
pub struct ConnectionCounter {
    active: Arc<AtomicUsize>,
}

impl ConnectionCounter {
    /// Builds an empty counter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of connections currently open on this instance.
    pub fn active(&self) -> usize {
        self.active.load(Ordering::Relaxed)
    }

    /// Tries to take one slot, returning false when the instance is at its limit.
    pub fn try_acquire(&self, max: u32) -> bool {
        let mut current = self.active.load(Ordering::Relaxed);
        loop {
            if current >= max as usize {
                return false;
            }
            match self.active.compare_exchange_weak(
                current,
                current + 1,
                Ordering::SeqCst,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(observed) => current = observed,
            }
        }
    }

    /// Releases one slot.
    pub fn release(&self) {
        let mut current = self.active.load(Ordering::Relaxed);
        while current > 0 {
            match self.active.compare_exchange_weak(
                current,
                current - 1,
                Ordering::SeqCst,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }
}

/// State of the upgrade route (docs/08 §3).
#[derive(Clone)]
pub struct StreamApiState {
    /// PostgreSQL pool, for the state checks of docs/06 §7.
    pub pool: PgPool,
    /// Ephemeral store that holds the tickets (docs/06 §5).
    pub ephemeral: Arc<InMemoryEphemeralStore>,
    /// Ephemeral key prefix (`GS_REDIS_KEY_PREFIX`, default `gs`).
    pub redis_key_prefix: String,
    /// Upgrades per minute per IP (`GS_STREAM_UPGRADE_RPM_PER_IP`).
    pub upgrade_rpm_per_ip: u32,
    /// Connections per instance (`GS_STREAM_MAX_CONNECTIONS`).
    pub max_connections: u32,
    /// Largest inbound frame (`GS_STREAM_MAX_FRAME_BYTES`).
    pub max_frame_bytes: usize,
    /// Live connection counter of this instance.
    pub connections: ConnectionCounter,
    /// Whether `gs.v1.json` is offered (`GS_STREAM_JSON_ENABLED`).
    pub json_subprotocol_enabled: bool,
    /// Runs the accepted session (`gs-stream`, wired by the binary).
    pub session_runner: SessionRunner,
}

impl std::fmt::Debug for StreamApiState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamApiState")
            .field("pool", &"<pg pool>")
            .field("redis_key_prefix", &self.redis_key_prefix)
            .field("upgrade_rpm_per_ip", &self.upgrade_rpm_per_ip)
            .field("max_connections", &self.max_connections)
            .field("max_frame_bytes", &self.max_frame_bytes)
            .field("connections", &self.connections.active())
            .field("json_subprotocol_enabled", &self.json_subprotocol_enabled)
            .finish()
    }
}

/// The WebSocket upgrade of a request, or `None` when it is not one.
///
/// Axum does not expose the rejection of [`WebSocketUpgrade`], so the extraction is wrapped
/// to answer with Problem Details instead of the default plain text error (docs/08 §3).
pub struct MaybeUpgrade(pub Option<WebSocketUpgrade>);

impl<S> FromRequestParts<S> for MaybeUpgrade
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        Ok(Self(
            WebSocketUpgrade::from_request_parts(parts, state)
                .await
                .ok(),
        ))
    }
}

/// Builds the `/v1/stream` route (docs/08 §2–§3).
pub fn stream_route(state: StreamApiState) -> axum::Router {
    axum::Router::new()
        .route("/v1/stream", axum::routing::get(stream_upgrade_handler))
        .with_state(state)
}

/// `GET /v1/stream` (OpenAPI `openStream`).
pub async fn stream_upgrade_handler(
    State(state): State<StreamApiState>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
    upgrade: MaybeUpgrade,
) -> Response {
    let request_id = request_id_from(&headers);

    let MaybeUpgrade(upgrade) = upgrade;
    let Some(upgrade) = upgrade else {
        // A request that is not a WebSocket upgrade never reaches the ticket check
        // (docs/08 §3: 400 BadRequest).
        return reject(
            &request_id,
            400,
            ErrorCode::ValidationFailed,
            "websocket upgrade required",
        );
    };

    // 1. Subprotocol: the first offered one the server supports (docs/08 §2).
    let offered = headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok());
    let Some(subprotocol) = negotiate_subprotocol(offered, state.json_subprotocol_enabled) else {
        return reject(
            &request_id,
            400,
            ErrorCode::ProtocolVersionUnsupported,
            "no supported subprotocol offered",
        );
    };

    // 2. Upgrade rate limit per IP (docs/17 §2.6, `GS_STREAM_UPGRADE_RPM_PER_IP`).
    let client_ip = extract_client_ip(&headers);
    let ip = IpAddr::from_str(&client_ip).unwrap_or(IpAddr::from([0, 0, 0, 0]));
    if let Err(exceeded) = check_upgrade_rate_limit(&state, ip).await {
        return rate_limited(&request_id, exceeded);
    }

    // 3. Connection limit of the instance (docs/08 §3).
    if !state.connections.try_acquire(state.max_connections) {
        let mut response = reject(
            &request_id,
            503,
            ErrorCode::TemporarilyUnavailable,
            "instance is at its connection limit",
        );
        if let Ok(value) = HeaderValue::from_str("1") {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        return response;
    }

    // 4. Ticket: present, well formed and redeemable exactly once (docs/06 §5).
    let Some(ticket) = query_param(&query, TICKET_PARAM) else {
        state.connections.release();
        return reject(
            &request_id,
            401,
            ErrorCode::Unauthenticated,
            "missing stream ticket",
        );
    };
    let grant = match redeem_stream_ticket(
        state.ephemeral.as_ref(),
        &state.redis_key_prefix,
        &ticket,
        OffsetDateTime::now_utc(),
    )
    .await
    {
        Ok(grant) => grant,
        Err(TicketError::Unauthenticated) => {
            state.connections.release();
            return reject(
                &request_id,
                401,
                ErrorCode::Unauthenticated,
                "invalid or expired stream ticket",
            );
        }
        Err(TicketError::TemporarilyUnavailable) => {
            state.connections.release();
            return reject(
                &request_id,
                503,
                ErrorCode::TemporarilyUnavailable,
                "stream ticket store unavailable",
            );
        }
        Err(error) => {
            state.connections.release();
            return reject(
                &request_id,
                500,
                error.error_code(),
                "stream ticket rejected",
            );
        }
    };

    // 5. Tenant and application state at upgrade time (docs/08 §3, docs/06 §7).
    if let Err(error) = verify_ticket_principal_state(&state.pool, &grant).await {
        state.connections.release();
        let status = error.error_code().http_status().unwrap_or(500);
        return reject(
            &request_id,
            status,
            error.error_code(),
            "stream principal is disabled",
        );
    }

    // Accepted: answer with the negotiated subprotocol and hand the socket to the runner.
    let slots = state.connections.clone();
    let frame_limit = state.max_frame_bytes;
    let runner = Arc::clone(&state.session_runner);
    let accepted_request_id = request_id.clone();
    let upgrade = upgrade
        .max_frame_size(frame_limit)
        .on_upgrade(move |socket| {
            let slots = slots.clone();
            async move {
                runner(AcceptedWebSocket {
                    subprotocol,
                    grant,
                    request_id: accepted_request_id,
                    socket,
                })
                .await;
                slots.release();
            }
        });

    let mut response = upgrade.into_response();
    if let Ok(value) = HeaderValue::from_str(subprotocol.as_str()) {
        response
            .headers_mut()
            .insert(header::SEC_WEBSOCKET_PROTOCOL, value);
    }
    add_request_id(&mut response, &request_id);
    response
}

/// Applies the upgrade rate limit (docs/07 §8, docs/17 §2.6).
async fn check_upgrade_rate_limit(
    state: &StreamApiState,
    ip: IpAddr,
) -> Result<(), RateLimitExceeded> {
    check_rate_limit(
        state.ephemeral.as_ref(),
        &state.redis_key_prefix,
        "upgrade_ip",
        &ip.to_string(),
        state.upgrade_rpm_per_ip,
        OffsetDateTime::now_utc(),
    )
    .await
    .map(|_info| ())
}

/// Reads one query parameter, percent-decoding its value.
fn query_param(query: &Option<String>, name: &str) -> Option<String> {
    let query = query.as_deref()?;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if key == name {
            return Some(percent_decode(value));
        }
    }
    None
}

/// Decodes `%XX` escapes of a query value.
fn percent_decode(value: &str) -> String {
    if !value.contains('%') {
        return value.to_string();
    }
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or_default();
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

/// Renders a rejection as Problem Details and records the metric (docs/08 §3, docs/16 §4.2).
fn reject(request_id: &str, status: u16, code: ErrorCode, detail: &str) -> Response {
    record_rejection(status);
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = (
        status,
        ProblemResponse::new(Problem::new(code, request_id).with_detail(detail)),
    )
        .into_response();
    add_request_id(&mut response, request_id);
    response
}

/// Renders a 429 with `Retry-After` (docs/07 §8, docs/08 §3).
fn rate_limited(request_id: &str, exceeded: RateLimitExceeded) -> Response {
    record_rejection(429);
    let mut response = rate_limit_problem_response(
        exceeded.retry_after,
        exceeded.limit,
        "upgrade_ip",
        request_id,
    );
    add_request_id(&mut response, request_id);
    response
}

/// Increments `gs_stream_upgrade_rejected_total` (docs/16 §4.2).
fn record_rejection(status: u16) {
    metrics::counter!(METRIC_UPGRADE_REJECTED_TOTAL, "reason" => status.to_string()).increment(1);
}

/// Adds the correlation id to a response (docs/07 §1).
fn add_request_id(response: &mut Response, request_id: &str) {
    if let Ok(value) = HeaderValue::from_str(request_id) {
        response.headers_mut().insert(REQUEST_ID_HEADER, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_counter_is_bounded() {
        // covers: docs/08 §3 (GS_STREAM_MAX_CONNECTIONS)
        let counter = ConnectionCounter::new();
        assert!(counter.try_acquire(2));
        assert!(counter.try_acquire(2));
        assert!(
            !counter.try_acquire(2),
            "the third connection must be refused"
        );
        assert_eq!(counter.active(), 2);
        counter.release();
        assert!(counter.try_acquire(2));
        assert_eq!(counter.active(), 2);
    }

    #[test]
    fn test_release_never_underflows() {
        // covers: docs/08 §3
        let counter = ConnectionCounter::new();
        counter.release();
        assert_eq!(counter.active(), 0);
        assert!(counter.try_acquire(1));
    }

    #[test]
    fn test_query_param_reads_the_ticket() {
        // covers: docs/08 §2
        assert_eq!(
            query_param(&Some("ticket=gst_abc".to_string()), TICKET_PARAM),
            Some("gst_abc".to_string())
        );
        assert_eq!(
            query_param(
                &Some("other=1&ticket=gst_abc%2Fdef".to_string()),
                TICKET_PARAM
            ),
            Some("gst_abc/def".to_string())
        );
        assert_eq!(
            query_param(&Some("other=1".to_string()), TICKET_PARAM),
            None
        );
        assert_eq!(query_param(&None, TICKET_PARAM), None);
    }

    #[test]
    fn test_rejections_render_problem_details() {
        // covers: docs/08 §3, docs/07 §5
        let request_id = "req_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a";
        let response = reject(
            request_id,
            401,
            ErrorCode::Unauthenticated,
            "invalid or expired stream ticket",
        );
        assert_eq!(response.status(), 401);
        assert_eq!(
            response
                .headers()
                .get("x-request-id")
                .and_then(|value| value.to_str().ok()),
            Some(request_id)
        );
    }
}
