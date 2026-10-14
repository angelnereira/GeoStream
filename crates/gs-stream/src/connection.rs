//! Connection state machine: hello handshake, heartbeat, inactivity and close codes
//! (docs/08 §5–§6.5 and §8, FR-STR-001, FR-STR-005).
//!
//! ```text
//! AWAITING_HELLO --ClientHello válido--> READY
//! AWAITING_HELLO --timeout--> CLOSED (4002)
//! READY --sin tramas durante idle_timeout--> CLOSED (4408)
//! ```
//!
//! Subscriptions (`T-F4-04`), publishing (`T-F4-05`), presence (`T-F4-08`) and
//! `AuthRefresh` (`T-F4-09`) arrive on top of this machine; until then their messages are
//! answered with `StreamError{PROTOCOL_ERROR}` so the client sees an explicit error instead
//! of a silent drop (docs/08 §4).
#![forbid(unsafe_code)]

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, WebSocket};
use futures::StreamExt;
use gs_service::authz::{AuthzCache, PrincipalIdentity};
use gs_service::gs_auth::TenantPolicy;
use gs_service::gs_domain::Subprotocol;
use gs_service::gs_domain::error::{AsErrorCode, ErrorCode};
use gs_service::gs_domain::ids::ConnectionId;
use gs_service::gs_ephemeral::EphemeralStore;
use gs_service::gs_protocol::ErrorCode as ProtoErrorCode;
use gs_service::gs_protocol::client_message::Body as ClientBody;
use gs_service::gs_protocol::server_message::Body as ServerBody;
use gs_service::gs_protocol::{
    ClientHello, ClientMessage, Heartbeat, HeartbeatAck, PrincipalKind, ServerHello, ServerMessage,
    StreamError,
};
use gs_service::gs_store::PgPool;
use gs_service::tickets::{TicketGrant, TicketPrincipalKind};
use gs_service::{InMemoryBus, InMemoryEphemeralStore};
use pbjson_types::Timestamp;
use time::OffsetDateTime;
use tokio::time::Instant;

use crate::close_codes::{
    CLOSE_AUTH_EXPIRED, CLOSE_GOING_AWAY, CLOSE_IDLE_TIMEOUT, CLOSE_INTERNAL,
    CLOSE_MESSAGE_TOO_BIG, CLOSE_NORMAL, CLOSE_PROTOCOL_ERROR, CLOSE_RATE_LIMITED,
    CLOSE_SLOW_CONSUMER, CLOSE_UNSUPPORTED_PROTOCOL,
};
use crate::codec::{FrameCodec, FrameError};
use crate::config::{StreamConfig, StreamLimits as EffectiveLimits};

/// Metric of open connections (docs/16 §4.2).
const METRIC_STREAM_CONNECTIONS: &str = "gs_stream_connections";
/// Metric of opened connections (docs/16 §4.2).
const METRIC_STREAM_CONNECTIONS_OPENED_TOTAL: &str = "gs_stream_connections_opened_total";
/// Metric of closed connections (docs/16 §4.2).
const METRIC_STREAM_CONNECTIONS_CLOSED_TOTAL: &str = "gs_stream_connections_closed_total";
/// Metric of outbound messages (docs/16 §4.2).
const METRIC_STREAM_MESSAGES_OUT_TOTAL: &str = "gs_stream_messages_out_total";
/// Metric of inbound messages (docs/16 §4.2).
const METRIC_STREAM_MESSAGES_IN_TOTAL: &str = "gs_stream_messages_in_total";

/// Highest protocol minor this server implements (docs/08 §6.1 and §11).
pub const MAX_PROTOCOL_MINOR: u32 = 0;

/// Session accepted by the upgrade, handed over by `gs-api` (docs/08 §3).
pub struct AcceptedSession {
    /// Negotiated subprotocol of the connection.
    pub subprotocol: Subprotocol,
    /// Grant of the redeemed ticket: the principal of this connection (docs/06 §5).
    pub grant: TicketGrant,
    /// Correlation id of the upgrade request (`X-Request-Id`).
    pub request_id: String,
    /// Accepted WebSocket, already upgraded.
    pub socket: WebSocket,
}

/// State a connection needs besides its own grant.
#[derive(Clone)]
pub struct StreamState {
    /// PostgreSQL pool, used to read the tenant policy that shapes `limits` (docs/08 §6.1).
    pub pool: PgPool,
    /// Ephemeral store: `sequence_floor` of docs/08 §6.2.
    pub ephemeral: Arc<InMemoryEphemeralStore>,
    /// Bus for control events (used from T-F4-06 onwards).
    pub bus: Arc<InMemoryBus>,
    /// Per instance authorization cache (ADR-0009, docs/04 §6.2).
    pub authz: Arc<AuthzCache>,
    /// Ephemeral key prefix (`GS_REDIS_KEY_PREFIX`, default `gs`).
    pub redis_key_prefix: String,
    /// Stream configuration (docs/17 §2.6).
    pub config: StreamConfig,
}

impl std::fmt::Debug for StreamState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamState")
            .field("pool", &"<pg pool>")
            .field("bus", &"<bus>")
            .field("authz", &"<authz cache>")
            .field("redis_key_prefix", &self.redis_key_prefix)
            .field("config", &self.config)
            .finish()
    }
}

/// States of a connection (docs/08 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Upgrade done, waiting for `ClientHello`.
    AwaitingHello,
    /// Hello accepted: subscriptions and publishing are allowed.
    Ready,
    /// SIGTERM received: no new work, closing after the grace period (docs/08 §6.8).
    Draining,
    /// The connection is over.
    Closed,
}

/// How a connection ended (docs/08 §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionOutcome {
    /// Close code sent to the client.
    pub code: u16,
}

/// One WebSocket connection and its state machine.
pub struct Connection {
    connection_id: ConnectionId,
    codec: FrameCodec,
    grant: TicketGrant,
    config: StreamConfig,
    state: ConnectionState,
    close_code: u16,
    /// Instant at which the authentication window ends (docs/06 §5).
    auth_expires_at: OffsetDateTime,
    /// Limits reported in `ServerHello` (docs/08 §6.1).
    limits: EffectiveLimits,
    /// Protocol errors accumulated in this connection (docs/08 §4).
    protocol_errors: u32,
    /// First instant at which the inbound message limit was exceeded (docs/08 §7.4).
    abuse_started_at: Option<Instant>,
    /// Messages received in the current one second window (docs/08 §7.4).
    inbound_window_count: u32,
    /// Start of the current one second window.
    inbound_window_started: Instant,
    /// Identity whose state is revalidated by TTL (docs/06 §7, ADR-0009).
    principal: PrincipalIdentity,
}

impl Connection {
    /// Builds a connection for an accepted session.
    ///
    /// Fails only when the grant does not carry a parsable `auth_expires_at`, which the
    /// server itself wrote (docs/06 §5).
    pub fn new(
        connection_id: ConnectionId,
        subprotocol: Subprotocol,
        grant: TicketGrant,
        config: StreamConfig,
    ) -> Result<Self, String> {
        let auth_expires_at = OffsetDateTime::parse(
            &grant.auth_expires_at,
            &time::format_description::well_known::Rfc3339,
        )
        .map_err(|error| format!("auth_expires_at: {error}"))?;
        let principal = PrincipalIdentity {
            tenant_id: grant.tenant_id,
            application_id: grant.application_id,
            subject_id: grant.subject_id,
            device_id: grant.device_id,
            token_epoch: grant.token_epoch,
        };

        Ok(Self {
            connection_id,
            codec: FrameCodec::new(subprotocol),
            grant,
            config,
            state: ConnectionState::AwaitingHello,
            close_code: CLOSE_INTERNAL,
            auth_expires_at,
            limits: EffectiveLimits::default(),
            protocol_errors: 0,
            abuse_started_at: None,
            inbound_window_count: 0,
            inbound_window_started: Instant::now(),
            principal,
        })
    }

    /// Current state of the machine (docs/08 §5).
    pub const fn state(&self) -> ConnectionState {
        self.state
    }

    /// Connection id assigned by the server (docs/08 §6.1).
    pub const fn connection_id(&self) -> ConnectionId {
        self.connection_id
    }

    /// Principal kind of the redeemed ticket (docs/06 §5).
    pub fn principal_kind(&self) -> PrincipalKind {
        match self.grant.principal_kind {
            TicketPrincipalKind::Application => PrincipalKind::Application,
            TicketPrincipalKind::Device => PrincipalKind::Device,
        }
    }

    /// Negotiated subprotocol (docs/08 §2).
    pub const fn subprotocol(&self) -> Subprotocol {
        self.codec.subprotocol()
    }

    /// Instant at which the authentication window of this connection ends (docs/06 §5).
    pub const fn auth_expires_at(&self) -> OffsetDateTime {
        self.auth_expires_at
    }

    /// Runs the state machine until the connection closes (docs/08 §5).
    ///
    /// Never panics and always ends with an explicit close code; the socket is closed by
    /// [`run_session`] with that code and its name as reason (docs/08 §8).
    pub async fn serve(
        &mut self,
        socket: &mut WebSocket,
        state: &StreamState,
    ) -> ConnectionOutcome {
        let started = Instant::now();
        let hello_deadline = started + self.config.hello_timeout;
        let mut idle_deadline = started + self.config.idle_timeout;
        // The state was checked at the upgrade, so the first revalidation comes after one full
        // TTL of the cache (docs/06 §7).
        let mut authz_deadline = started + state.authz.ttl();

        loop {
            if self.auth_expires_at <= OffsetDateTime::now_utc() {
                // The window closed without a refresh (docs/08 §6.6, T-F4-09).
                self.close(CLOSE_AUTH_EXPIRED);
                return self.outcome();
            }

            // The principal is revalidated at most every `GS_AUTHZ_CACHE_TTL_MS`, and a bus
            // event can drop the cached entry earlier (docs/06 §7, ADR-0009).
            if matches!(self.state, ConnectionState::Ready) && Instant::now() >= authz_deadline {
                match self.revalidate_principal(state).await {
                    Ok(()) => authz_deadline = Instant::now() + state.authz.ttl(),
                    Err(code) => {
                        self.close(code);
                        return self.outcome();
                    }
                }
            }

            let frame = tokio::select! {
                frame = socket.next() => frame,
                // The state machine of docs/08 §5: no hello in time closes with 4002, no frame
                // at all during `idle_timeout` closes with 4408.
                _ = tokio::time::sleep_until(match self.state {
                    ConnectionState::AwaitingHello => hello_deadline,
                    _ => idle_deadline,
                }) => {
                    self.close(match self.state {
                        ConnectionState::AwaitingHello => crate::close_codes::CLOSE_HELLO_TIMEOUT,
                        _ => CLOSE_IDLE_TIMEOUT,
                    });
                    return self.outcome();
                }
                // Wakes the loop up to revalidate the principal when the TTL lapses.
                _ = tokio::time::sleep_until(authz_deadline),
                    if matches!(self.state, ConnectionState::Ready) => continue,
            };

            let Some(frame) = frame else {
                // The peer closed the underlying stream.
                self.close(CLOSE_GOING_AWAY);
                return self.outcome();
            };

            let frame = match frame {
                Ok(frame) => frame,
                Err(_error) => {
                    // A transport error has no code of its own; the connection cannot go on.
                    self.close(CLOSE_INTERNAL);
                    return self.outcome();
                }
            };

            // Every frame renews the inactivity deadline (docs/08 §6.5).
            idle_deadline = Instant::now() + self.config.idle_timeout;

            // Sustained abuse of the inbound message limit closes with 4429 (docs/08 §7.4).
            if self.register_inbound_message() == InboundVerdict::Abusive {
                self.close(CLOSE_RATE_LIMITED);
                return self.outcome();
            }

            match frame {
                Message::Text(payload) => {
                    self.handle_frame(socket, state, false, payload.as_bytes())
                        .await;
                }
                Message::Binary(payload) => {
                    self.handle_frame(socket, state, true, &payload).await;
                }
                // Control frames keep the connection alive (docs/08 §2).
                Message::Ping(_) | Message::Pong(_) => {}
                Message::Close(_) => {
                    self.close(CLOSE_NORMAL);
                    return self.outcome();
                }
            }

            if matches!(self.state, ConnectionState::Closed) {
                return self.outcome();
            }
        }
    }

    /// Decodes and dispatches one frame (docs/08 §4).
    async fn handle_frame(
        &mut self,
        socket: &mut WebSocket,
        state: &StreamState,
        binary: bool,
        payload: &[u8],
    ) {
        // Axum enforces `max_frame_size` on the upgrade, so an oversized frame never reaches
        // this point; the check keeps the documented limit explicit in one place (docs/08 §2).
        if payload.len() > self.config.max_frame_bytes {
            self.close(CLOSE_MESSAGE_TOO_BIG);
            return;
        }

        let message = match self.codec.decode_client(binary, payload) {
            Ok(message) => message,
            Err(FrameError::WrongFrameType) => {
                // Text frame with `gs.v1.proto` or binary frame with `gs.v1.json` (docs/08 §2).
                self.close(CLOSE_PROTOCOL_ERROR);
                return;
            }
            Err(FrameError::Malformed) => {
                // Before the hello nothing but a valid `ClientHello` is acceptable: the
                // machine of docs/08 §5 closes with 4000 (FR-STR-001).
                if matches!(self.state, ConnectionState::AwaitingHello) {
                    self.close(CLOSE_PROTOCOL_ERROR);
                    return;
                }
                self.reply_protocol_error(
                    socket,
                    0,
                    ProtoErrorCode::ProtocolError,
                    "malformed message",
                )
                .await;
                return;
            }
        };
        metrics::counter!(
            METRIC_STREAM_MESSAGES_IN_TOTAL,
            "type" => message_type_of_client(&message)
        )
        .increment(1);

        // The hello must be the first message of the connection (docs/08 §5, FR-STR-001).
        if !matches!(message.body, Some(ClientBody::Hello(_)))
            && matches!(self.state, ConnectionState::AwaitingHello)
        {
            self.close(CLOSE_PROTOCOL_ERROR);
            return;
        }

        let request_id = message.request_id;
        match message.body {
            Some(ClientBody::Hello(hello)) => {
                if !matches!(self.state, ConnectionState::AwaitingHello) {
                    self.reply_protocol_error(
                        socket,
                        request_id,
                        ProtoErrorCode::ProtocolError,
                        "hello already processed",
                    )
                    .await;
                    return;
                }
                self.handle_hello(socket, state, request_id, hello).await;
            }
            Some(ClientBody::Heartbeat(heartbeat)) => {
                if !matches!(self.state, ConnectionState::Ready) {
                    self.reply_protocol_error(
                        socket,
                        request_id,
                        ProtoErrorCode::ProtocolError,
                        "hello must be the first message",
                    )
                    .await;
                    return;
                }
                self.handle_heartbeat(socket, request_id, heartbeat).await;
            }
            // Subscriptions, publishing and `AuthRefresh` belong to T-F4-04, T-F4-05 and
            // T-F4-09; until they land the client gets an explicit protocol error
            // (docs/08 §4).
            Some(ClientBody::Subscribe(_))
            | Some(ClientBody::Unsubscribe(_))
            | Some(ClientBody::PublishLocation(_))
            | Some(ClientBody::AuthRefresh(_)) => {
                self.reply_protocol_error(
                    socket,
                    request_id,
                    ProtoErrorCode::ProtocolError,
                    "operation not supported yet",
                )
                .await;
            }
            None => {
                self.reply_protocol_error(
                    socket,
                    request_id,
                    ProtoErrorCode::ProtocolError,
                    "missing message body",
                )
                .await;
            }
        }
    }

    /// Validates the hello and answers `ServerHello` (docs/08 §6.1).
    async fn handle_hello(
        &mut self,
        socket: &mut WebSocket,
        state: &StreamState,
        request_id: u64,
        hello: ClientHello,
    ) {
        if hello.protocol_minor > MAX_PROTOCOL_MINOR {
            // The client declares a minor this server does not implement (docs/08 §6.1).
            self.close(CLOSE_UNSUPPORTED_PROTOCOL);
            return;
        }
        if !valid_hello_strings(&hello) {
            self.close(CLOSE_PROTOCOL_ERROR);
            return;
        }

        // For application principals `last_sequence` is ignored (docs/08 §6.1).
        let sequence_floor = match self.grant.principal_kind {
            TicketPrincipalKind::Device => self.sequence_floor(state).await,
            TicketPrincipalKind::Application => 0,
        };
        if let Ok(limits) = self.effective_limits(state).await {
            self.limits = limits;
        }

        let server_time = OffsetDateTime::now_utc();
        let response = ServerHello {
            connection_id: self.connection_id.to_string(),
            protocol_minor: hello.protocol_minor,
            server_time: Some(to_proto_timestamp(server_time)),
            auth_expires_at: Some(to_proto_timestamp(self.auth_expires_at)),
            sequence_floor,
            limits: Some(to_proto_limits(self.limits)),
            principal_kind: self.principal_kind() as i32,
            subject_id: self
                .grant
                .subject_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
            device_id: self
                .grant
                .device_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
            application_id: self.grant.application_id.to_string(),
        };
        self.state = ConnectionState::Ready;

        let message = ServerMessage {
            request_id,
            body: Some(ServerBody::Hello(response)),
        };
        if !self.send(socket, &message).await {
            self.close(CLOSE_SLOW_CONSUMER);
        }
    }

    /// Answers a heartbeat with the server clock (docs/08 §6.5).
    async fn handle_heartbeat(
        &mut self,
        socket: &mut WebSocket,
        request_id: u64,
        heartbeat: Heartbeat,
    ) {
        let ack = HeartbeatAck {
            client_time: heartbeat.client_time,
            server_time: Some(to_proto_timestamp(OffsetDateTime::now_utc())),
        };
        let message = ServerMessage {
            request_id,
            body: Some(ServerBody::HeartbeatAck(ack)),
        };
        if !self.send(socket, &message).await {
            self.close(CLOSE_SLOW_CONSUMER);
        }
    }

    /// Reads `{p}:devseq:{device_id}`, the sequence floor of docs/08 §6.2.
    async fn sequence_floor(&self, state: &StreamState) -> u64 {
        let Some(device_id) = self.grant.device_id else {
            return 0;
        };
        let key = format!("{}:devseq:{}", state.redis_key_prefix, device_id);
        state
            .ephemeral
            .get_counter(&key)
            .await
            .ok()
            .flatten()
            .unwrap_or(0)
    }

    /// Builds the effective limits from the tenant policy (docs/08 §6.1).
    async fn effective_limits(&self, state: &StreamState) -> Result<EffectiveLimits, String> {
        let tenant = gs_service::gs_store::repos::TenantRepository::get_by_id(
            &state.pool,
            self.grant.tenant_id,
        )
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "tenant not found".to_string())?;
        let policy = TenantPolicy::from_json_value(tenant.policy)
            .map_err(|error| format!("tenant policy: {error}"))?;
        let tenant_limits = crate::config::TenantStreamLimits::from(&policy);
        Ok(self
            .config
            .effective_limits(&tenant_limits, self.is_application()))
    }

    /// Whether the principal is an application (docs/06 §5).
    fn is_application(&self) -> bool {
        matches!(self.grant.principal_kind, TicketPrincipalKind::Application)
    }

    /// Registers an inbound message and reports sustained abuse (docs/08 §7.4).
    fn register_inbound_message(&mut self) -> InboundVerdict {
        let now = Instant::now();
        if now.duration_since(self.inbound_window_started) >= Duration::from_secs(1) {
            self.inbound_window_started = now;
            self.inbound_window_count = 0;
        }
        self.inbound_window_count = self.inbound_window_count.saturating_add(1);
        if self.inbound_window_count <= self.config.max_inbound_msgs_per_s {
            if self.inbound_window_count < self.config.max_inbound_msgs_per_s {
                self.abuse_started_at = None;
            }
            return InboundVerdict::Allowed;
        }

        // The excess must be sustained for `GS_STREAM_ABUSE_WINDOW_S` (docs/08 §7.4).
        let started = *self.abuse_started_at.get_or_insert(now);
        if now.duration_since(started) >= self.config.abuse_window {
            InboundVerdict::Abusive
        } else {
            InboundVerdict::Allowed
        }
    }

    /// Answers a protocol error without closing the connection (docs/08 §4).
    async fn reply_protocol_error(
        &mut self,
        socket: &mut WebSocket,
        request_id: u64,
        code: ProtoErrorCode,
        message: &str,
    ) {
        let error = StreamError {
            code: code as i32,
            message: message.to_string(),
            retry_after_ms: 0,
            field: String::new(),
            session_id: String::new(),
        };
        let reply = ServerMessage {
            request_id,
            body: Some(ServerBody::Error(error)),
        };
        self.send(socket, &reply).await;

        self.protocol_errors = self.protocol_errors.saturating_add(1);
        if self.protocol_errors >= self.config.max_protocol_errors {
            self.close(CLOSE_PROTOCOL_ERROR);
        }
    }

    /// Writes one server message, returning false when the socket refused it.
    async fn send(&self, socket: &mut WebSocket, message: &ServerMessage) -> bool {
        let Ok(payload) = self.codec.encode_server(message) else {
            return false;
        };
        let frame = if self.codec.is_text() {
            Message::Text(String::from_utf8_lossy(&payload).into_owned().into())
        } else {
            Message::Binary(payload.into())
        };
        metrics::counter!(METRIC_STREAM_MESSAGES_OUT_TOTAL, "type" => message_type(message))
            .increment(1);
        socket.send(frame).await.is_ok()
    }

    /// Retries the principal state of this connection against PostgreSQL (docs/06 §7).
    ///
    /// The cache answers inside its TTL, so the query runs at most once per
    /// `GS_AUTHZ_CACHE_TTL_MS` per connection. A revoked token or a deactivated device closes
    /// with 4401, and a disabled tenant or application with 4403 (docs/06 §7).
    async fn revalidate_principal(&mut self, state: &StreamState) -> Result<(), u16> {
        match state
            .authz
            .revalidate_principal(&state.pool, &self.principal)
            .await
        {
            Ok(()) => Ok(()),
            Err(error) => {
                tracing::debug!(
                    connection_id = %self.connection_id,
                    code = %error.error_code(),
                    "stream principal is no longer valid"
                );
                Err(match error.error_code() {
                    ErrorCode::Unauthenticated => CLOSE_AUTH_EXPIRED,
                    ErrorCode::AccountDisabled => crate::close_codes::CLOSE_FORBIDDEN,
                    _ => CLOSE_INTERNAL,
                })
            }
        }
    }

    /// Marks the connection closed with a code (docs/08 §5).
    fn close(&mut self, code: u16) {
        self.state = ConnectionState::Closed;
        self.close_code = code;
    }

    /// Outcome of a closed connection.
    const fn outcome(&self) -> ConnectionOutcome {
        ConnectionOutcome {
            code: self.close_code,
        }
    }
}

/// Verdict of the inbound message limit (docs/08 §7.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InboundVerdict {
    /// The message is accepted.
    Allowed,
    /// Sustained abuse: the connection closes with 4429.
    Abusive,
}

/// Runs an accepted connection until it closes (docs/08 §5).
///
/// The ticket was already redeemed by the upgrade; from here on the principal is the one in
/// the grant. The function always closes the socket explicitly with the documented code and
/// the code name as reason (docs/08 §8).
pub async fn run_session(session: AcceptedSession, state: Arc<StreamState>) {
    let AcceptedSession {
        subprotocol,
        grant,
        request_id: _,
        socket,
    } = session;

    let connection_id = ConnectionId::new();
    let mut connection =
        match Connection::new(connection_id, subprotocol, grant, state.config.clone()) {
            Ok(connection) => connection,
            Err(error) => {
                // The server wrote the grant itself; a failure here is its own fault.
                tracing::error!(error = %error, "connection grant is not usable");
                return;
            }
        };

    let principal_kind = connection.principal_kind();
    // Label values of docs/16 §4.2, never the protobuf enum name.
    let principal = match principal_kind {
        PrincipalKind::Application => "application",
        _ => "device",
    };
    metrics::gauge!(METRIC_STREAM_CONNECTIONS, "principal_kind" => principal).increment(1);
    metrics::counter!(METRIC_STREAM_CONNECTIONS_OPENED_TOTAL, "principal_kind" => principal)
        .increment(1);

    let mut socket = socket;
    let outcome = connection.serve(&mut socket, state.as_ref()).await;

    metrics::gauge!(METRIC_STREAM_CONNECTIONS, "principal_kind" => principal).decrement(1);
    metrics::counter!(
        METRIC_STREAM_CONNECTIONS_CLOSED_TOTAL,
        "code" => outcome.code.to_string()
    )
    .increment(1);
    tracing::debug!(
        connection_id = %connection.connection_id(),
        code = outcome.code,
        state = ?connection.state(),
        "stream connection closed"
    );

    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code: outcome.code,
            reason: crate::close_codes::CloseCode::new(outcome.code)
                .name()
                .into(),
        })))
        .await;
}

/// Whether the strings of a `ClientHello` respect the documented bounds (docs/08 §6.1).
fn valid_hello_strings(hello: &ClientHello) -> bool {
    let bounded = |value: &str, max: usize| {
        !value.is_empty()
            && value.len() <= max
            // "Caracteres imprimibles": no control characters of any kind (docs/09 §2).
            && value.chars().all(|c| (0x20..0x7f).contains(&(c as u32)))
    };
    bounded(&hello.sdk_name, 64)
        && bounded(&hello.sdk_version, 32)
        && bounded(&hello.app_version, 32)
}

/// Converts an instant into the protobuf timestamp of the protocol.
fn to_proto_timestamp(instant: OffsetDateTime) -> Timestamp {
    Timestamp {
        seconds: instant.unix_timestamp(),
        nanos: instant.nanosecond() as i32,
    }
}

/// Converts the effective limits into the protobuf message.
fn to_proto_limits(limits: EffectiveLimits) -> gs_service::gs_protocol::StreamLimits {
    gs_service::gs_protocol::StreamLimits {
        max_frame_bytes: limits.max_frame_bytes,
        max_subscriptions: limits.max_subscriptions,
        min_publish_interval_ms: limits.min_publish_interval_ms,
        max_samples_per_publish: limits.max_samples_per_publish,
        heartbeat_interval_ms: limits.heartbeat_interval_ms,
        idle_timeout_ms: limits.idle_timeout_ms,
        max_event_age_ms: limits.max_event_age_ms,
    }
}

/// Name of the `oneof` of a server message, for metrics (docs/16 §4.2).
fn message_type(message: &ServerMessage) -> String {
    match &message.body {
        Some(ServerBody::Hello(_)) => "hello",
        Some(ServerBody::SubscribeAck(_)) => "subscribe_ack",
        Some(ServerBody::UnsubscribeAck(_)) => "unsubscribe_ack",
        Some(ServerBody::PublishAck(_)) => "publish_ack",
        Some(ServerBody::HeartbeatAck(_)) => "heartbeat_ack",
        Some(ServerBody::AuthRefreshAck(_)) => "auth_refresh_ack",
        Some(ServerBody::LocationUpdate(_)) => "location_update",
        Some(ServerBody::MemberStatus(_)) => "member_status",
        Some(ServerBody::MemberEvent(_)) => "member_event",
        Some(ServerBody::SessionEvent(_)) => "session_event",
        Some(ServerBody::SubscriptionEnded(_)) => "subscription_ended",
        Some(ServerBody::AuthExpiring(_)) => "auth_expiring",
        Some(ServerBody::ServerNotice(_)) => "server_notice",
        Some(ServerBody::GeofenceEvent(_)) => "geofence_event",
        Some(ServerBody::Error(_)) => "error",
        None => "none",
    }
    .to_string()
}

/// Name of the `oneof` of a client message, for metrics (docs/16 §4.2).
fn message_type_of_client(message: &ClientMessage) -> String {
    match &message.body {
        Some(ClientBody::Hello(_)) => "hello",
        Some(ClientBody::Subscribe(_)) => "subscribe",
        Some(ClientBody::Unsubscribe(_)) => "unsubscribe",
        Some(ClientBody::PublishLocation(_)) => "publish_location",
        Some(ClientBody::Heartbeat(_)) => "heartbeat",
        Some(ClientBody::AuthRefresh(_)) => "auth_refresh",
        None => "none",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello(sdk_name: &str, sdk_version: &str, app_version: &str) -> ClientHello {
        ClientHello {
            protocol_minor: 0,
            sdk_name: sdk_name.to_string(),
            sdk_version: sdk_version.to_string(),
            app_version: app_version.to_string(),
            platform: 0,
            last_sequence: 0,
            client_time: None,
        }
    }

    #[test]
    fn test_hello_strings_respect_the_documented_bounds() {
        // covers: docs/08 §6.1
        assert!(valid_hello_strings(&hello(
            "geostream-flutter",
            "0.1.0",
            "1.0.0"
        )));
        assert!(
            !valid_hello_strings(&hello("", "0.1.0", "1.0.0")),
            "empty sdk_name"
        );
        assert!(
            !valid_hello_strings(&hello(&"x".repeat(65), "0.1.0", "1.0.0")),
            "sdk_name above 64 characters"
        );
        assert!(
            !valid_hello_strings(&hello("sdk", &"x".repeat(33), "1.0.0")),
            "sdk_version above 32 characters"
        );
        assert!(
            !valid_hello_strings(&hello("sdk", "0.1.0", &"x".repeat(33))),
            "app_version above 32 characters"
        );
        assert!(
            !valid_hello_strings(&hello("sdk\n", "0.1.0", "1.0.0")),
            "control character"
        );
    }

    #[test]
    fn test_message_types_are_the_oneof_names() {
        // covers: docs/16 §4.2
        let message = ServerMessage {
            request_id: 0,
            body: Some(ServerBody::HeartbeatAck(HeartbeatAck {
                client_time: None,
                server_time: None,
            })),
        };
        assert_eq!(message_type(&message), "heartbeat_ack");
        let client = ClientMessage {
            request_id: 0,
            body: Some(ClientBody::Heartbeat(Heartbeat { client_time: None })),
        };
        assert_eq!(message_type_of_client(&client), "heartbeat");
    }

    #[test]
    fn test_principal_kind_matches_the_grant() {
        // covers: docs/06 §5, docs/08 §6.1
        let grant = TicketGrant {
            principal_kind: TicketPrincipalKind::Device,
            tenant_id: gs_service::gs_domain::ids::TenantId::new(),
            application_id: gs_service::gs_domain::ids::ApplicationId::new(),
            subject_id: None,
            device_id: None,
            api_key_id: None,
            scopes: vec!["stream:connect".to_string()],
            token_epoch: Some(1),
            auth_expires_at: "2026-10-13T12:00:00Z".to_string(),
        };
        let connection = Connection::new(
            ConnectionId::new(),
            Subprotocol::Proto,
            grant,
            StreamConfig::default(),
        )
        .expect("connection");
        assert_eq!(connection.principal_kind(), PrincipalKind::Device);
        assert_eq!(connection.state(), ConnectionState::AwaitingHello);
        assert_eq!(connection.subprotocol(), Subprotocol::Proto);
    }
}
