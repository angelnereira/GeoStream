//! Subscription use case: authorization, session information, member statuses with
//! minimization and the snapshot of a session (docs/08 §6.3, docs/05 §8, FR-STR-002,
//! FR-PRS-001, FR-PRS-004).
//!
//! The gap-free algorithm of `docs/08` §6.3 is: validate and authorize, register the
//! subscription as `PENDING`, read the session, the member statuses and the snapshots, enqueue
//! the `SubscribeAck` and only then move the subscription to `ACTIVE` so that nothing that
//! happens in between is lost. This module is the reading part; the backlog that makes it
//! gap-free belongs to the connection (`gs-stream::subscription`).
#![forbid(unsafe_code)]

use gs_domain::error::{AsErrorCode, ErrorCode};
use gs_domain::ids::{ApplicationId, DeviceId, SessionId, SubjectId, TenantId};
use gs_domain::membership::{Capabilities, Connectivity, MemberRole};
use gs_domain::session::{Session, SessionMode, SessionStatus};
use gs_ephemeral::EphemeralStore;
use gs_protocol::LocationUpdate;
use gs_store::repos::{MemberWithSubject, MembershipRepository, SessionRepository};
use gs_store::{PgPool, StoreError};
use prost::Message as _;
use thiserror::Error;
use time::OffsetDateTime;

/// Scope an application needs to subscribe (docs/06 §8.3).
pub const LOCATIONS_READ_SCOPE: &str = "locations:read";

/// Principal asking to subscribe (docs/06 §8.3).
pub enum SubscribePrincipal<'a> {
    /// Device: authorized by its active membership and its `read` capability.
    Device {
        /// Subject of the device.
        subject_id: SubjectId,
        /// Device of the connection.
        device_id: DeviceId,
    },
    /// Application: authorized by the `locations:read` scope of its API key.
    Application {
        /// Application of the principal.
        application_id: ApplicationId,
        /// Scopes of the API key.
        scopes: &'a [String],
    },
}

/// Everything a subscription question needs.
pub struct SubscribeParams<'a, E: ?Sized> {
    /// PostgreSQL pool.
    pub pool: &'a PgPool,
    /// Ephemeral store: snapshots and presence (docs/11 §3).
    pub ephemeral: &'a E,
    /// Ephemeral key prefix (`GS_REDIS_KEY_PREFIX`, default `gs`).
    pub key_prefix: &'a str,
    /// Tenant of the principal.
    pub tenant_id: TenantId,
    /// Session to subscribe to.
    pub session_id: SessionId,
    /// Principal asking.
    pub principal: SubscribePrincipal<'a>,
    /// Current instant, for the lazy expiry check of INV-SES-2.
    pub now: OffsetDateTime,
}

/// Errors of a subscription, mapped to the catalog of `docs/09` §6.
#[derive(Debug, Error)]
pub enum SubscribeError {
    /// The session does not exist, belongs to another tenant, or the principal is not an
    /// active member (FR-API-006).
    #[error("session not found for this principal")]
    NotFound,

    /// The principal is a member without `read`, or an application without
    /// `locations:read` (docs/06 §8.3).
    #[error("permission denied: {0}")]
    PermissionDenied(&'static str),

    /// The session is closed (INV-SES-1).
    #[error("session is closed")]
    SessionClosed,

    /// The session is active but already expired (INV-SES-2).
    #[error("session is expired")]
    SessionExpired,

    /// A persistence operation failed.
    #[error("storage error: {0}")]
    Store(#[from] StoreError),
}

impl AsErrorCode for SubscribeError {
    fn error_code(&self) -> ErrorCode {
        match self {
            Self::NotFound => ErrorCode::NotFound,
            Self::PermissionDenied(_) => ErrorCode::PermissionDenied,
            Self::SessionClosed => ErrorCode::SessionClosed,
            Self::SessionExpired => ErrorCode::SessionExpired,
            Self::Store(_) => ErrorCode::Internal,
        }
    }
}

/// Session information of a `SubscribeAck` (OpenAPI/`session.proto` `SessionInfo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfoView {
    /// Session id.
    pub session_id: SessionId,
    /// Mode of the session.
    pub mode: SessionMode,
    /// Effective status: `EXPIRED` when the session is active but past its expiry.
    pub status: SessionStatus,
    /// Expiry of the session.
    pub expires_at: OffsetDateTime,
    /// Maximum number of members.
    pub max_members: u32,
    /// Age after which a location is stale.
    pub stale_after_seconds: u32,
    /// TTL of the snapshots of the session.
    pub snapshot_ttl_seconds: u32,
}

/// Status of one member, with the minimization of `docs/05` §8 (FR-PRS-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberStatusView {
    /// Membership id.
    pub member_id: gs_domain::ids::MemberId,
    /// Integrator identifier of the subject (opaque by policy).
    pub external_subject_id: String,
    /// Role of the membership.
    pub role: MemberRole,
    /// Capabilities of the membership.
    pub capabilities: Capabilities,
    /// Whether the subject is sharing its location.
    pub sharing: bool,
    /// Connectivity of the publishing device: `UNSPECIFIED` when not sharing.
    pub connectivity: Connectivity,
    /// `captured_at` of the live snapshot, absent when not sharing or without snapshot.
    pub last_location_at: Option<OffsetDateTime>,
}

/// The answer to a successful subscription.
#[derive(Debug, Clone, PartialEq)]
pub struct SubscribeView {
    /// Information of the session.
    pub session: SessionInfoView,
    /// Status of every active member, in the documented order.
    pub members: Vec<MemberStatusView>,
    /// Live snapshots of the members that share, in the same order.
    pub snapshot: Vec<LocationUpdate>,
}

/// Answers a subscription: authorizes and reads session, members and snapshots.
pub async fn subscribe<E: EphemeralStore + ?Sized>(
    params: SubscribeParams<'_, E>,
) -> Result<SubscribeView, SubscribeError> {
    let session = load_authorized_session(&params).await?;

    let mut view = SubscribeView {
        session: session_info(&session, params.now),
        members: Vec::new(),
        snapshot: Vec::new(),
    };
    fill_members(&mut view, &params, &session).await?;
    Ok(view)
}

/// Loads the session and authorizes the principal (docs/06 §8.1, INV-SES-1/2).
async fn load_authorized_session<E: EphemeralStore + ?Sized>(
    params: &SubscribeParams<'_, E>,
) -> Result<Session, SubscribeError> {
    let session = SessionRepository::get_by_id(params.pool, params.tenant_id, params.session_id)
        .await?
        .ok_or(SubscribeError::NotFound)?;

    match session.status {
        SessionStatus::Active if session.expires_at <= params.now => {
            return Err(SubscribeError::SessionExpired);
        }
        SessionStatus::Active => {}
        SessionStatus::Closed => return Err(SubscribeError::SessionClosed),
        SessionStatus::Expired => return Err(SubscribeError::SessionExpired),
    }

    match &params.principal {
        SubscribePrincipal::Device {
            subject_id,
            device_id: _,
        } => {
            let membership = MembershipRepository::find_active_by_session_and_subject(
                params.pool,
                params.tenant_id,
                params.session_id,
                *subject_id,
            )
            .await?
            .ok_or(SubscribeError::NotFound)?;
            if !membership.capabilities.read {
                return Err(SubscribeError::PermissionDenied("read"));
            }
        }
        SubscribePrincipal::Application { scopes, .. } => {
            if !scopes.iter().any(|scope| scope == LOCATIONS_READ_SCOPE) {
                return Err(SubscribeError::PermissionDenied(LOCATIONS_READ_SCOPE));
            }
        }
    }

    Ok(session)
}

/// Builds the session information of the answer.
fn session_info(session: &Session, now: OffsetDateTime) -> SessionInfoView {
    SessionInfoView {
        session_id: session.id,
        mode: session.mode,
        // INV-SES-2: an active session past its expiry reads as expired.
        status: if session.status == SessionStatus::Active && session.expires_at <= now {
            SessionStatus::Expired
        } else {
            session.status
        },
        expires_at: session.expires_at,
        max_members: session.max_members,
        stale_after_seconds: session.stale_after_seconds,
        snapshot_ttl_seconds: session.snapshot_ttl_seconds,
    }
}

/// Reads the member statuses and the live snapshots (docs/05 §8, docs/11 §3).
async fn fill_members<E: EphemeralStore + ?Sized>(
    view: &mut SubscribeView,
    params: &SubscribeParams<'_, E>,
    session: &Session,
) -> Result<(), SubscribeError> {
    let page = MembershipRepository::list_active_by_session(
        params.pool,
        params.tenant_id,
        params.session_id,
        i64::from(session.max_members),
        None,
    )
    .await?;

    for member in &page.members {
        let status = member_status(
            member,
            params.ephemeral,
            params.key_prefix,
            params.session_id,
        )
        .await;
        if status.sharing
            && let Some(snapshot) = read_snapshot(
                params.ephemeral,
                params.key_prefix,
                params.session_id,
                status.member_id,
            )
            .await
        {
            view.snapshot.push(snapshot);
        }
        view.members.push(status);
    }

    Ok(())
}

/// Builds the status of one member with the minimization of `docs/05` §8.
///
/// INV-PRS-1: a member that does not share never reveals whether it is connected, so its
/// connectivity is `UNSPECIFIED` and it carries no `last_location_at`.
async fn member_status<E: EphemeralStore + ?Sized>(
    member: &MemberWithSubject,
    ephemeral: &E,
    key_prefix: &str,
    session_id: SessionId,
) -> MemberStatusView {
    let membership = &member.membership;
    if !membership.sharing {
        return MemberStatusView {
            member_id: membership.id,
            external_subject_id: member.external_subject_id.clone(),
            role: membership.role,
            capabilities: membership.capabilities,
            sharing: false,
            connectivity: Connectivity::Unspecified,
            last_location_at: None,
        };
    }

    let publisher_online = match membership.publisher_device_id {
        Some(device_id) => {
            let key = format!("{key_prefix}:presence:{device_id}");
            ephemeral.get_presence(&key).await.ok().flatten().is_some()
        }
        None => false,
    };

    let snapshot = read_snapshot(ephemeral, key_prefix, session_id, membership.id).await;
    let last_location_at = snapshot
        .as_ref()
        .and_then(|update| update.sample.as_ref())
        .and_then(|sample| sample.captured_at)
        .map(|captured_at| {
            // A protobuf timestamp always fits in the range `time` accepts.
            OffsetDateTime::from_unix_timestamp(captured_at.seconds)
                .unwrap_or(OffsetDateTime::UNIX_EPOCH)
                + time::Duration::nanoseconds(i64::from(captured_at.nanos))
        });

    MemberStatusView {
        member_id: membership.id,
        external_subject_id: member.external_subject_id.clone(),
        role: membership.role,
        capabilities: membership.capabilities,
        sharing: true,
        connectivity: if publisher_online {
            Connectivity::Online
        } else {
            Connectivity::Offline
        },
        last_location_at,
    }
}

/// Reads `{p}:snap:{session}:{member}`, the live snapshot of a member (docs/11 §3).
async fn read_snapshot<E: EphemeralStore + ?Sized>(
    ephemeral: &E,
    key_prefix: &str,
    session_id: SessionId,
    member_id: gs_domain::ids::MemberId,
) -> Option<LocationUpdate> {
    let key = format!("{key_prefix}:snap:{session_id}:{member_id}");
    let bytes = ephemeral.get_snapshot(&key).await.ok().flatten()?;
    gs_protocol::LocationUpdate::decode(bytes.as_slice()).ok()
}

/// Returns true when the capabilities allow reading a session (docs/06 §8.3).
pub fn can_read(capabilities: &Capabilities) -> bool {
    capabilities.read
}
