//! Domain control events published on the bus (docs/04 §7, docs/05 §10, docs/08 §9).
//!
//! Control events travel over the bus to notify stream instances of session transitions,
//! membership changes, and global security revocations (FR-STR-008, FR-STR-011, FR-AUTH-005).
#![forbid(unsafe_code)]

use gs_bus::{Bus, BusError};
use gs_domain::ids::{MemberId, SessionId};
use gs_domain::membership::{Capabilities, Connectivity, MemberRole};
use gs_domain::session::{SessionMode, SessionStatus};
use gs_protocol::{
    Capabilities as ProtoCapabilities, Connectivity as ProtoConnectivity,
    MemberRole as ProtoMemberRole, MemberStatus as ProtoMemberStatus,
    SessionInfo as ProtoSessionInfo,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Global security events published to `{prefix}:bus:global` (docs/04 §6.1, docs/06 §7, docs/08 §9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum GlobalBusEvent {
    /// A tenant has been disabled (closes connections with 4403).
    TenantDisabled { tenant_id: String, at: String },
    /// A tenant has been enabled.
    TenantEnabled { tenant_id: String, at: String },
    /// An application has been disabled (closes connections with 4403).
    ApplicationDisabled {
        tenant_id: String,
        application_id: String,
        at: String,
    },
    /// An application has been enabled.
    ApplicationEnabled {
        tenant_id: String,
        application_id: String,
        at: String,
    },
    /// An API key has been revoked (closes connections with 4401).
    ApiKeyRevoked {
        tenant_id: String,
        api_key_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        application_id: Option<String>,
        at: String,
    },
    /// A subject's tokens have been revoked (closes connections with 4401).
    SubjectTokensRevoked {
        tenant_id: String,
        subject_id: String,
        at: String,
    },
    /// A device has been deactivated (closes connections with 4401).
    DeviceDeactivated {
        tenant_id: String,
        device_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        subject_id: Option<String>,
        at: String,
    },
}

/// Session-scoped control events published to `{prefix}:bus:ctl:{session_id}` (docs/04 §6.1, docs/05 §10, docs/08 §9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum SessionControlEvent {
    /// A session was closed. Subscribers receive `SessionEvent{CLOSED}` and `SubscriptionEnded{SESSION_CLOSED}`.
    SessionClosed {
        session_id: String,
        reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        session: Option<ProtoSessionInfo>,
        at: String,
    },
    /// A session expired. Subscribers receive `SessionEvent{EXPIRED}` and `SubscriptionEnded{SESSION_EXPIRED}`.
    SessionExpired {
        session_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        session: Option<ProtoSessionInfo>,
        at: String,
    },
    /// A session's parameters were updated. Subscribers receive `SessionEvent{UPDATED}`.
    SessionUpdated {
        session_id: String,
        session: ProtoSessionInfo,
        at: String,
    },
    /// A new member joined. Subscribers receive `MemberEvent{JOINED}`.
    MemberJoined {
        session_id: String,
        member: ProtoMemberStatus,
        at: String,
    },
    /// A member's role or capabilities were updated.
    ///
    /// If `lost_read` is true, the affected subject's connection receives `SubscriptionEnded{PERMISSION_CHANGED}`.
    /// Other subscribers receive `MemberEvent{UPDATED}`.
    MemberUpdated {
        session_id: String,
        member: ProtoMemberStatus,
        subject_id: String,
        #[serde(default)]
        lost_read: bool,
        #[serde(default)]
        lost_publish: bool,
        at: String,
    },
    /// A member voluntarily left. Affected subject receives `SubscriptionEnded{MEMBER_LEFT}`, others `MemberEvent{LEFT}`.
    MemberLeft {
        session_id: String,
        member_id: String,
        subject_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        member: Option<ProtoMemberStatus>,
        at: String,
    },
    /// A member was revoked. Affected subject receives `SubscriptionEnded{MEMBER_REVOKED}`, others `MemberEvent{REVOKED}`.
    MemberRevoked {
        session_id: String,
        member_id: String,
        subject_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        member: Option<ProtoMemberStatus>,
        at: String,
    },
    /// A member started sharing. Subscribers receive `MemberStatus{sharing=true}`.
    SharingStarted {
        session_id: String,
        member_id: String,
        device_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        member: Option<ProtoMemberStatus>,
        at: String,
    },
    /// A member stopped sharing. Subscribers receive `MemberStatus{sharing=false}`.
    SharingStopped {
        session_id: String,
        member_id: String,
        device_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        member: Option<ProtoMemberStatus>,
        at: String,
    },
    /// A member transferred publishing to another device. Subscribers receive `MemberStatus{sharing=true}`.
    SharingTransferred {
        session_id: String,
        member_id: String,
        old_device_id: String,
        new_device_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        member: Option<ProtoMemberStatus>,
        at: String,
    },
}

/// Helper to build a wire `ProtoSessionInfo` from domain fields.
pub fn to_proto_session_info(
    session_id: SessionId,
    mode: SessionMode,
    status: SessionStatus,
    expires_at: OffsetDateTime,
    max_members: u32,
    stale_after_seconds: u32,
    snapshot_ttl_seconds: u32,
) -> ProtoSessionInfo {
    ProtoSessionInfo {
        session_id: session_id.to_string(),
        mode: match mode {
            SessionMode::OneToOne => gs_protocol::SessionMode::OneToOne as i32,
            SessionMode::OneToMany => gs_protocol::SessionMode::OneToMany as i32,
            SessionMode::ManyToMany => gs_protocol::SessionMode::ManyToMany as i32,
        },
        status: match status {
            SessionStatus::Active => gs_protocol::SessionStatus::Active as i32,
            SessionStatus::Closed => gs_protocol::SessionStatus::Closed as i32,
            SessionStatus::Expired => gs_protocol::SessionStatus::Expired as i32,
        },
        expires_at: Some(pbjson_types::Timestamp {
            seconds: expires_at.unix_timestamp(),
            nanos: expires_at.nanosecond() as i32,
        }),
        max_members,
        stale_after_seconds,
        snapshot_ttl_seconds,
    }
}

/// Helper to build a wire `ProtoMemberStatus` from domain fields.
#[allow(clippy::too_many_arguments)]
pub fn to_proto_member_status(
    session_id: SessionId,
    member_id: MemberId,
    external_subject_id: &str,
    role: MemberRole,
    capabilities: Capabilities,
    sharing: bool,
    connectivity: Connectivity,
    last_location_at: Option<OffsetDateTime>,
) -> ProtoMemberStatus {
    ProtoMemberStatus {
        session_id: session_id.to_string(),
        member_id: member_id.to_string(),
        external_subject_id: external_subject_id.to_string(),
        role: match role {
            MemberRole::Owner => ProtoMemberRole::Owner as i32,
            MemberRole::Member => ProtoMemberRole::Member as i32,
        },
        capabilities: Some(ProtoCapabilities {
            publish: capabilities.publish,
            read: capabilities.read,
            invite: capabilities.invite,
            manage: capabilities.manage,
        }),
        sharing,
        connectivity: match connectivity {
            Connectivity::Unspecified => ProtoConnectivity::Unspecified as i32,
            Connectivity::Online => ProtoConnectivity::Online as i32,
            Connectivity::Offline => ProtoConnectivity::Offline as i32,
        },
        last_location_at: last_location_at.map(|t| pbjson_types::Timestamp {
            seconds: t.unix_timestamp(),
            nanos: t.nanosecond() as i32,
        }),
    }
}

/// Publishes a session-scoped control event to `{key_prefix}:bus:ctl:{session_id}`.
pub async fn publish_session_control<B: Bus>(
    bus: &B,
    key_prefix: &str,
    session_id: SessionId,
    event: &SessionControlEvent,
) -> Result<usize, BusError> {
    let topic = format!("{key_prefix}:bus:ctl:{session_id}");
    let payload = serde_json::to_vec(event).map_err(|e| BusError::ConnectionLost(e.to_string()))?;
    bus.publish(&topic, &payload).await
}

/// Publishes a global security event to `{key_prefix}:bus:global`.
pub async fn publish_global_bus_event<B: Bus>(
    bus: &B,
    key_prefix: &str,
    event: &GlobalBusEvent,
) -> Result<usize, BusError> {
    let topic = format!("{key_prefix}:bus:global");
    let payload = serde_json::to_vec(event).map_err(|e| BusError::ConnectionLost(e.to_string()))?;
    bus.publish(&topic, &payload).await
}

/// Formats an instant in RFC 3339.
pub fn format_rfc3339(instant: OffsetDateTime) -> String {
    instant
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}
