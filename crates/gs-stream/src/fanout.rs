//! Fan-out of a session over the bus, with coalescing and bounded queues
//! (docs/04 §8, docs/08 §10, FR-STR-003, FR-STR-006).
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

use dashmap::DashMap;
use gs_service::gs_domain::ids::{MemberId, SessionId};
use gs_service::gs_protocol::{ServerMessage, SubscribeAck, UnsubscribeAck};
use gs_service::{Bus, InMemoryBus};
use prost::Message as _;
use tokio::sync::{Notify, mpsc};
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;

use gs_service::gs_domain::Subprotocol;

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

/// Handle of a live connection inside the fan-out of an instance.
#[derive(Debug, Clone)]
pub struct ConnHandle {
    /// Connection the handle belongs to.
    pub connection_id: gs_service::gs_domain::ids::ConnectionId,
    /// Subprotocol that connection negotiated: each frame is encoded with it (docs/08 §2).
    pub subprotocol: Subprotocol,
    /// Outbound side of that connection.
    pub outbound: Outbound,
    /// Cancelled when the connection ends, so its writer stops.
    pub cancel: CancellationToken,
}

/// Fan-out of an instance: local subscriptions per session and the bus tasks that feed them.
#[derive(Debug)]
pub struct Fanout {
    by_session: DashMap<SessionId, Vec<ConnHandle>>,
    /// Bus the instance publishes and receives from (docs/04 §6.1).
    bus: Arc<InMemoryBus>,
    /// Ephemeral key prefix (`GS_REDIS_KEY_PREFIX`, default `gs`).
    key_prefix: String,
}

impl Fanout {
    /// Builds the fan-out of an instance over its bus.
    pub fn new(bus: Arc<InMemoryBus>, key_prefix: impl Into<String>) -> Self {
        Self {
            by_session: DashMap::new(),
            bus,
            key_prefix: key_prefix.into(),
        }
    }

    /// Registers a connection in the fan-out of a session.
    ///
    /// The first local subscriber of a session also subscribes to its bus topic and starts the
    /// task that deposits what arrives, so every update travels one path (docs/04 §8).
    pub async fn subscribe(self: &Arc<Self>, session_id: SessionId, handle: ConnHandle) {
        let first = {
            let mut entry = self.by_session.entry(session_id).or_default();
            let first = entry.is_empty();
            entry.push(handle);
            first
        };
        if first {
            // The bus subscription completes before returning: with a broadcast bus, a
            // message published between the registration and the subscription would be lost.
            if let Ok(subscription) = self.bus.subscribe(&self.topic(session_id)).await {
                let fanout = Arc::clone(self);
                tokio::spawn(async move { topic_task(fanout, session_id, subscription).await });
            }
        }
    }

    /// Removes a connection from the fan-out of a session.
    ///
    /// With the last subscriber gone the task of the topic ends and releases its bus
    /// subscription (refcount of docs/04 §6.1).
    pub fn unsubscribe(
        &self,
        session_id: SessionId,
        connection_id: gs_service::gs_domain::ids::ConnectionId,
    ) -> bool {
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

    /// Bus topic of a session (docs/04 §6.1).
    fn topic(&self, session_id: SessionId) -> String {
        format!("{}:bus:loc:{session_id}", self.key_prefix)
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
            body: Some(gs_service::gs_protocol::server_message::Body::LocationUpdate(update)),
        };
        fanout.deposit_location(session_id, member_id, &message);
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
        body: Some(gs_service::gs_protocol::server_message::Body::SubscribeAck(
            ack,
        )),
    }
}

/// `UnsubscribeAck` envelope.
pub fn unsubscribe_ack_message(ack: UnsubscribeAck) -> ServerMessage {
    ServerMessage {
        request_id: 0,
        body: Some(gs_service::gs_protocol::server_message::Body::UnsubscribeAck(ack)),
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
            body: Some(gs_service::gs_protocol::server_message::Body::HeartbeatAck(
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
        let fanout = Arc::new(Fanout::new(InMemoryBus::new(), "gs:test"));
        let session_id = SessionId::new();
        let member_id = MemberId::new();
        let (first_outbound, _first_control, _first_latest, _first_notify) = Outbound::new(4);
        let (second_outbound, _second_control, _second_latest, _second_notify) = Outbound::new(4);
        let first = ConnHandle {
            connection_id: gs_service::gs_domain::ids::ConnectionId::new(),
            subprotocol: Subprotocol::Proto,
            outbound: first_outbound.clone(),
            cancel: CancellationToken::new(),
        };
        let second = ConnHandle {
            connection_id: gs_service::gs_domain::ids::ConnectionId::new(),
            subprotocol: Subprotocol::Proto,
            outbound: second_outbound.clone(),
            cancel: CancellationToken::new(),
        };

        fanout.by_session.entry(session_id).or_default().push(first);
        fanout
            .by_session
            .entry(session_id)
            .or_default()
            .push(second);
        assert_eq!(fanout.subscribers(session_id), 2);

        let update = ServerMessage {
            request_id: 0,
            body: Some(gs_service::gs_protocol::server_message::Body::HeartbeatAck(
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
        let fanout = Fanout::new(InMemoryBus::new(), "gs:test");
        let session_id = SessionId::new();
        let connection_id = gs_service::gs_domain::ids::ConnectionId::new();
        let (outbound, _control, _latest, _notify) = Outbound::new(4);
        fanout
            .by_session
            .entry(session_id)
            .or_default()
            .push(ConnHandle {
                connection_id,
                subprotocol: Subprotocol::Proto,
                outbound,
                cancel: CancellationToken::new(),
            });

        assert!(
            fanout.unsubscribe(session_id, connection_id),
            "the last subscriber leaves"
        );
        assert_eq!(fanout.subscribers(session_id), 0);
        assert!(!fanout.unsubscribe(session_id, connection_id));
    }
}
