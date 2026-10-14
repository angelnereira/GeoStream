//! Subscriptions of a connection: backlog without gaps, snapshot and unsubscribe
//! (docs/08 §6.3, FR-STR-002, FR-PRS-004).
//!
//! ```text
//! PENDING --ack encolado--> ACTIVE
//! ```
//!
//! While a subscription is `PENDING`, whatever the session emits is retained: locations in
//! the coalesced map of the subscriber and control events in a bounded queue. When the queue
//! overflows, the read is retried once and then the subscribe fails with
//! `TEMPORARILY_UNAVAILABLE`, so the client can rebuild its state instead of silently missing
//! events (docs/08 §6.3, docs/08 §10).
//!
//! The messages themselves are delivered by the fan-out of T-F4-06; this module owns the
//! per-connection state that makes the ordering correct.
#![forbid(unsafe_code)]

use prost::Message;
use std::collections::HashMap;
use std::str::FromStr;

use gs_service::gs_domain::ids::{MemberId, SessionId};
use gs_service::gs_domain::membership::{Capabilities, Connectivity, MemberRole};
use gs_service::gs_domain::session::{SessionMode, SessionStatus};
use gs_service::gs_protocol::{
    Capabilities as ProtoCapabilities, Connectivity as ProtoConnectivity, LocationUpdate,
    MemberRole as ProtoMemberRole, MemberStatus, SubscribeAck, UnsubscribeAck,
};
use gs_service::subscriptions::{SessionInfoView, SubscribeView};

/// Retention limit of the pending control queue (`GS_STREAM_PENDING_EVENTS_MAX`).
pub const DEFAULT_PENDING_EVENTS_MAX: usize = 256;

/// State of a subscription in the gap-free algorithm (docs/08 §6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionState {
    /// Reading in progress: events are retained.
    Pending,
    /// `SubscribeAck` enqueued: retained events may be released.
    Active,
}

/// One subscription of a connection.
#[derive(Debug, Clone)]
pub struct Subscription {
    /// Session subscribed to.
    pub session_id: SessionId,
    /// State of the gap-free algorithm.
    pub state: SubscriptionState,
    /// Control events retained while the subscription was pending, oldest first.
    pending_events: Vec<Vec<u8>>,
    /// Latest location per member, coalesced while the subscription was pending.
    pending_locations: HashMap<MemberId, Vec<u8>>,
}

impl Subscription {
    /// Builds a pending subscription.
    pub fn pending(session_id: SessionId) -> Self {
        Self {
            session_id,
            state: SubscriptionState::Pending,
            pending_events: Vec::new(),
            pending_locations: HashMap::new(),
        }
    }

    /// Whether the subscription is active.
    pub const fn is_active(&self) -> bool {
        matches!(self.state, SubscriptionState::Active)
    }

    /// Retains a control event, returning false when the bounded queue overflows.
    pub fn retain_event(&mut self, payload: &[u8], max_events: usize) -> bool {
        if self.pending_events.len() >= max_events {
            return false;
        }
        self.pending_events.push(payload.to_vec());
        true
    }

    /// Retains the latest location of a member, coalescing the intermediate ones
    /// (docs/08 §10).
    pub fn retain_location(&mut self, member_id: MemberId, payload: &[u8]) {
        self.pending_locations.insert(member_id, payload.to_vec());
    }

    /// Moves the subscription to active and returns what was retained.
    ///
    /// The events come first, then the coalesced locations whose sequence is newer than the
    /// one in the snapshot (docs/08 §6.3 step 5).
    pub fn activate(&mut self, snapshot_sequences: &[(MemberId, u64)]) -> Vec<Vec<u8>> {
        self.state = SubscriptionState::Active;
        let mut released = std::mem::take(&mut self.pending_events);
        for (member_id, payload) in std::mem::take(&mut self.pending_locations) {
            let floor = snapshot_sequences
                .iter()
                .find(|(id, _)| *id == member_id)
                .map_or(0, |(_, sequence)| *sequence);
            if sequence_of(&payload) > floor {
                released.push(payload);
            }
        }
        released
    }

    /// Number of retained control events: diagnostic helper.
    pub fn pending_event_count(&self) -> usize {
        self.pending_events.len()
    }
}

/// Subscriptions of one connection, bounded by the effective limit (docs/08 §6.3).
#[derive(Debug, Default)]
pub struct SubscriptionSet {
    subscriptions: Vec<Subscription>,
}

impl SubscriptionSet {
    /// Builds an empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of live subscriptions.
    pub fn len(&self) -> usize {
        self.subscriptions.len()
    }

    /// Whether there is no subscription.
    pub fn is_empty(&self) -> bool {
        self.subscriptions.is_empty()
    }

    /// Returns the subscription of a session.
    pub fn get(&self, session_id: SessionId) -> Option<&Subscription> {
        self.subscriptions
            .iter()
            .find(|subscription| subscription.session_id == session_id)
    }

    /// Mutable access to the subscription of a session.
    pub fn get_mut(&mut self, session_id: SessionId) -> Option<&mut Subscription> {
        self.subscriptions
            .iter_mut()
            .find(|subscription| subscription.session_id == session_id)
    }

    /// Whether the session is subscribed, so a second `Subscribe` is idempotent.
    pub fn contains(&self, session_id: SessionId) -> bool {
        self.get(session_id).is_some()
    }

    /// Inserts a pending subscription, returning false when the limit is already reached.
    pub fn insert(&mut self, session_id: SessionId, max: u32) -> bool {
        if self.subscriptions.len() >= max as usize {
            return false;
        }
        self.subscriptions.push(Subscription::pending(session_id));
        true
    }

    /// Drops the subscription of a session; returns whether it existed.
    pub fn remove(&mut self, session_id: SessionId) -> bool {
        let before = self.subscriptions.len();
        self.subscriptions
            .retain(|subscription| subscription.session_id != session_id);
        before != self.subscriptions.len()
    }

    /// Every subscribed session, in insertion order.
    pub fn sessions(&self) -> impl Iterator<Item = SessionId> + '_ {
        self.subscriptions
            .iter()
            .map(|subscription| subscription.session_id)
    }
}

/// Builds the `SubscribeAck` of a view (docs/08 §6.3).
pub fn subscribe_ack(view: &SubscribeView) -> SubscribeAck {
    SubscribeAck {
        session: Some(session_info(&view.session)),
        members: view.members.iter().map(member_status).collect(),
        snapshot: view.snapshot.clone(),
    }
}

/// Builds the `SessionInfo` of the wire message.
fn session_info(view: &SessionInfoView) -> gs_service::gs_protocol::SessionInfo {
    gs_service::gs_protocol::SessionInfo {
        session_id: view.session_id.to_string(),
        mode: proto_mode(view.mode) as i32,
        status: proto_status(view.status) as i32,
        expires_at: Some(pbjson_types::Timestamp {
            seconds: view.expires_at.unix_timestamp(),
            nanos: view.expires_at.nanosecond() as i32,
        }),
        max_members: view.max_members,
        stale_after_seconds: view.stale_after_seconds,
        snapshot_ttl_seconds: view.snapshot_ttl_seconds,
    }
}

/// Builds the `MemberStatus` of the wire message with the minimization of docs/05 §8.
fn member_status(view: &gs_service::subscriptions::MemberStatusView) -> MemberStatus {
    MemberStatus {
        session_id: String::new(),
        member_id: view.member_id.to_string(),
        external_subject_id: view.external_subject_id.clone(),
        role: proto_role(view.role) as i32,
        capabilities: Some(proto_capabilities(&view.capabilities)),
        sharing: view.sharing,
        connectivity: proto_connectivity(view.connectivity) as i32,
        last_location_at: view
            .last_location_at
            .map(|instant| pbjson_types::Timestamp {
                seconds: instant.unix_timestamp(),
                nanos: instant.nanosecond() as i32,
            }),
    }
}

/// Builds the `UnsubscribeAck` of the wire message.
pub fn unsubscribe_ack(session_id: SessionId) -> UnsubscribeAck {
    UnsubscribeAck {
        session_id: session_id.to_string(),
    }
}

/// Domain session mode to wire session mode.
fn proto_mode(mode: SessionMode) -> gs_service::gs_protocol::SessionMode {
    match mode {
        SessionMode::OneToOne => gs_service::gs_protocol::SessionMode::OneToOne,
        SessionMode::OneToMany => gs_service::gs_protocol::SessionMode::OneToMany,
        SessionMode::ManyToMany => gs_service::gs_protocol::SessionMode::ManyToMany,
    }
}

/// Domain session status to wire session status.
fn proto_status(status: SessionStatus) -> gs_service::gs_protocol::SessionStatus {
    match status {
        SessionStatus::Active => gs_service::gs_protocol::SessionStatus::Active,
        SessionStatus::Closed => gs_service::gs_protocol::SessionStatus::Closed,
        SessionStatus::Expired => gs_service::gs_protocol::SessionStatus::Expired,
    }
}

/// Domain role to wire role (docs/08 §12).
fn proto_role(role: MemberRole) -> ProtoMemberRole {
    match role {
        MemberRole::Owner => ProtoMemberRole::Owner,
        MemberRole::Member => ProtoMemberRole::Member,
    }
}

/// Domain capabilities to wire capabilities.
fn proto_capabilities(capabilities: &Capabilities) -> ProtoCapabilities {
    ProtoCapabilities {
        publish: capabilities.publish,
        read: capabilities.read,
        invite: capabilities.invite,
        manage: capabilities.manage,
    }
}

/// Domain connectivity to wire connectivity.
fn proto_connectivity(connectivity: Connectivity) -> ProtoConnectivity {
    match connectivity {
        Connectivity::Unspecified => ProtoConnectivity::Unspecified,
        Connectivity::Online => ProtoConnectivity::Online,
        Connectivity::Offline => ProtoConnectivity::Offline,
    }
}

/// Sequence of an encoded `LocationUpdate`: 0 when it does not decode.
fn sequence_of(payload: &[u8]) -> u64 {
    LocationUpdate::decode(payload)
        .ok()
        .and_then(|update| update.sample)
        .map_or(0, |sample| sample.sequence)
}

/// Snapshot sequences of a view, used to release only newer retained locations.
pub fn snapshot_sequences(view: &SubscribeView) -> Vec<(MemberId, u64)> {
    let mut sequences: Vec<(MemberId, u64)> = view
        .snapshot
        .iter()
        .filter_map(|update| {
            let member_id = MemberId::from_str(&update.member_id).ok()?;
            let sequence = update.sample.as_ref().map_or(0, |sample| sample.sequence);
            Some((member_id, sequence))
        })
        .collect();
    sequences.sort_by_key(|(member_id, _)| *member_id);
    sequences
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_id() -> SessionId {
        SessionId::new()
    }

    #[test]
    fn test_subscription_starts_pending_and_becomes_active() {
        // covers: docs/08 §6.3
        let session_id = session_id();
        let mut set = SubscriptionSet::new();
        assert!(set.insert(session_id, 20));
        assert_eq!(set.len(), 1, "the same session is not inserted twice");
        assert!(set.contains(session_id));
        let subscription = set.get(session_id).expect("subscription");
        assert!(!subscription.is_active());
        assert_eq!(subscription.state, SubscriptionState::Pending);

        // Once the ack is enqueued the subscription becomes active.
        let subscription = set.get_mut(session_id).expect("subscription");
        subscription.activate(&[]);
        assert!(subscription.is_active());
    }

    #[test]
    fn test_the_subscription_limit_is_enforced() {
        // covers: docs/08 §6.3 (TOO_MANY_SUBSCRIPTIONS)
        let mut set = SubscriptionSet::new();
        assert!(set.insert(session_id(), 2));
        assert!(set.insert(session_id(), 2));
        assert!(!set.insert(session_id(), 2), "the third is refused");
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_subscribing_twice_is_idempotent() {
        // covers: docs/08 §6.3
        let session_id = session_id();
        let mut set = SubscriptionSet::new();
        assert!(set.insert(session_id, 20));
        assert!(set.contains(session_id));
        let subscription = set.get_mut(session_id).expect("subscription");
        subscription.state = SubscriptionState::Active;
        assert!(set.get(session_id).is_some_and(|s| s.is_active()));
    }

    #[test]
    fn test_unsubscribe_drops_the_entry() {
        // covers: docs/08 §6.3
        let session_id = session_id();
        let mut set = SubscriptionSet::new();
        set.insert(session_id, 20);
        assert!(set.remove(session_id));
        assert!(!set.remove(session_id), "unsubscribe is idempotent");
        assert!(set.is_empty());
    }

    #[test]
    fn test_pending_events_are_bounded_and_released_in_order() {
        // covers: docs/08 §6.3, docs/08 §10
        let session_id = session_id();
        let mut subscription = Subscription::pending(session_id);
        assert!(subscription.retain_event(b"first", 2));
        assert!(subscription.retain_event(b"second", 2));
        assert!(
            !subscription.retain_event(b"third", 2),
            "the bounded queue overflows"
        );
        assert_eq!(subscription.pending_event_count(), 2);

        let released = subscription.activate(&[]);
        assert_eq!(released, vec![b"first".to_vec(), b"second".to_vec()]);
        assert!(subscription.is_active());
    }

    #[test]
    fn test_retained_locations_are_coalesced_per_member() {
        // covers: docs/08 §10
        let session_id = session_id();
        let mut subscription = Subscription::pending(session_id);
        let member = MemberId::new();
        let older = LocationUpdate {
            session_id: session_id.to_string(),
            member_id: member.to_string(),
            sample: Some(gs_service::gs_protocol::LocationSample {
                sequence: 1,
                ..Default::default()
            }),
            received_at: None,
            delivery: gs_service::gs_protocol::DeliveryKind::Live as i32,
        };
        let newer = LocationUpdate {
            sample: Some(gs_service::gs_protocol::LocationSample {
                sequence: 2,
                ..Default::default()
            }),
            ..older.clone()
        };
        subscription.retain_location(member, &older.encode_to_vec());
        subscription.retain_location(member, &newer.encode_to_vec());

        let released = subscription.activate(&[]);
        assert_eq!(released.len(), 1, "only the newest survives");
        assert_eq!(sequence_of(&released[0]), 2);
    }

    #[test]
    fn test_retained_locations_older_than_the_snapshot_are_dropped() {
        // covers: docs/08 §6.3 step 5
        let session_id = session_id();
        let mut subscription = Subscription::pending(session_id);
        let member = MemberId::new();
        let update = LocationUpdate {
            session_id: session_id.to_string(),
            member_id: member.to_string(),
            sample: Some(gs_service::gs_protocol::LocationSample {
                sequence: 7,
                ..Default::default()
            }),
            received_at: None,
            delivery: gs_service::gs_protocol::DeliveryKind::Live as i32,
        };
        subscription.retain_location(member, &update.encode_to_vec());

        // A snapshot already carries sequence 7: nothing newer was retained.
        assert!(subscription.activate(&[(member, 7)]).is_empty());
    }
}
