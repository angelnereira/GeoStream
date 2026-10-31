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
use futures::SinkExt;
use futures::StreamExt;
use futures::stream::SplitSink;
use gs_service::auth::{AuthError, verify_device_token_principal};
use gs_service::authz::{AuthzCache, PrincipalIdentity};
use gs_service::control::{
    DeviceBusEvent, SessionControlEvent, format_rfc3339, publish_device_bus_event,
    publish_session_control, to_proto_member_status,
};
use gs_service::gs_auth::TenantPolicy;
use gs_service::gs_auth::jwt::{JwtError, SigningKeySet};
use gs_service::gs_domain::Subprotocol;
use gs_service::gs_domain::error::{AsErrorCode, ErrorCode};
use gs_service::gs_domain::ids::{ConnectionId, SessionId};
use gs_service::gs_domain::membership::Connectivity;
use gs_service::gs_ephemeral::EphemeralStore;
use gs_service::gs_protocol::ErrorCode as ProtoErrorCode;
use gs_service::gs_protocol::client_message::Body as ClientBody;
use gs_service::gs_protocol::server_message::Body as ServerBody;
use gs_service::gs_protocol::{
    AuthExpiring, AuthRefresh, AuthRefreshAck, ClientHello, ClientMessage, Heartbeat, HeartbeatAck,
    LocationUpdate, PrincipalKind, PublishAck, PublishLocation, ServerHello, ServerMessage,
    StreamError, Subscribe, SubscriptionEndReason, Unsubscribe,
};
use gs_service::gs_store::PgPool;
use gs_service::members::list_active_publishing_by_device;
use gs_service::publishing::{
    PublishCaller, PublishError, PublishParams, PublishThrottle, publish_ack_for,
};
use gs_service::subscriptions::{SubscribeError, SubscribeParams, SubscribePrincipal};
use gs_service::tickets::{TicketGrant, TicketPrincipalKind};
use gs_service::{Bus, InMemoryBus, InMemoryEphemeralStore};
use pbjson_types::Timestamp;
use prost::Message as _;
use std::str::FromStr;
use time::OffsetDateTime;
use tokio::sync::Notify;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::close_codes::{
    CLOSE_AUTH_EXPIRED, CLOSE_FORBIDDEN, CLOSE_GOING_AWAY, CLOSE_IDLE_TIMEOUT, CLOSE_INTERNAL,
    CLOSE_MESSAGE_TOO_BIG, CLOSE_NORMAL, CLOSE_PROTOCOL_ERROR, CLOSE_RATE_LIMITED,
    CLOSE_SLOW_CONSUMER, CLOSE_SUPERSEDED, CLOSE_UNSUPPORTED_PROTOCOL,
};
use crate::codec::{FrameCodec, FrameError};
use crate::config::{StreamConfig, StreamLimits as EffectiveLimits};
use crate::fanout::{ActiveConn, ConnHandle, Fanout, Frame, LatestMap, Outbound};
use crate::subscription::{SubscriptionSet, snapshot_sequences, subscribe_ack, unsubscribe_ack};

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

/// TTL of the trajectory-engine checkpoint at `{p}:trk:{device}`
/// (`GS_TRACK_STATE_TTL_S`, default 600 s; `17` §2.10, `docs/22` §2.1).
const TRACK_STATE_TTL: Duration = Duration::from_secs(600);

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
    /// Bus for control events (docs/04 §6.1).
    pub bus: Arc<InMemoryBus>,
    /// Local fan-out of the instance (docs/04 §8).
    pub fanout: Arc<Fanout>,
    /// Per instance authorization cache (ADR-0009, docs/04 §6.2).
    pub authz: Arc<AuthzCache>,
    /// Ephemeral key prefix (`GS_REDIS_KEY_PREFIX`, default `gs`).
    pub redis_key_prefix: String,
    /// Stream configuration (docs/17 §2.6).
    pub config: StreamConfig,
    /// Public keys for verifying device tokens on AuthRefresh (docs/06 §4.4).
    pub signing_keys: Arc<SigningKeySet>,
    /// JWT issuer (docs/06 §4.3).
    pub jwt_issuer: String,
    /// JWT verification leeway in seconds (docs/06 §4.3).
    pub jwt_leeway_seconds: u64,
}

impl std::fmt::Debug for StreamState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamState")
            .field("pool", &"<pg pool>")
            .field("bus", &"<bus>")
            .field("authz", &"<authz cache>")
            .field("redis_key_prefix", &self.redis_key_prefix)
            .field("config", &self.config)
            .field("jwt_issuer", &self.jwt_issuer)
            .field("jwt_leeway_seconds", &self.jwt_leeway_seconds)
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
    /// Subscriptions of this connection (docs/08 §6.3).
    subscriptions: SubscriptionSet,
    /// Outbound side drained by the writer task (docs/04 §8).
    outbound: Outbound,
    /// Cancelled when the connection ends (docs/04 §8).
    cancel: CancellationToken,
    /// Publish token buckets per (connection, session) (docs/08 §7.4).
    throttles: std::collections::HashMap<SessionId, PublishThrottle>,
    /// Sender for session end notifications from fan-out.
    session_end_tx: mpsc::Sender<(SessionId, SubscriptionEndReason)>,
    /// Receiver for session end notifications from fan-out.
    session_end_rx: mpsc::Receiver<(SessionId, SubscriptionEndReason)>,
    /// Last instant at which presence was renewed in ephemeral store (docs/08 §6.5).
    last_presence_renew: Instant,
    /// Whether `AuthExpiring` has already been sent for the current auth expiration window.
    auth_expiring_sent: bool,
    /// Tracking engine (E2) of the publishing device. One connection per device
    /// (FR-STR-007), so one engine per connection is the correct unit.
    tracker: gs_track::TrackStateMachine,
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
        outbound: Outbound,
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
        let (session_end_tx, session_end_rx) = mpsc::channel(64);

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
            subscriptions: SubscriptionSet::new(),
            throttles: std::collections::HashMap::new(),
            outbound,
            cancel: CancellationToken::new(),
            session_end_tx,
            session_end_rx,
            last_presence_renew: Instant::now(),
            auth_expiring_sent: false,
            tracker: gs_track::TrackStateMachine::new(gs_track::TrackConfig::defaults()),
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

    /// Principal identity for selective security revocations and authz checks (docs/06 §5).
    pub fn principal(&self) -> &PrincipalIdentity {
        &self.principal
    }

    /// Subscriptions of this connection (docs/08 §6.3).
    pub fn subscriptions(&self) -> &SubscriptionSet {
        &self.subscriptions
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

    /// Runs the reader side of the state machine until the connection closes (docs/08 §5).
    ///
    /// The socket belongs to the writer task (docs/04 §8): the reader only decodes and
    /// dispatches, and stops as soon as the writer reports that it can no longer write.
    /// Never panics and always ends with an explicit close code.
    pub async fn serve(
        &mut self,
        stream: &mut futures::stream::SplitStream<WebSocket>,
        state: &StreamState,
        cancel: &CancellationToken,
        close_signal_rx: &mut mpsc::Receiver<u16>,
    ) -> ConnectionOutcome {
        let started = Instant::now();
        let hello_deadline = started + self.config.hello_timeout;
        let mut idle_deadline = started + self.config.idle_timeout;
        // The state was checked at the upgrade, so the first revalidation comes after one full
        // TTL of the cache (docs/06 §7).
        let mut authz_deadline = started + state.authz.ttl();
        let presence_interval = (self.config.presence_ttl / 3).max(Duration::from_millis(50));
        let mut next_presence_renew = started + presence_interval;
        let mut close_signal_closed = false;

        loop {
            let now_utc = OffsetDateTime::now_utc();
            if self.auth_expires_at <= now_utc {
                // The window closed without a refresh (docs/08 §6.6, T-F4-09).
                self.close(CLOSE_AUTH_EXPIRED);
                return self.outcome();
            }
            let remaining_dur = (self.auth_expires_at - now_utc)
                .try_into()
                .unwrap_or(Duration::ZERO);
            let auth_expire_deadline = Instant::now() + remaining_dur;

            let lead_dur = self.config.auth_expiring_lead;
            let next_auth_expiring =
                if !self.auth_expiring_sent && matches!(self.state, ConnectionState::Ready) {
                    if remaining_dur <= lead_dur {
                        let auth_expiring = AuthExpiring {
                            auth_expires_at: Some(to_proto_timestamp(self.auth_expires_at)),
                        };
                        let message = ServerMessage {
                            request_id: 0,
                            body: Some(ServerBody::AuthExpiring(auth_expiring)),
                        };
                        if !self.send(&message).await {
                            self.close(CLOSE_SLOW_CONSUMER);
                            return self.outcome();
                        }
                        self.auth_expiring_sent = true;
                        Instant::now() + Duration::from_secs(86400 * 365)
                    } else {
                        Instant::now() + (remaining_dur - lead_dur)
                    }
                } else {
                    Instant::now() + Duration::from_secs(86400 * 365)
                };

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
                code = close_signal_rx.recv(), if !close_signal_closed => {
                    match code {
                        Some(code) => {
                            self.close(code);
                            return self.outcome();
                        }
                        None => {
                            close_signal_closed = true;
                            continue;
                        }
                    }
                }
                end = self.session_end_rx.recv() => {
                    if let Some((session_id, _reason)) = end {
                        self.subscriptions.remove(session_id);
                        self.throttles.remove(&session_id);
                    }
                    continue;
                }
                // Periodic presence renewal for devices (docs/04 §6.3, docs/08 §6.5)
                _ = tokio::time::sleep_until(next_presence_renew),
                    if matches!(self.state, ConnectionState::Ready)
                        && matches!(self.grant.principal_kind, TicketPrincipalKind::Device)
                        && self.grant.device_id.is_some() => {
                    if let Some(device_id) = self.grant.device_id {
                        let presence_key = format!("{}:presence:{}", state.redis_key_prefix, device_id);
                        let conn_id_str = self.connection_id.to_string();
                        let now_str = format_rfc3339(OffsetDateTime::now_utc());
                        let fields = [
                            ("connection_id", conn_id_str.as_str()),
                            ("instance_id", self.config.instance_id.as_str()),
                            ("connected_at", now_str.as_str()),
                        ];
                        let _ = state
                            .ephemeral
                            .set_presence(&presence_key, &fields, self.config.presence_ttl)
                            .await;
                        self.last_presence_renew = Instant::now();
                    }
                    next_presence_renew = Instant::now() + presence_interval;
                    continue;
                }
                // Auth window expiry (docs/08 §6.6, FR-AUTH-007)
                _ = tokio::time::sleep_until(auth_expire_deadline) => {
                    self.close(CLOSE_AUTH_EXPIRED);
                    return self.outcome();
                }
                // Auth expiring lead notification (docs/08 §6.6, FR-AUTH-007)
                _ = tokio::time::sleep_until(next_auth_expiring),
                    if matches!(self.state, ConnectionState::Ready) && !self.auth_expiring_sent => {
                    let auth_expiring = AuthExpiring {
                        auth_expires_at: Some(to_proto_timestamp(self.auth_expires_at)),
                    };
                    let message = ServerMessage {
                        request_id: 0,
                        body: Some(ServerBody::AuthExpiring(auth_expiring)),
                    };
                    if !self.send(&message).await {
                        self.close(CLOSE_SLOW_CONSUMER);
                        return self.outcome();
                    }
                    self.auth_expiring_sent = true;
                    continue;
                }
                frame = stream.next() => frame,
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
                // The writer could no longer write: the connection is over (docs/04 §8).
                _ = cancel.cancelled() => {
                    self.close(CLOSE_INTERNAL);
                    return self.outcome();
                }
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
                    self.handle_frame(state, false, payload.as_bytes()).await;
                }
                Message::Binary(payload) => {
                    self.handle_frame(state, true, &payload).await;
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
    async fn handle_frame(&mut self, state: &StreamState, binary: bool, payload: &[u8]) {
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
                self.reply_protocol_error(0, ProtoErrorCode::ProtocolError, "malformed message")
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
                        request_id,
                        ProtoErrorCode::ProtocolError,
                        "hello already processed",
                    )
                    .await;
                    return;
                }
                self.handle_hello(state, request_id, hello).await;
            }
            Some(ClientBody::Heartbeat(heartbeat)) => {
                if !matches!(self.state, ConnectionState::Ready) {
                    self.reply_protocol_error(
                        request_id,
                        ProtoErrorCode::ProtocolError,
                        "hello must be the first message",
                    )
                    .await;
                    return;
                }
                self.handle_heartbeat(state, request_id, heartbeat).await;
            }
            Some(ClientBody::Subscribe(subscribe)) => {
                self.handle_subscribe(state, request_id, subscribe).await;
            }
            Some(ClientBody::Unsubscribe(unsubscribe)) => {
                self.handle_unsubscribe(state, request_id, unsubscribe)
                    .await;
            }
            Some(ClientBody::PublishLocation(publish)) => {
                self.handle_publish(state, request_id, publish).await;
            }
            Some(ClientBody::AuthRefresh(refresh)) => {
                if !matches!(self.state, ConnectionState::Ready) {
                    self.reply_protocol_error(
                        request_id,
                        ProtoErrorCode::ProtocolError,
                        "hello must be the first message",
                    )
                    .await;
                    return;
                }
                self.handle_auth_refresh(state, request_id, refresh).await;
            }
            None => {
                self.reply_protocol_error(
                    request_id,
                    ProtoErrorCode::ProtocolError,
                    "missing message body",
                )
                .await;
            }
        }
    }

    /// Validates the hello and answers `ServerHello` (docs/08 §6.1).
    async fn handle_hello(&mut self, state: &StreamState, request_id: u64, hello: ClientHello) {
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
            // Sampling policy (E3) is not sent until its phase exists; a client
            // that receives no policy uses its own defaults (docs/22 §3.4).
            sampling: None,
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
        if !self.send(&message).await {
            self.close(CLOSE_SLOW_CONSUMER);
            return;
        }

        // Establish device presence and single connection guarantee (FR-STR-007, FR-PRS-002, docs/04 §6.3, docs/08 §6.7)
        if matches!(self.grant.principal_kind, TicketPrincipalKind::Device)
            && let Some(device_id) = self.grant.device_id
        {
            self.last_presence_renew = Instant::now();
            let presence_key = format!("{}:presence:{}", state.redis_key_prefix, device_id);
            let conn_id_str = self.connection_id.to_string();
            let server_time_str = format_rfc3339(server_time);
            let fields = [
                ("connection_id", conn_id_str.as_str()),
                ("instance_id", self.config.instance_id.as_str()),
                ("connected_at", server_time_str.as_str()),
            ];
            let _ = state
                .ephemeral
                .set_presence(&presence_key, &fields, self.config.presence_ttl)
                .await;

            // Close any older connection of the same device on this instance with 4409 (FR-STR-007)
            for entry in state.fanout.active_connections.iter() {
                let conn = entry.value();
                if conn.principal.device_id == Some(device_id)
                    && conn.connection_id != self.connection_id
                {
                    let _ = conn.close_tx.send(CLOSE_SUPERSEDED).await;
                }
            }

            // Publish supersede event on bus to close connections on other instances (docs/04 §6.3, docs/08 §6.7, FR-STR-007)
            let supersede = DeviceBusEvent::Supersede {
                device_id: device_id.to_string(),
                connection_id: conn_id_str,
                at: server_time_str.clone(),
            };
            let _ = publish_device_bus_event(
                state.bus.as_ref(),
                &state.redis_key_prefix,
                device_id,
                &supersede,
            )
            .await;

            // Broadcast ONLINE to active publishing sessions (docs/05 §8, FR-PRS-002)
            if let Ok(publishers) =
                list_active_publishing_by_device(&state.pool, self.grant.tenant_id, device_id).await
            {
                for pub_member in publishers {
                    let proto_status = to_proto_member_status(
                        pub_member.membership.session_id,
                        pub_member.membership.id,
                        &pub_member.external_subject_id,
                        pub_member.membership.role,
                        pub_member.membership.capabilities,
                        true,
                        Connectivity::Online,
                        None,
                    );
                    let event = SessionControlEvent::MemberStatusChanged {
                        session_id: pub_member.membership.session_id.to_string(),
                        member: proto_status,
                        at: server_time_str.clone(),
                    };
                    let _ = publish_session_control(
                        state.bus.as_ref(),
                        &state.redis_key_prefix,
                        pub_member.membership.session_id,
                        &event,
                    )
                    .await;
                }
            }

            // Resume the trajectory engine from the checkpoint of this device
            // (docs/22 §2.1, T-E2-06). A device that reconnects on another
            // instance continues its filter instead of starting cold. A
            // checkpoint that cannot be read is discarded, never half-applied.
            let trk_key = format!("{}:trk:{}", state.redis_key_prefix, device_id);
            if let Ok(Some((bytes, _version))) = state.ephemeral.get_checkpoint(&trk_key).await
                && let Some(restored) = gs_track::TrackStateMachine::decode_checkpoint(&bytes)
            {
                self.tracker = restored;
            }
        }

        // If remaining time until auth_expires_at is already <= auth_expiring_lead,
        // send AuthExpiring immediately (docs/08 §6.6).
        let now_utc = OffsetDateTime::now_utc();
        let remaining_secs = (self.auth_expires_at - now_utc).whole_seconds();
        if remaining_secs <= self.config.auth_expiring_lead.as_secs() as i64 {
            let auth_expiring = AuthExpiring {
                auth_expires_at: Some(to_proto_timestamp(self.auth_expires_at)),
            };
            let message = ServerMessage {
                request_id: 0,
                body: Some(ServerBody::AuthExpiring(auth_expiring)),
            };
            if !self.send(&message).await {
                self.close(CLOSE_SLOW_CONSUMER);
                return;
            }
            self.auth_expiring_sent = true;
        }
    }

    /// Handles `AuthRefresh` from a device or application (docs/08 §6.6, FR-AUTH-007).
    async fn handle_auth_refresh(
        &mut self,
        state: &StreamState,
        request_id: u64,
        refresh: AuthRefresh,
    ) {
        if self.is_application() {
            let error = StreamError {
                code: ProtoErrorCode::PermissionDenied as i32,
                message: "applications cannot refresh authentication; reconnect with a new ticket"
                    .to_string(),
                retry_after_ms: 0,
                field: String::new(),
                session_id: String::new(),
            };
            let reply = ServerMessage {
                request_id,
                body: Some(ServerBody::Error(error)),
            };
            let _ = self.send(&reply).await;
            return;
        }

        match verify_device_token_principal(
            &state.pool,
            &refresh.access_token,
            &state.signing_keys,
            &state.jwt_issuer,
            state.jwt_leeway_seconds,
        )
        .await
        {
            Ok(verified) => {
                let expected_did = self.grant.device_id;
                if expected_did.is_none() || expected_did != Some(verified.device.id) {
                    let error = StreamError {
                        code: ProtoErrorCode::PermissionDenied as i32,
                        message: "token belongs to another device".to_string(),
                        retry_after_ms: 0,
                        field: String::new(),
                        session_id: String::new(),
                    };
                    let reply = ServerMessage {
                        request_id,
                        body: Some(ServerBody::Error(error)),
                    };
                    let _ = self.send(&reply).await;
                    self.close(CLOSE_FORBIDDEN);
                    return;
                }

                if verified.tenant.id != self.grant.tenant_id
                    || verified.application.id != self.grant.application_id
                {
                    let error = StreamError {
                        code: ProtoErrorCode::PermissionDenied as i32,
                        message: "token belongs to another tenant or application".to_string(),
                        retry_after_ms: 0,
                        field: String::new(),
                        session_id: String::new(),
                    };
                    let reply = ServerMessage {
                        request_id,
                        body: Some(ServerBody::Error(error)),
                    };
                    let _ = self.send(&reply).await;
                    self.close(CLOSE_FORBIDDEN);
                    return;
                }

                if let Ok(new_exp) = OffsetDateTime::from_unix_timestamp(verified.claims.exp) {
                    self.auth_expires_at = new_exp;
                    self.grant.auth_expires_at = format_rfc3339(new_exp);
                }
                self.grant.scopes = verified.claims.scp;
                self.grant.token_epoch = Some(verified.claims.sep);
                self.principal.token_epoch = Some(verified.claims.sep);
                self.auth_expiring_sent = false;

                let ack = AuthRefreshAck {
                    auth_expires_at: Some(to_proto_timestamp(self.auth_expires_at)),
                };
                let reply = ServerMessage {
                    request_id,
                    body: Some(ServerBody::AuthRefreshAck(ack)),
                };
                if !self.send(&reply).await {
                    self.close(CLOSE_SLOW_CONSUMER);
                }
            }
            Err(auth_err) => {
                let code = match &auth_err {
                    AuthError::Token(JwtError::Expired) => ProtoErrorCode::TokenExpired,
                    _ => ProtoErrorCode::Unauthenticated,
                };
                let message = match code {
                    ProtoErrorCode::TokenExpired => "device token expired",
                    _ => "unauthenticated device token",
                };
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
                let _ = self.send(&reply).await;
            }
        }
    }

    /// Answers a heartbeat with the server clock and renews presence (docs/08 §6.5).
    async fn handle_heartbeat(
        &mut self,
        state: &StreamState,
        request_id: u64,
        heartbeat: Heartbeat,
    ) {
        if matches!(self.grant.principal_kind, TicketPrincipalKind::Device)
            && let Some(device_id) = self.grant.device_id
        {
            let presence_interval = (self.config.presence_ttl / 3).max(Duration::from_millis(50));
            if Instant::now().duration_since(self.last_presence_renew) >= presence_interval {
                let presence_key = format!("{}:presence:{}", state.redis_key_prefix, device_id);
                let conn_id_str = self.connection_id.to_string();
                let now_str = format_rfc3339(OffsetDateTime::now_utc());
                let fields = [
                    ("connection_id", conn_id_str.as_str()),
                    ("instance_id", self.config.instance_id.as_str()),
                    ("connected_at", now_str.as_str()),
                ];
                let _ = state
                    .ephemeral
                    .set_presence(&presence_key, &fields, self.config.presence_ttl)
                    .await;
                self.last_presence_renew = Instant::now();
            }
        }
        let ack = HeartbeatAck {
            client_time: heartbeat.client_time,
            server_time: Some(to_proto_timestamp(OffsetDateTime::now_utc())),
        };
        let message = ServerMessage {
            request_id,
            body: Some(ServerBody::HeartbeatAck(ack)),
        };
        if !self.send(&message).await {
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
    async fn reply_protocol_error(&mut self, request_id: u64, code: ProtoErrorCode, message: &str) {
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
        self.send(&reply).await;

        self.protocol_errors = self.protocol_errors.saturating_add(1);
        if self.protocol_errors >= self.config.max_protocol_errors {
            self.close(CLOSE_PROTOCOL_ERROR);
        }
    }

    /// Writes one server message, returning false when the socket refused it.
    async fn send(&self, message: &ServerMessage) -> bool {
        metrics::counter!(METRIC_STREAM_MESSAGES_OUT_TOTAL, "type" => message_type(message))
            .increment(1);
        self.outbound
            .send_control(message, self.codec.subprotocol())
            .await
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

    /// Subscribes to a session with the gap-free algorithm of docs/08 §6.3.
    ///
    /// A second `Subscribe` to the same session is idempotent: it answers a new
    /// `SubscribeAck` without creating a second subscription.
    async fn handle_subscribe(
        &mut self,
        state: &StreamState,
        request_id: u64,
        subscribe: Subscribe,
    ) {
        let session_id = match SessionId::from_str(&subscribe.session_id) {
            Ok(session_id) => session_id,
            Err(_) => {
                self.reply_stream_error(
                    request_id,
                    ProtoErrorCode::ValidationFailed,
                    String::new(),
                    "malformed session id",
                )
                .await;
                return;
            }
        };

        // Already subscribed: the answer is rebuilt, nothing changes (docs/08 §6.3).
        if self.subscriptions.contains(session_id) {
            match self.subscribe_view(state, session_id).await {
                Ok(view) => {
                    let message = ServerMessage {
                        request_id,
                        body: Some(ServerBody::SubscribeAck(subscribe_ack(&view))),
                    };
                    if !self.send(&message).await {
                        self.close(CLOSE_SLOW_CONSUMER);
                    }
                }
                Err(error) => self.reply_subscribe_error(request_id, error).await,
            }
            return;
        }

        let max = if self.is_application() {
            self.config.max_subscriptions_app
        } else {
            self.config.max_subscriptions
        };
        if !self.subscriptions.insert(session_id, max) {
            self.reply_stream_error(
                request_id,
                ProtoErrorCode::TooManySubscriptions,
                subscribe.session_id.clone(),
                "too many subscriptions",
            )
            .await;
            return;
        }

        // The subscription is registered as PENDING before reading, so that nothing that
        // happens meanwhile is lost (docs/08 §6.3).
        match self.subscribe_view(state, session_id).await {
            Ok(view) => {
                if let Some(subscription) = self.subscriptions.get_mut(session_id) {
                    subscription.activate(&snapshot_sequences(&view));
                }
                // The connection now receives what the session emits (docs/04 §8).
                state.fanout.subscribe(session_id, self.handle()).await;
                let message = ServerMessage {
                    request_id,
                    body: Some(ServerBody::SubscribeAck(subscribe_ack(&view))),
                };
                if !self.send(&message).await {
                    self.close(CLOSE_SLOW_CONSUMER);
                }
            }
            Err(error) => {
                self.subscriptions.remove(session_id);
                self.reply_subscribe_error(request_id, error).await;
            }
        }
    }

    /// Reads the session, the member statuses and the snapshot of a subscription.
    async fn subscribe_view(
        &self,
        state: &StreamState,
        session_id: SessionId,
    ) -> Result<gs_service::subscriptions::SubscribeView, SubscribeError> {
        let principal = match self.grant.principal_kind {
            TicketPrincipalKind::Application => SubscribePrincipal::Application {
                application_id: self.grant.application_id,
                scopes: &self.grant.scopes,
            },
            TicketPrincipalKind::Device => SubscribePrincipal::Device {
                subject_id: self
                    .grant
                    .subject_id
                    .ok_or(SubscribeError::PermissionDenied("subject"))?,
                device_id: self
                    .grant
                    .device_id
                    .ok_or(SubscribeError::PermissionDenied("device"))?,
            },
        };
        gs_service::subscriptions::subscribe(SubscribeParams {
            pool: &state.pool,
            ephemeral: state.ephemeral.as_ref(),
            key_prefix: &state.redis_key_prefix,
            tenant_id: self.grant.tenant_id,
            session_id,
            principal,
            now: OffsetDateTime::now_utc(),
        })
        .await
    }

    /// Unsubscribes from a session, always answering `UnsubscribeAck` (docs/08 §6.3).
    async fn handle_unsubscribe(
        &mut self,
        state: &StreamState,
        request_id: u64,
        unsubscribe: Unsubscribe,
    ) {
        let session_id = match SessionId::from_str(&unsubscribe.session_id) {
            Ok(session_id) => session_id,
            Err(_) => {
                self.reply_stream_error(
                    request_id,
                    ProtoErrorCode::ValidationFailed,
                    String::new(),
                    "malformed session id",
                )
                .await;
                return;
            }
        };
        // Idempotent: unsubscribing from something not subscribed still answers
        // `UnsubscribeAck` (docs/08 §6.3).
        self.subscriptions.remove(session_id);
        if !self.subscriptions.contains(session_id) {
            // The connection stops receiving what the session emits (docs/04 §8).
            state.fanout.unsubscribe(session_id, self.connection_id);
        }
        let message = ServerMessage {
            request_id,
            body: Some(ServerBody::UnsubscribeAck(unsubscribe_ack(session_id))),
        };
        if !self.send(&message).await {
            self.close(CLOSE_SLOW_CONSUMER);
        }
    }

    /// Renders a subscription error as a `StreamError` (docs/08 §6.3).
    async fn reply_subscribe_error(&mut self, request_id: u64, error: SubscribeError) {
        let code = match error.error_code() {
            ErrorCode::NotFound => ProtoErrorCode::NotFound,
            ErrorCode::PermissionDenied => ProtoErrorCode::PermissionDenied,
            ErrorCode::SessionClosed => ProtoErrorCode::SessionClosed,
            ErrorCode::SessionExpired => ProtoErrorCode::SessionExpired,
            ErrorCode::ValidationFailed => ProtoErrorCode::ValidationFailed,
            _ => ProtoErrorCode::Internal,
        };
        self.reply_stream_error(request_id, code, String::new(), &error.to_string())
            .await;
    }

    /// Answers a `StreamError`, echoing the request id (docs/08 §4).
    async fn reply_stream_error(
        &mut self,
        request_id: u64,
        code: ProtoErrorCode,
        session_id: String,
        message: &str,
    ) {
        let error = StreamError {
            code: code as i32,
            message: message.to_string(),
            retry_after_ms: 0,
            field: String::new(),
            session_id,
        };
        let reply = ServerMessage {
            request_id,
            body: Some(ServerBody::Error(error)),
        };
        self.send(&reply).await;
    }

    /// Publishes a batch of samples with the preconditions and the per-sample algorithm of
    /// docs/08 §6.4 and docs/09 §4.
    ///
    /// The connection owns the token bucket of (connection, session), so the rate limit of
    /// docs/08 §7.4 is enforced per connection and not per process.
    async fn handle_publish(
        &mut self,
        state: &StreamState,
        request_id: u64,
        publish: PublishLocation,
    ) {
        let session_id = match SessionId::from_str(&publish.session_id) {
            Ok(session_id) => session_id,
            Err(_) => {
                self.reply_publish_error(
                    request_id,
                    &publish.session_id,
                    ProtoErrorCode::ValidationFailed,
                    0,
                    "malformed session id",
                )
                .await;
                return;
            }
        };

        // An application principal may not publish (docs/06 §8.3).
        if self.is_application() {
            self.reply_publish_error(
                request_id,
                &publish.session_id,
                ProtoErrorCode::PermissionDenied,
                0,
                "application principals cannot publish",
            )
            .await;
            return;
        }

        let (subject_id, device_id) = match (self.grant.subject_id, self.grant.device_id) {
            (Some(subject_id), Some(device_id)) => (subject_id, device_id),
            _ => {
                self.reply_publish_error(
                    request_id,
                    &publish.session_id,
                    ProtoErrorCode::PermissionDenied,
                    0,
                    "device credentials required",
                )
                .await;
                return;
            }
        };

        let (policy, publish_burst, publish_interval) = self.location_policy(state).await;
        let throttle = self.throttles.entry(session_id).or_insert_with(|| {
            PublishThrottle::new(publish_burst, publish_interval, OffsetDateTime::now_utc())
        });

        match gs_service::publishing::publish_batch(PublishParams {
            pool: &state.pool,
            ephemeral: state.ephemeral.as_ref(),
            key_prefix: &state.redis_key_prefix,
            tenant_id: self.grant.tenant_id,
            session_id,
            caller: PublishCaller {
                subject_id,
                device_id,
            },
            samples: &publish.samples,
            policy,
            throttle,
            tracker: Some(&mut self.tracker),
            now: OffsetDateTime::now_utc(),
        })
        .await
        {
            Ok(outcome) => {
                // The request is answered first; the accepted updates follow in the order they
                // were enqueued (docs/08 §7.6).
                let message = ServerMessage {
                    request_id,
                    body: Some(ServerBody::PublishAck(publish_ack_for(
                        &publish.session_id,
                        &outcome,
                    ))),
                };
                if !self.send(&message).await {
                    self.close(CLOSE_SLOW_CONSUMER);
                    return;
                }
                // Checkpoint the engine so a reconnect on another instance
                // resumes the trajectory (docs/22 §2.1, T-E2-06). A failed
                // checkpoint is not fatal: it expires by its own TTL.
                if !outcome.accepted.is_empty() {
                    let trk_key = format!("{}:trk:{}", state.redis_key_prefix, device_id);
                    let blob = self.tracker.encode_checkpoint();
                    let _ = state
                        .ephemeral
                        .set_checkpoint(&trk_key, &blob, TRACK_STATE_TTL)
                        .await;
                }
                self.publish_to_bus(state, session_id, &outcome.accepted)
                    .await;
            }
            Err(error) => {
                let code = match error.error_code() {
                    ErrorCode::ValidationFailed => ProtoErrorCode::ValidationFailed,
                    ErrorCode::PayloadTooLarge => ProtoErrorCode::PayloadTooLarge,
                    ErrorCode::PermissionDenied => ProtoErrorCode::PermissionDenied,
                    ErrorCode::RateLimited => ProtoErrorCode::RateLimited,
                    ErrorCode::NotFound => ProtoErrorCode::NotFound,
                    ErrorCode::MemberRevoked => ProtoErrorCode::MemberRevoked,
                    ErrorCode::NotPublisher => ProtoErrorCode::NotPublisher,
                    ErrorCode::NotSharing => ProtoErrorCode::NotSharing,
                    ErrorCode::SessionClosed => ProtoErrorCode::SessionClosed,
                    ErrorCode::SessionExpired => ProtoErrorCode::SessionExpired,
                    ErrorCode::TemporarilyUnavailable => ProtoErrorCode::TemporarilyUnavailable,
                    _ => ProtoErrorCode::Internal,
                };
                let retry_after_ms = match &error {
                    PublishError::RateLimited { retry_after_ms }
                    | PublishError::TemporarilyUnavailable { retry_after_ms } => *retry_after_ms,
                    _ => 0,
                };
                self.reply_publish_error(
                    request_id,
                    &publish.session_id,
                    code,
                    retry_after_ms,
                    &error.to_string(),
                )
                .await;
            }
        }
    }

    /// Reads the tenant policy that shapes the limits of publication (docs/17 §4).
    ///
    /// Returns the location policy of the pure validation and the two throttle parameters
    /// (`publish_burst` and `min_publish_interval_ms`), which live outside the location policy
    /// because they belong to the connection and not to the sample.
    async fn location_policy(
        &self,
        state: &StreamState,
    ) -> (
        gs_service::gs_domain::location::LocationPolicy,
        u32,
        Duration,
    ) {
        let tenant = gs_service::gs_store::repos::TenantRepository::get_by_id(
            &state.pool,
            self.grant.tenant_id,
        )
        .await;
        let policy = match tenant {
            Ok(Some(tenant)) => {
                gs_service::gs_auth::TenantPolicy::from_json_value(tenant.policy).ok()
            }
            _ => None,
        };
        let policy = policy.unwrap_or_default();
        let location = gs_service::gs_domain::location::LocationPolicy {
            max_horizontal_accuracy_m: policy.max_horizontal_accuracy_m,
            reject_null_island: policy.reject_null_island,
            reject_mock_samples: policy.reject_mock_samples,
            reject_network_only: policy.reject_network_only,
            // `GS_LOCATION_MAX_FUTURE_SKEW_MS` and `GS_LOCATION_MAX_EVENT_AGE_MS` are server
            // parameters; the connection reports them in `limits` (docs/08 §6.1).
            max_future_skew_ms: self.config.max_future_skew_ms,
            max_event_age_ms: self.config.max_event_age.as_millis() as i64,
            max_samples_per_publish: policy.max_samples_per_publish,
        };
        (
            location,
            policy.publish_burst,
            Duration::from_millis(policy.min_publish_interval_ms),
        )
    }

    /// Delivers the accepted updates to the subscriptions of this connection, including the
    /// one of the publisher itself (docs/08 §7.3).
    ///
    /// The fan-out to the other connections of the session is T-F4-06; a publisher subscribed
    /// to its own session already sees its samples as everybody else does.
    /// Publishes the accepted updates on the bus topic of the session (docs/09 §4).
    ///
    /// Every instance, this one included, receives them through the single task of the topic
    /// and deposits them in its local subscribers, publisher included (docs/08 §7.3).
    async fn publish_to_bus(
        &self,
        state: &StreamState,
        session_id: SessionId,
        accepted: &[LocationUpdate],
    ) {
        if accepted.is_empty() {
            return;
        }
        let topic = format!("{}:bus:loc:{session_id}", state.redis_key_prefix);
        for update in accepted {
            if state
                .bus
                .publish(&topic, &update.encode_to_vec())
                .await
                .is_err()
            {
                // The bus is best-effort: the other instances converge through the
                // revalidation by TTL (docs/04 §7.6).
                tracing::debug!(
                    connection_id = %self.connection_id,
                    "bus publish failed for a location update"
                );
            }
        }
    }

    /// Handle of this connection inside the fan-out (docs/04 §8).
    fn handle(&self) -> ConnHandle {
        ConnHandle {
            connection_id: self.connection_id,
            subprotocol: self.codec.subprotocol(),
            outbound: self.outbound.clone(),
            cancel: self.cancel.clone(),
            principal: self.principal.clone(),
            session_end_tx: self.session_end_tx.clone(),
        }
    }

    /// Answers a publication error as a `PublishAck` with `request_error` (docs/08 §6.4).
    async fn reply_publish_error(
        &mut self,
        request_id: u64,
        session_id: &str,
        code: ProtoErrorCode,
        retry_after_ms: u32,
        message: &str,
    ) {
        let ack = PublishAck {
            session_id: session_id.to_string(),
            request_error: code as i32,
            results: Vec::new(),
            retry_after_ms,
        };
        let reply = ServerMessage {
            request_id,
            body: Some(ServerBody::PublishAck(ack)),
        };
        self.send(&reply).await;
        let _ = message;
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
/// The reader side is [`Connection::serve`]; a second task owns the socket and drains the
/// outbound side (docs/04 §8). When the reader ends, the writer is cancelled, given a moment to
/// flush, and the socket is closed with the documented code and its name as reason (docs/08 §8).
pub async fn run_session(session: AcceptedSession, state: Arc<StreamState>) {
    let AcceptedSession {
        subprotocol,
        grant,
        request_id: _,
        socket,
    } = session;

    let connection_id = ConnectionId::new();
    let (outbound, control_rx, latest, notify) = Outbound::new(state.config.outbound_control_queue);
    let (close_tx, close_rx) = mpsc::channel::<CloseFrame>(1);
    let cancel = CancellationToken::new();

    let mut connection = match Connection::new(
        connection_id,
        subprotocol,
        grant,
        state.config.clone(),
        outbound,
    ) {
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

    // The reader and the writer own different halves of the socket (docs/04 §8).
    let (sink, mut stream) = socket.split();
    let (active_close_tx, mut close_signal_rx) = mpsc::channel::<u16>(1);
    state.fanout.set_authz(Arc::clone(&state.authz));
    state
        .fanout
        .register_active_conn(ActiveConn {
            connection_id,
            principal: connection.principal().clone(),
            close_tx: active_close_tx,
        })
        .await;

    let writer = spawn_writer(sink, control_rx, close_rx, latest, notify, cancel.clone());
    let outcome = connection
        .serve(&mut stream, state.as_ref(), &cancel, &mut close_signal_rx)
        .await;

    for session_id in connection.subscriptions().sessions() {
        state.fanout.unsubscribe(session_id, connection_id);
    }
    state.fanout.unregister_active_conn(connection_id);

    // Presence cleanup and disconnection grace (FR-PRS-002, docs/04 §6.3)
    if matches!(connection.grant.principal_kind, TicketPrincipalKind::Device)
        && let Some(device_id) = connection.grant.device_id
    {
        let presence_key = format!("{}:presence:{}", state.redis_key_prefix, device_id);
        let conn_id_str = connection_id.to_string();
        let released = state
            .ephemeral
            .release_presence(&presence_key, &conn_id_str)
            .await
            .unwrap_or(false);

        if released {
            let state_clone = Arc::clone(&state);
            let tenant_id = connection.grant.tenant_id;
            let offline_grace = state.config.offline_grace;
            tokio::spawn(async move {
                tokio::time::sleep(offline_grace).await;
                if let Ok(None) = state_clone.ephemeral.get_presence(&presence_key).await
                    && let Ok(publishers) =
                        list_active_publishing_by_device(&state_clone.pool, tenant_id, device_id)
                            .await
                {
                    let now_str = format_rfc3339(OffsetDateTime::now_utc());
                    for pub_member in publishers {
                        if pub_member.membership.sharing {
                            let proto_status = to_proto_member_status(
                                pub_member.membership.session_id,
                                pub_member.membership.id,
                                &pub_member.external_subject_id,
                                pub_member.membership.role,
                                pub_member.membership.capabilities,
                                true,
                                Connectivity::Offline,
                                None,
                            );
                            let event = SessionControlEvent::MemberStatusChanged {
                                session_id: pub_member.membership.session_id.to_string(),
                                member: proto_status,
                                at: now_str.clone(),
                            };
                            let _ = publish_session_control(
                                state_clone.bus.as_ref(),
                                &state_clone.redis_key_prefix,
                                pub_member.membership.session_id,
                                &event,
                            )
                            .await;
                        }
                    }
                }
            });
        }
    }

    // The close frame is written by the owner of the socket, before it goes away (docs/08 §8).
    let close = CloseFrame {
        code: outcome.code,
        reason: crate::close_codes::CloseCode::new(outcome.code)
            .name()
            .into(),
    };
    let _ = tokio::time::timeout(Duration::from_secs(1), close_tx.send(close)).await;
    // Dropping the sender tells the writer that nothing else is coming, so it writes what is
    // left and ends by itself; only then is the token cancelled.
    drop(close_tx);
    let _ = tokio::time::timeout(Duration::from_secs(5), writer).await;
    cancel.cancel();

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
}

/// Spawns the writer task of a connection (docs/04 §8).
///
/// Control frames have priority over the coalesced locations, as docs/04 §8 requires: the
/// writer drains everything pending in the control queue before looking at `latest`, so a
/// response is never delayed by a burst of updates (docs/08 §10).
fn spawn_writer(
    sink: futures::stream::SplitSink<WebSocket, Message>,
    mut control_rx: mpsc::Receiver<Frame>,
    mut close_rx: mpsc::Receiver<CloseFrame>,
    latest: Arc<std::sync::Mutex<LatestMap>>,
    notify: Arc<Notify>,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut sink = sink;
        loop {
            // 1. Everything already queued as control goes out first (docs/04 §8).
            while let Ok(frame) = control_rx.try_recv() {
                if !write_frame(&mut sink, &frame).await {
                    cancel.cancel();
                    return;
                }
            }

            tokio::select! {
                biased;
                // Control frames are never discarded (docs/08 §10).
                frame = control_rx.recv() => {
                    let Some(frame) = frame else {
                        cancel.cancel();
                        return;
                    };
                    if !write_frame(&mut sink, &frame).await {
                        cancel.cancel();
                        return;
                    }
                }
                // The close frame ends the connection (docs/08 §8).
                close = close_rx.recv() => {
                    let Some(close) = close else {
                        return;
                    };
                    // Drain everything already queued as control before closing.
                    // docs/08 §6.6 promises the client a correlated `StreamError`
                    // before the close frame; without this drain, a select that
                    // happens to take the close branch loses the error and the
                    // client sees only the code. The `biased;` above makes this
                    // branch ordering deterministic instead of leaving it to the
                    // random choice of a non-biased select.
                    while let Ok(frame) = control_rx.try_recv() {
                        if !write_frame(&mut sink, &frame).await {
                            cancel.cancel();
                            return;
                        }
                    }
                    let _ = sink.send(Message::Close(Some(close))).await;
                    return;
                }
                // Coalesced locations: only the newest per (session, member) is written.
                _ = notify.notified() => {
                    let pending: Vec<Frame> = {
                        let Ok(mut latest) = latest.lock() else {
                            cancel.cancel();
                            return;
                        };
                        latest.drain().map(|(_, frame)| frame).collect()
                    };
                    for frame in pending {
                        if !write_frame(&mut sink, &frame).await {
                            cancel.cancel();
                            return;
                        }
                    }
                }
                _ = cancel.cancelled() => return,
            }
        }
    })
}

/// Writes one frame, reporting whether the socket accepted it.
async fn write_frame(sink: &mut SplitSink<WebSocket, Message>, frame: &Frame) -> bool {
    let message = if frame.text {
        Message::Text(String::from_utf8_lossy(&frame.payload).into_owned().into())
    } else {
        Message::Binary(frame.payload.clone().into())
    };
    sink.send(message).await.is_ok()
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
        Some(ServerBody::TrackEvent(_)) => "track_event",
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
            core_version: String::new(),
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
        let (outbound, _control, _latest, _notify) = Outbound::new(8);
        let connection = Connection::new(
            ConnectionId::new(),
            Subprotocol::Proto,
            grant,
            StreamConfig::default(),
            outbound,
        )
        .expect("connection");
        assert_eq!(connection.principal_kind(), PrincipalKind::Device);
        assert_eq!(connection.state(), ConnectionState::AwaitingHello);
        assert_eq!(connection.subprotocol(), Subprotocol::Proto);
    }
}
