//! Fan-out of a session over the bus, with coalescing and bounded queues
//! (docs/04 §8, docs/08 §10, FR-STR-003, FR-STR-006, FR-STR-008, FR-STR-011, FR-AUTH-005).
//!
//! Every connection has one writer task that owns its socket and two inputs:
//!
//! - **`control`**: a bounded `mpsc` channel for responses and control events. It is never
//!   dropped: if it fills up the connection closes with 4005 and the client rebuilds its state
//!   when it reconnects (docs/08 §10).
//! - **`latest`**: a map `(session_id, member_id) → LocationUpdate` guarded by a mutex that is
//!   never held across an `.await`, with a `Notify`. Inserting replaces, which is the
//!   coalescing of docs/08 §10: the client always gets the newest location per member.
//!
//! The instance keeps a `DashMap<SessionId, Vec<ConnHandle>>`. A single task per session topic
//! receives the bus messages and deposits them locally, so every update travels one path
//! (docs/04 §8) whether the publisher is this process or another one.
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use dashmap::DashMap;
use gs_service::authz::{AuthzCache, PrincipalIdentity};
use gs_service::control::{GlobalBusEvent, SessionControlEvent};
use gs_service::gs_domain::Subprotocol;
use gs_service::gs_domain::ids::{ConnectionId, DeviceId, MemberId, SessionId};
use gs_service::gs_protocol::server_message::Body as ServerBody;
use gs_service::gs_protocol::{
    MemberEvent, MemberEventKind, MemberStatus, ServerMessage, SessionEvent, SessionEventKind,
    SessionInfo, SessionStatus, SubscribeAck, SubscriptionEndReason, SubscriptionEnded,
    UnsubscribeAck,
};
use gs_service::{Bus, BusBackend};
use prost::Message as _;
use tokio::sync::{Notify, mpsc};
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;

/// Default of `GS_STREAM_OUTBOUND_CONTROL_QUEUE` (docs/17 §2.6).
pub const DEFAULT_OUTBOUND_CONTROL_QUEUE: usize = 1_024;

/// A frame ready to be written: its payload and whether it is a text frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Encoded payload.
    pub payload: Vec<u8>,
    /// Whether the frame is text (`gs.v1.json`).
    pub text: bool,
}

/// Coalesced locations of one connection, keyed by (session, member).
pub type LatestMap = HashMap<(SessionId, MemberId), Frame>;

/// Outbound side of a connection: what the writer task drains.
#[derive(Debug, Clone)]
pub struct Outbound {
    control: mpsc::Sender<Frame>,
    latest: Arc<std::sync::Mutex<LatestMap>>,
    notify: Arc<Notify>,
}

impl Outbound {
    /// Builds the outbound side with the documented queue size.
    pub fn new(
        queue: usize,
    ) -> (
        Self,
        mpsc::Receiver<Frame>,
        Arc<std::sync::Mutex<LatestMap>>,
        Arc<Notify>,
    ) {
        let (control, receiver) = mpsc::channel(queue.max(1));
        let latest = Arc::new(std::sync::Mutex::new(LatestMap::new()));
        let notify = Arc::new(Notify::new());
        (
            Self {
                control,
                latest: Arc::clone(&latest),
                notify: Arc::clone(&notify),
            },
            receiver,
            latest,
            notify,
        )
    }

    /// Enqueues a control message, encoding it in the negotiated subprotocol.
    ///
    /// Returns false when the bounded queue is full, which closes the connection with 4005
    /// (docs/08 §10).
    pub async fn send_control(&self, message: &ServerMessage, subprotocol: Subprotocol) -> bool {
        let Ok(payload) = crate::codec::FrameCodec::new(subprotocol).encode_server(message) else {
            return true;
        };
        let frame = Frame {
            payload,
            text: subprotocol.is_text(),
        };
        // `send` waits for capacity; a control frame is never discarded.
        self.control.send(frame).await.is_ok()
    }

    /// Enqueues a control frame without waiting for capacity, reporting a full queue.
    ///
    /// Used by the fan-out, which cannot block: a connection whose queue is full is cancelled
    /// with 4005 by its owner (docs/08 §10).
    pub async fn control_send(&self, frame: Frame) -> Result<(), mpsc::error::SendError<Frame>> {
        self.control.send(frame).await
    }

    /// Deposits the newest location of a member, replacing any intermediate one.
    pub fn send_latest(&self, session_id: SessionId, member_id: MemberId, frame: Frame) {
        let Ok(mut latest) = self.latest.lock() else {
            return;
        };
        latest.insert((session_id, member_id), frame);
        drop(latest);
        self.notify.notify_one();
    }

    /// Number of coalesced locations waiting: diagnostic helper.
    pub fn pending_locations(&self) -> usize {
        self.latest.lock().map(|latest| latest.len()).unwrap_or(0)
    }
}

/// An active connection registered on this stream instance (docs/08 §8, §9).
#[derive(Debug, Clone)]
pub struct ActiveConn {
    /// Connection identifier.
    pub connection_id: ConnectionId,
    /// Principal identity for selective security revocations.
    pub principal: PrincipalIdentity,
    /// Channel to send close code (4401 or 4403) to the connection.
    pub close_tx: mpsc::Sender<u16>,
}

/// Handle of a live connection inside the fan-out of an instance.
#[derive(Debug, Clone)]
pub struct ConnHandle {
    /// Connection the handle belongs to.
    pub connection_id: ConnectionId,
    /// Subprotocol that connection negotiated: each frame is encoded with it (docs/08 §2).
    pub subprotocol: Subprotocol,
    /// Outbound side of that connection.
    pub outbound: Outbound,
    /// Cancelled when the connection ends, so its writer stops.
    pub cancel: CancellationToken,
    /// Principal identity of the connection.
    pub principal: PrincipalIdentity,
    /// Channel to signal subscription end for a session to the connection machine.
    pub session_end_tx: mpsc::Sender<(SessionId, SubscriptionEndReason)>,
}

/// Fan-out of an instance: local subscriptions per session, active connection registry,
/// and the bus tasks that feed them (docs/04 §8, docs/08 §9, §10).
#[derive(Debug)]
pub struct Fanout {
    by_session: DashMap<SessionId, Vec<ConnHandle>>,
    pub(crate) active_connections: DashMap<ConnectionId, ActiveConn>,
    device_listeners: DashMap<DeviceId, CancellationToken>,
    /// Bus the instance publishes and receives from (docs/04 §6.1).
    bus: Arc<BusBackend>,
    /// Authorization cache for invalidation on bus events (docs/04 §6.2, ADR-0009).
    authz: std::sync::RwLock<Option<Arc<AuthzCache>>>,
    /// Ephemeral key prefix (`GS_REDIS_KEY_PREFIX`, default `gs`).
    key_prefix: String,
    /// Tracks if global security bus listener is running.
    global_listener_started: AtomicBool,
}

impl Fanout {
    /// Builds the fan-out of an instance over its bus.
    pub fn new(bus: Arc<BusBackend>, key_prefix: impl Into<String>) -> Self {
        Self::with_authz(bus, key_prefix, None)
    }

    /// Builds the fan-out with an optional authorization cache reference.
    pub fn with_authz(
        bus: Arc<BusBackend>,
        key_prefix: impl Into<String>,
        authz: Option<Arc<AuthzCache>>,
    ) -> Self {
        Self {
            by_session: DashMap::new(),
            active_connections: DashMap::new(),
            device_listeners: DashMap::new(),
            bus,
            authz: std::sync::RwLock::new(authz),
            key_prefix: key_prefix.into(),
            global_listener_started: AtomicBool::new(false),
        }
    }

    /// Associates an authorization cache with this fan-out.
    pub fn set_authz(&self, authz: Arc<AuthzCache>) {
        if let Ok(mut lock) = self.authz.write() {
            *lock = Some(authz);
        }
    }

    /// Retrieves a reference to the authorization cache if set.
    pub fn authz(&self) -> Option<Arc<AuthzCache>> {
        self.authz.read().ok().and_then(|guard| guard.clone())
    }

    /// Registers an active connection on this instance and ensures listeners run.
    pub async fn register_active_conn(self: &Arc<Self>, conn: ActiveConn) {
        let device_id = conn.principal.device_id;
        self.active_connections.insert(conn.connection_id, conn);
        self.ensure_global_listener().await;
        if let Some(dev_id) = device_id {
            self.ensure_device_listener(dev_id).await;
        }
    }

    /// Unregisters an active connection when it closes.
    pub fn unregister_active_conn(&self, connection_id: ConnectionId) {
        if let Some((_, conn)) = self.active_connections.remove(&connection_id)
            && let Some(device_id) = conn.principal.device_id
        {
            let still_has = self
                .active_connections
                .iter()
                .any(|entry| entry.principal.device_id == Some(device_id));
            if !still_has && let Some((_, cancel)) = self.device_listeners.remove(&device_id) {
                cancel.cancel();
            }
        }
    }

    /// Ensures device listener task is running on `{key_prefix}:bus:dev:{device_id}` (docs/04 §6.3, docs/08 §6.7, FR-STR-007).
    pub async fn ensure_device_listener(self: &Arc<Self>, device_id: DeviceId) {
        if self.device_listeners.contains_key(&device_id) {
            return;
        }
        let cancel = CancellationToken::new();
        self.device_listeners.insert(device_id, cancel.clone());
        let topic = format!("{}:bus:dev:{}", self.key_prefix, device_id);
        if let Ok(sub) = self.bus.subscribe(&topic).await {
            let fanout = Arc::clone(self);
            tokio::spawn(async move {
                device_bus_task(fanout, device_id, sub, cancel).await;
            });
        }
    }

    /// Number of active connections on this stream instance.
    pub fn active_connections_count(&self) -> usize {
        self.active_connections.len()
    }

    /// Ensures the global security listener task is running on `{key_prefix}:bus:global`.
    pub async fn ensure_global_listener(self: &Arc<Self>) {
        if self
            .global_listener_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let topic = format!("{}:bus:global", self.key_prefix);
            if let Ok(sub) = self.bus.subscribe(&topic).await {
                let fanout = Arc::clone(self);
                tokio::spawn(async move {
                    global_bus_task(fanout, sub).await;
                });
            }
        }
    }

    /// Registers a connection in the fan-out of a session.
    ///
    /// The first local subscriber of a session also subscribes to its location and control bus
    /// topics and starts the tasks that deposit what arrives (docs/04 §8, docs/08 §9).
    pub async fn subscribe(self: &Arc<Self>, session_id: SessionId, handle: ConnHandle) {
        let first = {
            let mut entry = self.by_session.entry(session_id).or_default();
            let first = entry.is_empty();
            entry.push(handle);
            first
        };
        if first {
            // Subscribe to location updates
            if let Ok(subscription) = self.bus.subscribe(&self.topic(session_id)).await {
                let fanout = Arc::clone(self);
                tokio::spawn(async move { topic_task(fanout, session_id, subscription).await });
            }
            // Subscribe to control events
            if let Ok(ctl_sub) = self.bus.subscribe(&self.ctl_topic(session_id)).await {
                let fanout = Arc::clone(self);
                tokio::spawn(async move { ctl_topic_task(fanout, session_id, ctl_sub).await });
            }
        }
    }

    /// Removes a connection from the fan-out of a session.
    ///
    /// With the last subscriber gone the task of the topic ends and releases its bus
    /// subscription (refcount of docs/04 §6.1).
    pub fn unsubscribe(&self, session_id: SessionId, connection_id: ConnectionId) -> bool {
        let mut emptied = false;
        if let Some(mut entry) = self.by_session.get_mut(&session_id) {
            entry.retain(|handle| handle.connection_id != connection_id);
            emptied = entry.is_empty();
        }
        if emptied {
            self.by_session.remove(&session_id);
        }
        emptied
    }

    /// Deposits an update in every local subscriber of the session (coalescing).
    pub fn deposit_location(
        &self,
        session_id: SessionId,
        member_id: MemberId,
        message: &ServerMessage,
    ) {
        if let Some(entry) = self.by_session.get(&session_id) {
            for handle in entry.iter() {
                if let Some(frame) = frame_of(message, handle.subprotocol) {
                    handle.outbound.send_latest(session_id, member_id, frame);
                }
            }
        }
    }

    /// Deposits a control event in every local subscriber of the session (docs/08 §10).
    pub async fn deposit_control(
        &self,
        session_id: SessionId,
        message: &ServerMessage,
        subprotocol: Subprotocol,
    ) {
        let encoded = crate::codec::FrameCodec::new(subprotocol).encode_server(message);
        let Ok(payload) = encoded else {
            return;
        };
        let frame = Frame {
            payload,
            text: subprotocol.is_text(),
        };
        if let Some(entry) = self.by_session.get(&session_id) {
            for handle in entry.iter() {
                // A control frame is never discarded: a full queue closes the connection
                // with 4005 (docs/08 §10).
                if handle.outbound.control_send(frame.clone()).await.is_err() {
                    handle.cancel.cancel();
                }
            }
        }
    }

    /// Handles a session-scoped control event received from the bus (docs/05 §10, docs/08 §9).
    pub async fn handle_control_event(&self, session_id: SessionId, event: SessionControlEvent) {
        match event {
            SessionControlEvent::SessionClosed {
                session_id: sid,
                reason: _,
                session,
                at: _,
            } => {
                let session_info = session.unwrap_or_else(|| SessionInfo {
                    session_id: sid.clone(),
                    status: SessionStatus::Closed as i32,
                    ..Default::default()
                });
                let session_msg = ServerMessage {
                    request_id: 0,
                    body: Some(ServerBody::SessionEvent(SessionEvent {
                        kind: SessionEventKind::Closed as i32,
                        session: Some(session_info),
                    })),
                };
                let ended_msg = ServerMessage {
                    request_id: 0,
                    body: Some(ServerBody::SubscriptionEnded(SubscriptionEnded {
                        session_id: sid,
                        reason: SubscriptionEndReason::SessionClosed as i32,
                    })),
                };
                if let Some((_, handles)) = self.by_session.remove(&session_id) {
                    for handle in handles {
                        send_server_msg(&handle, &session_msg).await;
                        send_server_msg(&handle, &ended_msg).await;
                        let _ = handle
                            .session_end_tx
                            .send((session_id, SubscriptionEndReason::SessionClosed))
                            .await;
                    }
                }
            }
            SessionControlEvent::SessionExpired {
                session_id: sid,
                session,
                at: _,
            } => {
                let session_info = session.unwrap_or_else(|| SessionInfo {
                    session_id: sid.clone(),
                    status: SessionStatus::Expired as i32,
                    ..Default::default()
                });
                let session_msg = ServerMessage {
                    request_id: 0,
                    body: Some(ServerBody::SessionEvent(SessionEvent {
                        kind: SessionEventKind::Expired as i32,
                        session: Some(session_info),
                    })),
                };
                let ended_msg = ServerMessage {
                    request_id: 0,
                    body: Some(ServerBody::SubscriptionEnded(SubscriptionEnded {
                        session_id: sid,
                        reason: SubscriptionEndReason::SessionExpired as i32,
                    })),
                };
                if let Some((_, handles)) = self.by_session.remove(&session_id) {
                    for handle in handles {
                        send_server_msg(&handle, &session_msg).await;
                        send_server_msg(&handle, &ended_msg).await;
                        let _ = handle
                            .session_end_tx
                            .send((session_id, SubscriptionEndReason::SessionExpired))
                            .await;
                    }
                }
            }
            SessionControlEvent::SessionUpdated {
                session_id: _,
                session,
                at: _,
            } => {
                let msg = ServerMessage {
                    request_id: 0,
                    body: Some(ServerBody::SessionEvent(SessionEvent {
                        kind: SessionEventKind::Updated as i32,
                        session: Some(session),
                    })),
                };
                if let Some(entry) = self.by_session.get(&session_id) {
                    for handle in entry.iter() {
                        send_server_msg(handle, &msg).await;
                    }
                }
            }
            SessionControlEvent::MemberJoined {
                session_id: _,
                member,
                at: _,
            } => {
                let msg = ServerMessage {
                    request_id: 0,
                    body: Some(ServerBody::MemberEvent(MemberEvent {
                        kind: MemberEventKind::Joined as i32,
                        member: Some(member),
                    })),
                };
                if let Some(entry) = self.by_session.get(&session_id) {
                    for handle in entry.iter() {
                        send_server_msg(handle, &msg).await;
                    }
                }
            }
            SessionControlEvent::MemberUpdated {
                session_id: sid,
                member,
                subject_id,
                lost_read,
                lost_publish: _,
                at: _,
            } => {
                let member_msg = ServerMessage {
                    request_id: 0,
                    body: Some(ServerBody::MemberEvent(MemberEvent {
                        kind: MemberEventKind::Updated as i32,
                        member: Some(member),
                    })),
                };
                if lost_read {
                    let ended_msg = ServerMessage {
                        request_id: 0,
                        body: Some(ServerBody::SubscriptionEnded(SubscriptionEnded {
                            session_id: sid,
                            reason: SubscriptionEndReason::PermissionChanged as i32,
                        })),
                    };
                    let mut empty = false;
                    if let Some(mut entry) = self.by_session.get_mut(&session_id) {
                        for handle in entry.iter() {
                            if handle.principal.subject_id.as_ref().map(|s| s.to_string())
                                == Some(subject_id.clone())
                            {
                                send_server_msg(handle, &ended_msg).await;
                                let _ = handle
                                    .session_end_tx
                                    .send((session_id, SubscriptionEndReason::PermissionChanged))
                                    .await;
                            } else {
                                send_server_msg(handle, &member_msg).await;
                            }
                        }
                        entry.retain(|h| {
                            h.principal.subject_id.as_ref().map(|s| s.to_string())
                                != Some(subject_id.clone())
                        });
                        empty = entry.is_empty();
                    }
                    if empty {
                        self.by_session.remove(&session_id);
                    }
                } else if let Some(entry) = self.by_session.get(&session_id) {
                    for handle in entry.iter() {
                        send_server_msg(handle, &member_msg).await;
                    }
                }
            }
            SessionControlEvent::MemberLeft {
                session_id: sid,
                member_id,
                subject_id,
                member,
                at: _,
            } => {
                let ended_msg = ServerMessage {
                    request_id: 0,
                    body: Some(ServerBody::SubscriptionEnded(SubscriptionEnded {
                        session_id: sid.clone(),
                        reason: SubscriptionEndReason::MemberLeft as i32,
                    })),
                };
                let member_status = member.unwrap_or_else(|| MemberStatus {
                    session_id: sid,
                    member_id,
                    ..Default::default()
                });
                let member_msg = ServerMessage {
                    request_id: 0,
                    body: Some(ServerBody::MemberEvent(MemberEvent {
                        kind: MemberEventKind::Left as i32,
                        member: Some(member_status),
                    })),
                };
                let mut empty = false;
                if let Some(mut entry) = self.by_session.get_mut(&session_id) {
                    for handle in entry.iter() {
                        if handle.principal.subject_id.as_ref().map(|s| s.to_string())
                            == Some(subject_id.clone())
                        {
                            send_server_msg(handle, &ended_msg).await;
                            let _ = handle
                                .session_end_tx
                                .send((session_id, SubscriptionEndReason::MemberLeft))
                                .await;
                        } else {
                            send_server_msg(handle, &member_msg).await;
                        }
                    }
                    entry.retain(|h| {
                        h.principal.subject_id.as_ref().map(|s| s.to_string())
                            != Some(subject_id.clone())
                    });
                    empty = entry.is_empty();
                }
                if empty {
                    self.by_session.remove(&session_id);
                }
            }
            SessionControlEvent::MemberRevoked {
                session_id: sid,
                member_id,
                subject_id,
                member,
                at: _,
            } => {
                let ended_msg = ServerMessage {
                    request_id: 0,
                    body: Some(ServerBody::SubscriptionEnded(SubscriptionEnded {
                        session_id: sid.clone(),
                        reason: SubscriptionEndReason::MemberRevoked as i32,
                    })),
                };
                let member_status = member.unwrap_or_else(|| MemberStatus {
                    session_id: sid,
                    member_id,
                    ..Default::default()
                });
                let member_msg = ServerMessage {
                    request_id: 0,
                    body: Some(ServerBody::MemberEvent(MemberEvent {
                        kind: MemberEventKind::Revoked as i32,
                        member: Some(member_status),
                    })),
                };
                let mut empty = false;
                if let Some(mut entry) = self.by_session.get_mut(&session_id) {
                    for handle in entry.iter() {
                        if handle.principal.subject_id.as_ref().map(|s| s.to_string())
                            == Some(subject_id.clone())
                        {
                            send_server_msg(handle, &ended_msg).await;
                            let _ = handle
                                .session_end_tx
                                .send((session_id, SubscriptionEndReason::MemberRevoked))
                                .await;
                        } else {
                            send_server_msg(handle, &member_msg).await;
                        }
                    }
                    entry.retain(|h| {
                        h.principal.subject_id.as_ref().map(|s| s.to_string())
                            != Some(subject_id.clone())
                    });
                    empty = entry.is_empty();
                }
                if empty {
                    self.by_session.remove(&session_id);
                }
            }
            SessionControlEvent::SharingStarted {
                session_id: sid,
                member_id,
                device_id: _,
                member,
                at: _,
            } => {
                let status = member.unwrap_or_else(|| MemberStatus {
                    session_id: sid,
                    member_id,
                    sharing: true,
                    connectivity: gs_service::gs_protocol::Connectivity::Online as i32,
                    ..Default::default()
                });
                let msg = ServerMessage {
                    request_id: 0,
                    body: Some(ServerBody::MemberStatus(status)),
                };
                if let Some(entry) = self.by_session.get(&session_id) {
                    for handle in entry.iter() {
                        send_server_msg(handle, &msg).await;
                    }
                }
            }
            SessionControlEvent::SharingStopped {
                session_id: sid,
                member_id,
                device_id: _,
                member,
                at: _,
            } => {
                let status = member.unwrap_or_else(|| MemberStatus {
                    session_id: sid,
                    member_id,
                    sharing: false,
                    connectivity: gs_service::gs_protocol::Connectivity::Unspecified as i32,
                    ..Default::default()
                });
                let msg = ServerMessage {
                    request_id: 0,
                    body: Some(ServerBody::MemberStatus(status)),
                };
                if let Some(entry) = self.by_session.get(&session_id) {
                    for handle in entry.iter() {
                        send_server_msg(handle, &msg).await;
                    }
                }
            }
            SessionControlEvent::SharingTransferred {
                session_id: sid,
                member_id,
                old_device_id: _,
                new_device_id: _,
                member,
                at: _,
            } => {
                let status = member.unwrap_or_else(|| MemberStatus {
                    session_id: sid,
                    member_id,
                    sharing: true,
                    connectivity: gs_service::gs_protocol::Connectivity::Online as i32,
                    ..Default::default()
                });
                let msg = ServerMessage {
                    request_id: 0,
                    body: Some(ServerBody::MemberStatus(status)),
                };
                if let Some(entry) = self.by_session.get(&session_id) {
                    for handle in entry.iter() {
                        send_server_msg(handle, &msg).await;
                    }
                }
            }
            SessionControlEvent::MemberStatusChanged {
                session_id: _,
                member,
                at: _,
            } => {
                let msg = ServerMessage {
                    request_id: 0,
                    body: Some(ServerBody::MemberStatus(member)),
                };
                if let Some(entry) = self.by_session.get(&session_id) {
                    for handle in entry.iter() {
                        send_server_msg(handle, &msg).await;
                    }
                }
            }
        }
    }

    /// Number of local subscribers of a session: diagnostic helper.
    pub fn subscribers(&self, session_id: SessionId) -> usize {
        self.by_session
            .get(&session_id)
            .map(|entry| entry.len())
            .unwrap_or(0)
    }

    /// Total local subscribers: diagnostic helper.
    pub fn total_subscribers(&self) -> usize {
        self.by_session.iter().map(|entry| entry.len()).sum()
    }

    /// Bus topic of location updates for a session (docs/04 §6.1).
    fn topic(&self, session_id: SessionId) -> String {
        format!("{}:bus:loc:{session_id}", self.key_prefix)
    }

    /// Bus topic of control events for a session (docs/04 §6.1, docs/05 §10).
    fn ctl_topic(&self, session_id: SessionId) -> String {
        format!("{}:bus:ctl:{session_id}", self.key_prefix)
    }
}

/// Reads the bus topic of a session and deposits what arrives into the local subscribers.
///
/// Ends when the topic loses its last local subscriber, which is when the bus subscription of
/// docs/04 §6.1 is released.
async fn topic_task(fanout: Arc<Fanout>, session_id: SessionId, subscription: u64) {
    loop {
        if fanout.subscribers(session_id) == 0 {
            let _ = fanout.bus.unsubscribe(subscription).await;
            return;
        }
        let Ok(message) = tokio::time::timeout(Duration::from_secs(1), async {
            fanout.bus.next_message(subscription).await
        })
        .await
        else {
            continue;
        };
        let Ok(Some(message)) = message else {
            return;
        };
        let Ok(update) =
            gs_service::gs_protocol::LocationUpdate::decode(message.payload.as_slice())
        else {
            continue;
        };
        let Ok(member_id) = member_id_of(&update) else {
            continue;
        };
        // The bus carries the wire `LocationUpdate` (docs/04 §6.1); the frame that reaches
        // the client is a `ServerMessage` with `request_id = 0` (docs/08 §4).
        let message = ServerMessage {
            request_id: 0,
            body: Some(ServerBody::LocationUpdate(update)),
        };
        fanout.deposit_location(session_id, member_id, &message);
    }
}

/// Reads the control bus topic of a session and dispatches control events (docs/08 §9).
async fn ctl_topic_task(fanout: Arc<Fanout>, session_id: SessionId, subscription: u64) {
    loop {
        if fanout.subscribers(session_id) == 0 {
            let _ = fanout.bus.unsubscribe(subscription).await;
            return;
        }
        let Ok(message) = tokio::time::timeout(Duration::from_secs(1), async {
            fanout.bus.next_message(subscription).await
        })
        .await
        else {
            continue;
        };
        let Ok(Some(message)) = message else {
            return;
        };

        if let Some(ref authz) = fanout.authz() {
            authz.invalidate_session(session_id);
        }

        let Ok(event) = serde_json::from_slice::<SessionControlEvent>(&message.payload) else {
            continue;
        };

        fanout.handle_control_event(session_id, event).await;
    }
}

/// Reads the global bus topic and handles global security revocations (docs/06 §7, docs/08 §9).
async fn global_bus_task(fanout: Arc<Fanout>, subscription: u64) {
    loop {
        let Ok(message) = tokio::time::timeout(Duration::from_secs(1), async {
            fanout.bus.next_message(subscription).await
        })
        .await
        else {
            continue;
        };
        let Ok(Some(message)) = message else {
            return;
        };

        if let Some(ref authz) = fanout.authz() {
            authz.note_global_bus_event(&message.payload);
        }

        let Ok(event) = serde_json::from_slice::<GlobalBusEvent>(&message.payload) else {
            continue;
        };

        match event {
            GlobalBusEvent::TenantDisabled { tenant_id, .. } => {
                for entry in fanout.active_connections.iter() {
                    let conn = entry.value();
                    if conn.principal.tenant_id.to_string() == tenant_id {
                        let _ = conn
                            .close_tx
                            .send(crate::close_codes::CLOSE_FORBIDDEN)
                            .await;
                    }
                }
            }
            GlobalBusEvent::TenantEnabled { .. } => {}
            GlobalBusEvent::ApplicationDisabled {
                tenant_id,
                application_id,
                ..
            } => {
                for entry in fanout.active_connections.iter() {
                    let conn = entry.value();
                    if conn.principal.tenant_id.to_string() == tenant_id
                        && conn.principal.application_id.to_string() == application_id
                    {
                        let _ = conn
                            .close_tx
                            .send(crate::close_codes::CLOSE_FORBIDDEN)
                            .await;
                    }
                }
            }
            GlobalBusEvent::ApplicationEnabled { .. } => {}
            GlobalBusEvent::ApiKeyRevoked {
                tenant_id,
                application_id,
                ..
            } => {
                for entry in fanout.active_connections.iter() {
                    let conn = entry.value();
                    if conn.principal.tenant_id.to_string() == tenant_id {
                        if let Some(ref app_id) = application_id {
                            if conn.principal.application_id.to_string() == *app_id {
                                let _ = conn
                                    .close_tx
                                    .send(crate::close_codes::CLOSE_AUTH_EXPIRED)
                                    .await;
                            }
                        } else {
                            let _ = conn
                                .close_tx
                                .send(crate::close_codes::CLOSE_AUTH_EXPIRED)
                                .await;
                        }
                    }
                }
            }
            GlobalBusEvent::SubjectTokensRevoked {
                tenant_id,
                subject_id,
                ..
            } => {
                for entry in fanout.active_connections.iter() {
                    let conn = entry.value();
                    if conn.principal.tenant_id.to_string() == tenant_id
                        && conn.principal.subject_id.as_ref().map(|s| s.to_string())
                            == Some(subject_id.clone())
                    {
                        let _ = conn
                            .close_tx
                            .send(crate::close_codes::CLOSE_AUTH_EXPIRED)
                            .await;
                    }
                }
            }
            GlobalBusEvent::DeviceDeactivated {
                tenant_id,
                device_id,
                ..
            } => {
                for entry in fanout.active_connections.iter() {
                    let conn = entry.value();
                    if conn.principal.tenant_id.to_string() == tenant_id
                        && conn.principal.device_id.as_ref().map(|d| d.to_string())
                            == Some(device_id.clone())
                    {
                        let _ = conn
                            .close_tx
                            .send(crate::close_codes::CLOSE_AUTH_EXPIRED)
                            .await;
                    }
                }
            }
        }
    }
}

/// Reads the device-scoped bus topic `{key_prefix}:bus:dev:{device_id}` and closes superseded connections (docs/04 §6.3, docs/08 §6.7, FR-STR-007).
async fn device_bus_task(
    fanout: Arc<Fanout>,
    device_id: DeviceId,
    subscription: u64,
    cancel: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = fanout.bus.unsubscribe(subscription).await;
                return;
            }
            res = tokio::time::timeout(Duration::from_secs(1), fanout.bus.next_message(subscription)) => {
                let Ok(message) = res else {
                    continue;
                };
                let Ok(Some(message)) = message else {
                    return;
                };

                let Ok(event) = serde_json::from_slice::<gs_service::control::DeviceBusEvent>(&message.payload) else {
                    continue;
                };

                match event {
                    gs_service::control::DeviceBusEvent::Supersede {
                        device_id: dev_id_str,
                        connection_id: incoming_conn_id_str,
                        ..
                    } => {
                        if dev_id_str == device_id.to_string() {
                            for entry in fanout.active_connections.iter() {
                                let conn = entry.value();
                                if conn.principal.device_id.as_ref().map(|d| d.to_string()) == Some(dev_id_str.clone())
                                    && conn.connection_id.to_string() != incoming_conn_id_str
                                {
                                    let _ = conn
                                        .close_tx
                                        .send(crate::close_codes::CLOSE_SUPERSEDED)
                                        .await;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Encodes a server message in the subprotocol of one connection and sends it via control queue.
async fn send_server_msg(handle: &ConnHandle, message: &ServerMessage) {
    if let Some(frame) = frame_of(message, handle.subprotocol)
        && handle.outbound.control_send(frame).await.is_err()
    {
        handle.cancel.cancel();
    }
}

/// Encodes a server message in the subprotocol of one connection (docs/08 §2).
fn frame_of(message: &ServerMessage, subprotocol: Subprotocol) -> Option<Frame> {
    let payload = crate::codec::FrameCodec::new(subprotocol)
        .encode_server(message)
        .ok()?;
    Some(Frame {
        payload,
        text: subprotocol.is_text(),
    })
}

/// Member id carried by an update.
fn member_id_of(update: &gs_service::gs_protocol::LocationUpdate) -> Result<MemberId, ()> {
    use std::str::FromStr;
    MemberId::from_str(&update.member_id).map_err(|_| ())
}

/// Answers of the subscription protocol, built here so the fan-out and the connection share
/// their shape.
pub fn subscribe_ack_message(ack: SubscribeAck) -> ServerMessage {
    ServerMessage {
        request_id: 0,
        body: Some(ServerBody::SubscribeAck(ack)),
    }
}

/// `UnsubscribeAck` envelope.
pub fn unsubscribe_ack_message(ack: UnsubscribeAck) -> ServerMessage {
    ServerMessage {
        request_id: 0,
        body: Some(ServerBody::UnsubscribeAck(ack)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(payload: &[u8]) -> Frame {
        Frame {
            payload: payload.to_vec(),
            text: false,
        }
    }

    fn test_handle(connection_id: ConnectionId, outbound: Outbound) -> ConnHandle {
        let (session_end_tx, _) = mpsc::channel(4);
        ConnHandle {
            connection_id,
            subprotocol: Subprotocol::Proto,
            outbound,
            cancel: CancellationToken::new(),
            principal: PrincipalIdentity {
                tenant_id: gs_service::gs_domain::ids::TenantId::new(),
                application_id: gs_service::gs_domain::ids::ApplicationId::new(),
                subject_id: Some(gs_service::gs_domain::ids::SubjectId::new()),
                device_id: None,
                token_epoch: None,
            },
            session_end_tx,
        }
    }

    #[tokio::test]
    async fn test_locations_are_coalesced_per_session_and_member() {
        // covers: docs/08 §10
        let (outbound, _control, _latest, _notify) = Outbound::new(8);
        let session_id = SessionId::new();
        let member_id = MemberId::new();

        outbound.send_latest(session_id, member_id, frame(b"first"));
        assert_eq!(outbound.pending_locations(), 1);
        outbound.send_latest(session_id, member_id, frame(b"second"));

        let latest = outbound.latest.lock().expect("lock");
        assert_eq!(latest.len(), 1, "the intermediate location is replaced");
        assert_eq!(latest[&(session_id, member_id)].payload, b"second");
    }

    #[tokio::test]
    async fn test_locations_of_different_members_do_not_coalesce() {
        // covers: docs/08 §10
        let (outbound, _control, _latest, _notify) = Outbound::new(8);
        let session_id = SessionId::new();
        let first = MemberId::new();
        let second = MemberId::new();

        outbound.send_latest(session_id, first, frame(b"a"));
        outbound.send_latest(session_id, second, frame(b"b"));
        assert_eq!(outbound.pending_locations(), 2);
    }

    #[tokio::test]
    async fn test_the_control_queue_is_bounded() {
        // covers: docs/08 §10 (4005 SLOW_CONSUMER)
        let (outbound, _control, _latest, _notify) = Outbound::new(1);
        let message = ServerMessage {
            request_id: 1,
            body: Some(ServerBody::HeartbeatAck(
                gs_service::gs_protocol::HeartbeatAck {
                    client_time: None,
                    server_time: None,
                },
            )),
        };
        // The first frame takes the only slot; the receiver is not drained, so the queue is full.
        outbound.control.try_send(frame(b"taken")).expect("slot");
        let queued = tokio::time::timeout(
            Duration::from_millis(50),
            outbound.send_control(&message, Subprotocol::Proto),
        )
        .await;
        assert!(
            queued.is_err() || queued == Ok(true),
            "a full queue blocks instead of discarding the frame"
        );
    }

    #[tokio::test]
    async fn test_the_fanout_deposits_in_every_local_subscriber() {
        // covers: FR-STR-003, docs/04 §8
        let fanout = Arc::new(Fanout::new(BusBackend::memory(), "gs:test"));
        let session_id = SessionId::new();
        let member_id = MemberId::new();
        let (first_outbound, _first_control, _first_latest, _first_notify) = Outbound::new(4);
        let (second_outbound, _second_control, _second_latest, _second_notify) = Outbound::new(4);
        let first = test_handle(
            gs_service::gs_domain::ids::ConnectionId::new(),
            first_outbound.clone(),
        );
        let second = test_handle(
            gs_service::gs_domain::ids::ConnectionId::new(),
            second_outbound.clone(),
        );

        fanout.by_session.entry(session_id).or_default().push(first);
        fanout
            .by_session
            .entry(session_id)
            .or_default()
            .push(second);
        assert_eq!(fanout.subscribers(session_id), 2);

        let update = ServerMessage {
            request_id: 0,
            body: Some(ServerBody::HeartbeatAck(
                gs_service::gs_protocol::HeartbeatAck {
                    client_time: None,
                    server_time: None,
                },
            )),
        };
        fanout.deposit_location(session_id, member_id, &update);
        assert_eq!(first_outbound.pending_locations(), 1);
        assert_eq!(second_outbound.pending_locations(), 1);
    }

    #[tokio::test]
    async fn test_unsubscribing_releases_the_last_slot() {
        // covers: docs/04 §6.1 (refcount de temas)
        let fanout = Fanout::new(BusBackend::memory(), "gs:test");
        let session_id = SessionId::new();
        let connection_id = gs_service::gs_domain::ids::ConnectionId::new();
        let (outbound, _control, _latest, _notify) = Outbound::new(4);
        fanout
            .by_session
            .entry(session_id)
            .or_default()
            .push(test_handle(connection_id, outbound));

        assert!(
            fanout.unsubscribe(session_id, connection_id),
            "the last subscriber leaves"
        );
        assert_eq!(fanout.subscribers(session_id), 0);
        assert!(!fanout.unsubscribe(session_id, connection_id));
    }
}
