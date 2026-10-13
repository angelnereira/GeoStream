//! Membership use cases: add member, list, get, update capabilities/role, revoke, leave
//! (docs/05 §5, docs/07 §9.3, FR-MEM-001..007, FR-SES-007).
//!
//! Enforces:
//! - INV-MEM-1: Owner capabilities cannot be reduced.
//! - INV-MEM-2: Manager can only grant subset of own capabilities.
//! - INV-MEM-3: Member cannot modify own capabilities.
//! - INV-MEM-4: Removing publish while sharing stops sharing immediately.
//! - INV-MEM-6: Single active membership per (session, subject).
//! - INV-MEM-7: Active member limit enforced under session row lock.
//! - FR-MEM-005: Ownership transfer atomically swaps owner roles and updates session.
//! - FR-MEM-007: Readmission rules (REVOKED allowed via direct add, rejected via invitation).
//! - FR-SES-007: Owner departure cascades to oldest manager or closes/clears owner.
#![forbid(unsafe_code)]

use time::OffsetDateTime;
use uuid::Uuid;

use gs_bus::Bus;
use gs_domain::error::{AsErrorCode, ErrorCode};
use gs_domain::ids::{DeviceId, MemberId, SessionId, SubjectId, TenantId};
use gs_domain::membership::{
    Capabilities, Connectivity, MemberRole, MemberStatusKind, Membership, MembershipError,
    SharingTransition,
};
use gs_ephemeral::EphemeralStore;
use gs_store::models::{ActorKind, AuditOutcome};
pub use gs_store::repos::{
    AddMemberOutcome, LeaveOutcome, MemberPage, MemberWithSubject, MembershipRepository,
    RevokeOutcome, SetSharingOutcome, SetSharingParams as StoreSetSharingParams,
    TransferOwnershipOutcome, UpdateCapabilitiesOutcome, UpdateCapabilitiesParams,
};
use gs_store::repos::{SessionRepository, SubjectRepository};
use gs_store::{PgPool, StoreError};

use crate::audit::record_audit;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors of membership use cases, mapped to `docs/09` §6.
#[derive(Debug, Error)]
pub enum MemberUseCaseError {
    /// Validation error in request payload.
    #[error("invalid member request: {0}")]
    Validation(String),

    /// Caller lacks required scope.
    #[error("insufficient scope")]
    Forbidden,

    /// Caller lacks required capability or operation not permitted.
    #[error("permission denied: {0}")]
    PermissionDenied(&'static str),

    /// Session or member not found.
    #[error("resource not found")]
    NotFound,

    /// Session is not ACTIVE or member is not active.
    #[error("session is not active")]
    InactiveSession,

    /// Session has reached max_members.
    #[error("session is full")]
    SessionFull,

    /// Subject already has an active membership in the session.
    #[error("subject is already an active member of this session")]
    AlreadyActiveMember,

    /// Direct add specified OWNER role but session already has an owner.
    #[error("session already has an owner")]
    SessionAlreadyHasOwner,

    /// Version precondition failed (If-Match).
    #[error("member version does not match the If-Match header")]
    PreconditionFailed,

    /// Domain rule violation.
    #[error("domain rule violated: {0}")]
    Domain(#[from] MembershipError),

    /// Storage failure.
    #[error("storage error: {0}")]
    Store(#[from] StoreError),
}

impl AsErrorCode for MemberUseCaseError {
    fn error_code(&self) -> ErrorCode {
        match self {
            Self::Validation(_) => ErrorCode::ValidationFailed,
            Self::Forbidden => ErrorCode::InsufficientScope,
            Self::PermissionDenied(_) => ErrorCode::PermissionDenied,
            Self::NotFound => ErrorCode::NotFound,
            Self::InactiveSession => ErrorCode::SessionClosed,
            Self::SessionFull => ErrorCode::SessionFull,
            Self::AlreadyActiveMember | Self::SessionAlreadyHasOwner => ErrorCode::Conflict,
            Self::PreconditionFailed => ErrorCode::PreconditionFailed,
            Self::Domain(d) => match d {
                MembershipError::OwnerCapabilitiesImmutable
                | MembershipError::CannotEscalateCapabilities
                | MembershipError::CannotModifyOwnCapabilities
                | MembershipError::PermissionDenied
                | MembershipError::OnlySelfCanActivateSharing
                | MembershipError::MissingPublishCapability => ErrorCode::PermissionDenied,
                MembershipError::SessionFull => ErrorCode::SessionFull,
                MembershipError::AlreadyActiveMember => ErrorCode::Conflict,
                MembershipError::NotActiveMember => ErrorCode::Conflict,
                MembershipError::MemberRevoked => ErrorCode::MemberRevoked,
                MembershipError::RevokedMemberCannotRedeemInvitation => {
                    ErrorCode::InvitationInvalid
                }
                _ => ErrorCode::ValidationFailed,
            },
            Self::Store(_) => ErrorCode::Internal,
        }
    }
}

/// OpenAPI `Member` representation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberResource {
    pub id: String,
    pub session_id: String,
    pub subject_id: String,
    pub external_subject_id: String,
    pub role: MemberRole,
    pub capabilities: Capabilities,
    pub status: MemberStatusKind,
    pub sharing: bool,
    pub connectivity: Connectivity,
    pub last_location_at: Option<String>,
    pub joined_at: String,
    pub ended_at: Option<String>,
    pub joined_via: String,
    pub version: u64,
}

impl MemberResource {
    /// Builds an API resource from stored member entity enforcing INV-PRS-1.
    pub fn from_member_with_subject(
        m: &MemberWithSubject,
        device_online: bool,
        snapshot_time: Option<OffsetDateTime>,
    ) -> Self {
        let (connectivity, last_location_at) = if m.membership.sharing {
            let conn = if device_online {
                Connectivity::Online
            } else {
                Connectivity::Offline
            };
            (conn, snapshot_time.map(format_rfc3339))
        } else {
            // INV-PRS-1: never reveal presence or coordinates if member is not sharing
            (Connectivity::Unspecified, None)
        };

        let joined_via_str = match &m.membership.joined_via {
            gs_domain::membership::JoinedVia::Creator => "CREATOR",
            gs_domain::membership::JoinedVia::DirectAdd => "DIRECT_ADD",
            gs_domain::membership::JoinedVia::Invitation(_) => "INVITATION",
        };

        Self {
            id: m.membership.id.to_string(),
            session_id: m.membership.session_id.to_string(),
            subject_id: m.membership.subject_id.to_string(),
            external_subject_id: m.external_subject_id.clone(),
            role: m.membership.role,
            capabilities: m.membership.capabilities,
            status: m.membership.status,
            sharing: m.membership.sharing,
            connectivity,
            last_location_at,
            joined_at: format_rfc3339(m.membership.joined_at),
            ended_at: m.membership.ended_at.map(format_rfc3339),
            joined_via: joined_via_str.to_string(),
            version: m.membership.version,
        }
    }

    /// ETag header value for this resource.
    pub fn etag(&self) -> String {
        format!("\"{}\"", self.version)
    }
}

/// OpenAPI `MemberList` representation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberList {
    pub data: Vec<MemberResource>,
    pub next_cursor: Option<String>,
}

/// Request body for `POST /v1/sessions/{session_id}/members`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddMemberRequest {
    pub external_subject_id: String,
    #[serde(default)]
    pub role: Option<MemberRole>,
    #[serde(default)]
    pub capabilities: Option<Capabilities>,
}

/// Request body for `PATCH /v1/sessions/{session_id}/members/{member_id}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateMemberRequest {
    #[serde(default)]
    pub capabilities: Option<Capabilities>,
    #[serde(default)]
    pub role: Option<MemberRole>,
}

/// Caller context for member update operations.
#[derive(Debug, Clone, Copy)]
pub enum UpdateCaller<'a> {
    Application,
    Device(&'a Membership),
}

/// Request body for `PUT /v1/sessions/{session_id}/members/{member_id}/sharing` (docs/07 §9.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharingRequest {
    pub sharing: bool,
}

/// Caller context for setting member sharing state.
#[derive(Debug, Clone)]
pub struct SetSharingCaller {
    pub is_app: bool,
    pub subject_id: Option<SubjectId>,
    pub device_id: Option<DeviceId>,
    pub has_manage: bool,
}

/// Context parameters for the set_sharing use case.
pub struct SetSharingContext<'a, E, B> {
    pub ephemeral: &'a E,
    pub bus: &'a B,
    pub key_prefix: &'a str,
    pub caller: SetSharingCaller,
}

/// Formats a timestamp into RFC 3339.
fn format_rfc3339(instant: OffsetDateTime) -> String {
    instant
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// Validates external subject ID according to docs/05 §2 (`^[A-Za-z0-9._:\-]{1,128}$`).
fn validate_external_subject_id(id: &str) -> Result<(), MemberUseCaseError> {
    if id.is_empty() || id.len() > 128 {
        return Err(MemberUseCaseError::Validation(
            "external_subject_id must be 1..=128 characters".to_string(),
        ));
    }
    for b in id.bytes() {
        if !b.is_ascii_alphanumeric() && b != b'.' && b != b'_' && b != b':' && b != b'-' {
            return Err(MemberUseCaseError::Validation(
                "external_subject_id contains invalid characters".to_string(),
            ));
        }
    }
    Ok(())
}

/// Adds a member directly to a session (application caller only, FR-MEM-001).
pub async fn add_member(
    pool: &PgPool,
    tenant_id: TenantId,
    session_id: SessionId,
    request: &AddMemberRequest,
) -> Result<MemberResource, MemberUseCaseError> {
    validate_external_subject_id(&request.external_subject_id)?;

    let now = OffsetDateTime::now_utc();

    // Verify session exists and is ACTIVE
    let session = SessionRepository::get_by_id(pool, tenant_id, session_id)
        .await?
        .ok_or(MemberUseCaseError::NotFound)?;

    if session.status != gs_domain::session::SessionStatus::Active || session.expires_at <= now {
        return Err(MemberUseCaseError::InactiveSession);
    }

    // Get or create active subject record
    let subject =
        SubjectRepository::get_or_create_active(pool, tenant_id, &request.external_subject_id, now)
            .await?;

    // Check readmission rules (FR-MEM-007)
    if let Some(latest) = MembershipRepository::find_latest_by_session_and_subject(
        pool, tenant_id, session_id, subject.id,
    )
    .await?
        && latest.status == MemberStatusKind::Active
    {
        return Err(MemberUseCaseError::AlreadyActiveMember);
    }

    let role = request.role.unwrap_or(MemberRole::Member);
    let capabilities = match request.capabilities {
        Some(c) => {
            if role == MemberRole::Owner && c != Capabilities::all() {
                return Err(MemberUseCaseError::Domain(
                    MembershipError::OwnerCapabilitiesImmutable,
                ));
            }
            c
        }
        None => Capabilities::default_for_mode(session.mode, role == MemberRole::Owner),
    };

    let outcome = MembershipRepository::add_member(
        pool,
        tenant_id,
        session_id,
        subject.id,
        role,
        capabilities,
        now,
    )
    .await?;

    match outcome {
        AddMemberOutcome::Success(m) => {
            let _ = record_audit(
                pool,
                tenant_id,
                ActorKind::Application,
                None,
                Some(m.membership.subject_id),
                "member.added",
                Some("member"),
                Some(m.membership.id.into_uuid()),
                AuditOutcome::Success,
                None,
                serde_json::json!({
                    "session_id": session_id.to_string(),
                    "role": m.membership.role.to_string(),
                    "can_publish": m.membership.capabilities.publish,
                    "can_read": m.membership.capabilities.read,
                    "can_invite": m.membership.capabilities.invite,
                    "can_manage": m.membership.capabilities.manage,
                }),
            )
            .await;
            Ok(MemberResource::from_member_with_subject(&m, false, None))
        }
        AddMemberOutcome::SessionNotFound => Err(MemberUseCaseError::NotFound),
        AddMemberOutcome::SessionNotActive => Err(MemberUseCaseError::InactiveSession),
        AddMemberOutcome::SessionFull => Err(MemberUseCaseError::SessionFull),
        AddMemberOutcome::AlreadyActive => Err(MemberUseCaseError::AlreadyActiveMember),
        AddMemberOutcome::SessionAlreadyHasOwner => Err(MemberUseCaseError::SessionAlreadyHasOwner),
    }
}

/// Lists active members of a session with cursor pagination (FR-MEM-002, docs/07 §9.3).
pub async fn list_members(
    pool: &PgPool,
    tenant_id: TenantId,
    session_id: SessionId,
    limit: u32,
    cursor: Option<(OffsetDateTime, Uuid)>,
) -> Result<MemberPage, MemberUseCaseError> {
    let session = SessionRepository::get_by_id(pool, tenant_id, session_id)
        .await?
        .ok_or(MemberUseCaseError::NotFound)?;

    if session.status != gs_domain::session::SessionStatus::Active
        && session.expires_at <= OffsetDateTime::now_utc()
    {
        // Still allow listing members of closed/expired sessions as read-only (docs/05 §4.3)
    }

    Ok(MembershipRepository::list_active_by_session(
        pool,
        tenant_id,
        session_id,
        i64::from(limit) + 1,
        cursor,
    )
    .await?)
}

/// Gets a member by ID (FR-MEM-003).
pub async fn get_member(
    pool: &PgPool,
    tenant_id: TenantId,
    session_id: SessionId,
    member_id: MemberId,
) -> Result<MemberResource, MemberUseCaseError> {
    let member = MembershipRepository::get_by_id(pool, tenant_id, session_id, member_id)
        .await?
        .ok_or(MemberUseCaseError::NotFound)?;

    Ok(MemberResource::from_member_with_subject(
        &member, false, None,
    ))
}

/// Resolves `me` for a device caller in a session (docs/07 §9.3, FR-MEM-003).
pub async fn get_member_for_device_me(
    pool: &PgPool,
    tenant_id: TenantId,
    session_id: SessionId,
    caller_subject_id: SubjectId,
) -> Result<MemberResource, MemberUseCaseError> {
    let member = MembershipRepository::find_active_with_subject(
        pool,
        tenant_id,
        session_id,
        caller_subject_id,
    )
    .await?
    .ok_or(MemberUseCaseError::NotFound)?;

    Ok(MemberResource::from_member_with_subject(
        &member, false, None,
    ))
}

/// Updates member capabilities or transfers ownership (FR-MEM-004, FR-MEM-005, docs/07 §9.3).
pub async fn update_member(
    pool: &PgPool,
    tenant_id: TenantId,
    session_id: SessionId,
    target_member_id: MemberId,
    expected_version: Option<u64>,
    request: &UpdateMemberRequest,
    caller: UpdateCaller<'_>,
) -> Result<MemberResource, MemberUseCaseError> {
    if request.capabilities.is_none() && request.role.is_none() {
        return Err(MemberUseCaseError::Validation(
            "at least one of 'capabilities' or 'role' must be provided".to_string(),
        ));
    }

    let caller_is_app = matches!(caller, UpdateCaller::Application);

    // Role changes are strictly application only
    if request.role.is_some() && !caller_is_app {
        return Err(MemberUseCaseError::PermissionDenied(
            "role modifications are only allowed for application callers",
        ));
    }

    let now = OffsetDateTime::now_utc();

    // 1. Role change: OWNER transfers ownership (FR-MEM-005, docs/07 §9.3)
    if let Some(new_role) = request.role
        && new_role == MemberRole::Owner
    {
        let outcome = MembershipRepository::transfer_ownership(
            pool,
            tenant_id,
            session_id,
            target_member_id,
            expected_version,
            now,
        )
        .await?;

        match outcome {
            TransferOwnershipOutcome::Success(m) => {
                let (actor_kind, actor_id, actor_sub_id) = match caller {
                    UpdateCaller::Application => (ActorKind::Application, None, None),
                    UpdateCaller::Device(cm) => (
                        ActorKind::Device,
                        Some(cm.id.into_uuid()),
                        Some(cm.subject_id),
                    ),
                };
                let _ = record_audit(
                    pool,
                    tenant_id,
                    actor_kind,
                    actor_id,
                    actor_sub_id,
                    "member.updated",
                    Some("member"),
                    Some(target_member_id.into_uuid()),
                    AuditOutcome::Success,
                    None,
                    serde_json::json!({
                        "session_id": session_id.to_string(),
                        "new_role": "OWNER",
                    }),
                )
                .await;
                return Ok(MemberResource::from_member_with_subject(&m, false, None));
            }
            TransferOwnershipOutcome::SessionNotFound
            | TransferOwnershipOutcome::MemberNotFound => {
                return Err(MemberUseCaseError::NotFound);
            }
            TransferOwnershipOutcome::SessionNotActive
            | TransferOwnershipOutcome::MemberNotActive => {
                return Err(MemberUseCaseError::InactiveSession);
            }
            TransferOwnershipOutcome::PreconditionFailed => {
                return Err(MemberUseCaseError::PreconditionFailed);
            }
        }
    }

    // 2. Capabilities change (INV-MEM-1..4)
    if let Some(new_caps) = request.capabilities {
        let current_target =
            MembershipRepository::get_by_id(pool, tenant_id, session_id, target_member_id)
                .await?
                .ok_or(MemberUseCaseError::NotFound)?;

        if current_target.membership.status != MemberStatusKind::Active {
            return Err(MemberUseCaseError::InactiveSession);
        }

        // INV-MEM-1: owner capabilities cannot be reduced
        if current_target.membership.role == MemberRole::Owner && new_caps != Capabilities::all() {
            return Err(MemberUseCaseError::Domain(
                MembershipError::OwnerCapabilitiesImmutable,
            ));
        }

        if let UpdateCaller::Device(caller_member) = caller {
            // INV-MEM-3: member cannot modify own capabilities
            if caller_member.id == target_member_id {
                return Err(MemberUseCaseError::Domain(
                    MembershipError::CannotModifyOwnCapabilities,
                ));
            }

            // Must have manage
            if !caller_member.capabilities.manage {
                return Err(MemberUseCaseError::PermissionDenied(
                    "caller lacks manage capability",
                ));
            }

            // INV-MEM-2: new capabilities must be a subset of caller capabilities
            if !new_caps.is_subset_of(&caller_member.capabilities) {
                return Err(MemberUseCaseError::Domain(
                    MembershipError::CannotEscalateCapabilities,
                ));
            }
        }

        // INV-MEM-4: withdrawing publish when sharing=true stops sharing immediately
        let stop_sharing = current_target.membership.sharing && !new_caps.publish;

        let outcome = MembershipRepository::update_capabilities(
            pool,
            tenant_id,
            session_id,
            target_member_id,
            UpdateCapabilitiesParams {
                expected_version,
                new_caps,
                stop_sharing,
                now,
            },
        )
        .await?;

        match outcome {
            UpdateCapabilitiesOutcome::Success(m) => {
                let (actor_kind, actor_id, actor_sub_id) = match caller {
                    UpdateCaller::Application => (ActorKind::Application, None, None),
                    UpdateCaller::Device(cm) => (
                        ActorKind::Device,
                        Some(cm.id.into_uuid()),
                        Some(cm.subject_id),
                    ),
                };
                let _ = record_audit(
                    pool,
                    tenant_id,
                    actor_kind,
                    actor_id,
                    actor_sub_id,
                    "member.updated",
                    Some("member"),
                    Some(target_member_id.into_uuid()),
                    AuditOutcome::Success,
                    None,
                    serde_json::json!({
                        "session_id": session_id.to_string(),
                        "can_publish": new_caps.publish,
                        "can_read": new_caps.read,
                        "can_invite": new_caps.invite,
                        "can_manage": new_caps.manage,
                    }),
                )
                .await;
                if stop_sharing {
                    let _ = record_audit(
                        pool,
                        tenant_id,
                        actor_kind,
                        actor_id,
                        Some(m.membership.subject_id),
                        "sharing.stopped",
                        Some("member"),
                        Some(target_member_id.into_uuid()),
                        AuditOutcome::Success,
                        None,
                        serde_json::json!({
                            "session_id": session_id.to_string(),
                            "sharing": false,
                            "reason": "publish_withdrawn",
                        }),
                    )
                    .await;
                }
                return Ok(MemberResource::from_member_with_subject(&m, false, None));
            }
            UpdateCapabilitiesOutcome::NotFound => return Err(MemberUseCaseError::NotFound),
            UpdateCapabilitiesOutcome::NotActive => {
                return Err(MemberUseCaseError::InactiveSession);
            }
            UpdateCapabilitiesOutcome::PreconditionFailed => {
                return Err(MemberUseCaseError::PreconditionFailed);
            }
        }
    }

    // If only role was provided without OWNER (e.g. setting MEMBER on an existing MEMBER)
    let member = MembershipRepository::get_by_id(pool, tenant_id, session_id, target_member_id)
        .await?
        .ok_or(MemberUseCaseError::NotFound)?;

    Ok(MemberResource::from_member_with_subject(
        &member, false, None,
    ))
}

/// Revokes a member (idempotent 204, FR-MEM-006, docs/07 §9.3).
pub async fn revoke_member(
    pool: &PgPool,
    tenant_id: TenantId,
    session_id: SessionId,
    target_member_id: MemberId,
) -> Result<(), MemberUseCaseError> {
    let now = OffsetDateTime::now_utc();
    let outcome =
        MembershipRepository::revoke_member(pool, tenant_id, session_id, target_member_id, now)
            .await?;

    match outcome {
        RevokeOutcome::NotFound => Err(MemberUseCaseError::NotFound),
        RevokeOutcome::AlreadyEnded => Ok(()), // Idempotent 204
        RevokeOutcome::Revoked {
            member,
            session_closed,
            ..
        } => {
            let _ = record_audit(
                pool,
                tenant_id,
                ActorKind::Application,
                None,
                Some(member.membership.subject_id),
                "member.revoked",
                Some("member"),
                Some(target_member_id.into_uuid()),
                AuditOutcome::Success,
                None,
                serde_json::json!({
                    "session_id": session_id.to_string(),
                    "session_closed": session_closed,
                }),
            )
            .await;
            if session_closed {
                let _ = record_audit(
                    pool,
                    tenant_id,
                    ActorKind::System,
                    None,
                    Some(member.membership.subject_id),
                    "session.closed",
                    Some("session"),
                    Some(session_id.into_uuid()),
                    AuditOutcome::Success,
                    None,
                    serde_json::json!({
                        "reason": "owner_left",
                    }),
                )
                .await;
            }
            Ok(())
        }
    }
}

/// Voluntarily leaves a session (device caller only, docs/07 §9.3, FR-SES-007).
pub async fn leave_session(
    pool: &PgPool,
    tenant_id: TenantId,
    session_id: SessionId,
    caller_subject_id: SubjectId,
) -> Result<(), MemberUseCaseError> {
    let now = OffsetDateTime::now_utc();
    let outcome =
        MembershipRepository::leave_session(pool, tenant_id, session_id, caller_subject_id, now)
            .await?;

    match outcome {
        LeaveOutcome::NotFound => Err(MemberUseCaseError::NotFound),
        LeaveOutcome::Left {
            member,
            session_closed,
            ..
        } => {
            let _ = record_audit(
                pool,
                tenant_id,
                ActorKind::Device,
                None,
                Some(caller_subject_id),
                "member.left",
                Some("member"),
                Some(member.membership.id.into_uuid()),
                AuditOutcome::Success,
                None,
                serde_json::json!({
                    "session_id": session_id.to_string(),
                    "session_closed": session_closed,
                }),
            )
            .await;
            if session_closed {
                let _ = record_audit(
                    pool,
                    tenant_id,
                    ActorKind::System,
                    None,
                    Some(caller_subject_id),
                    "session.closed",
                    Some("session"),
                    Some(session_id.into_uuid()),
                    AuditOutcome::Success,
                    None,
                    serde_json::json!({
                        "reason": "owner_left",
                    }),
                )
                .await;
            }
            Ok(())
        }
    }
}

/// Activates or stops location sharing for a member (FR-SHR-001..004, docs/05 §7, docs/04 §7.4).
pub async fn set_sharing<E: EphemeralStore, B: Bus>(
    pool: &PgPool,
    tenant_id: TenantId,
    session_id: SessionId,
    target_member_id: MemberId,
    desired_sharing: bool,
    ctx: SetSharingContext<'_, E, B>,
) -> Result<MemberResource, MemberUseCaseError> {
    let now = OffsetDateTime::now_utc();

    let outcome = MembershipRepository::set_sharing(
        pool,
        tenant_id,
        session_id,
        target_member_id,
        StoreSetSharingParams {
            desired_sharing,
            caller_subject_id: ctx.caller.subject_id,
            publisher_device_id: ctx.caller.device_id,
            caller_is_app: ctx.caller.is_app,
            caller_has_manage: ctx.caller.has_manage,
            now,
        },
    )
    .await?;

    match outcome {
        SetSharingOutcome::Success { member, transition } => {
            // Apply ephemeral and bus effects (docs/04 §7.4, docs/11 §2.1)
            let mver_key = format!("{}:mver:{session_id}", ctx.key_prefix);
            let new_mver = match ctx.ephemeral.increment_membership_version(&mver_key).await {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to increment membership version in ephemeral store");
                    1
                }
            };

            let ctl_topic = format!("{}:bus:ctl:{session_id}", ctx.key_prefix);

            match transition {
                SharingTransition::Stopped => {
                    // Delete snapshot immediately, retrying up to 3 times (NFR-PRIV-002, docs/11 §2.1)
                    let snap_key = format!(
                        "{}:snap:{session_id}:{}",
                        ctx.key_prefix, member.membership.id
                    );
                    for attempt in 0..3 {
                        match ctx.ephemeral.delete_snapshot(&snap_key).await {
                            Ok(_) => break,
                            Err(e) => {
                                tracing::warn!(attempt, error = %e, "failed to delete ephemeral snapshot");
                            }
                        }
                    }

                    let payload = serde_json::to_vec(&serde_json::json!({
                        "event": "SharingStopped",
                        "session_id": session_id.to_string(),
                        "member_id": member.membership.id.to_string(),
                        "external_subject_id": member.external_subject_id,
                        "sharing": false,
                        "mver": new_mver,
                        "occurred_at": format_rfc3339(now),
                    }))
                    .unwrap_or_default();

                    if let Err(e) = ctx.bus.publish(&ctl_topic, &payload).await {
                        tracing::warn!(error = %e, "failed to publish SharingStopped control event on bus");
                    }

                    let (actor_kind, actor_id) = if ctx.caller.is_app {
                        (ActorKind::Application, None)
                    } else {
                        (
                            ActorKind::Device,
                            ctx.caller.device_id.map(|d| d.into_uuid()),
                        )
                    };
                    let _ = record_audit(
                        pool,
                        tenant_id,
                        actor_kind,
                        actor_id,
                        Some(member.membership.subject_id),
                        "sharing.stopped",
                        Some("member"),
                        Some(member.membership.id.into_uuid()),
                        AuditOutcome::Success,
                        None,
                        serde_json::json!({
                            "session_id": session_id.to_string(),
                            "sharing": false,
                        }),
                    )
                    .await;
                }
                SharingTransition::Started(publisher_device) => {
                    let payload = serde_json::to_vec(&serde_json::json!({
                        "event": "SharingStarted",
                        "session_id": session_id.to_string(),
                        "member_id": member.membership.id.to_string(),
                        "external_subject_id": member.external_subject_id,
                        "sharing": true,
                        "mver": new_mver,
                        "occurred_at": format_rfc3339(now),
                    }))
                    .unwrap_or_default();

                    if let Err(e) = ctx.bus.publish(&ctl_topic, &payload).await {
                        tracing::warn!(error = %e, "failed to publish SharingStarted control event on bus");
                    }

                    let _ = record_audit(
                        pool,
                        tenant_id,
                        ActorKind::Device,
                        Some(publisher_device.into_uuid()),
                        Some(member.membership.subject_id),
                        "sharing.started",
                        Some("member"),
                        Some(member.membership.id.into_uuid()),
                        AuditOutcome::Success,
                        None,
                        serde_json::json!({
                            "session_id": session_id.to_string(),
                            "sharing": true,
                            "publisher_device_id": publisher_device.to_string(),
                        }),
                    )
                    .await;
                }
                SharingTransition::Transferred {
                    previous_device,
                    new_device,
                } => {
                    let payload = serde_json::to_vec(&serde_json::json!({
                        "event": "SharingTransferred",
                        "session_id": session_id.to_string(),
                        "member_id": member.membership.id.to_string(),
                        "external_subject_id": member.external_subject_id,
                        "previous_device_id": previous_device.to_string(),
                        "new_device_id": new_device.to_string(),
                        "sharing": true,
                        "mver": new_mver,
                        "occurred_at": format_rfc3339(now),
                    }))
                    .unwrap_or_default();

                    if let Err(e) = ctx.bus.publish(&ctl_topic, &payload).await {
                        tracing::warn!(error = %e, "failed to publish SharingTransferred control event on bus");
                    }

                    let _ = record_audit(
                        pool,
                        tenant_id,
                        ActorKind::Device,
                        Some(new_device.into_uuid()),
                        Some(member.membership.subject_id),
                        "sharing.started",
                        Some("member"),
                        Some(member.membership.id.into_uuid()),
                        AuditOutcome::Success,
                        None,
                        serde_json::json!({
                            "session_id": session_id.to_string(),
                            "sharing": true,
                            "publisher_device_id": new_device.to_string(),
                            "previous_device_id": previous_device.to_string(),
                        }),
                    )
                    .await;
                }
                SharingTransition::Unchanged => {}
            }

            Ok(MemberResource::from_member_with_subject(
                &member, false, None,
            ))
        }
        SetSharingOutcome::Unchanged(member) => Ok(MemberResource::from_member_with_subject(
            &member, false, None,
        )),
        SetSharingOutcome::SessionNotFound | SetSharingOutcome::MemberNotFound => {
            Err(MemberUseCaseError::NotFound)
        }
        SetSharingOutcome::SessionNotActive | SetSharingOutcome::MemberNotActive => {
            Err(MemberUseCaseError::InactiveSession)
        }
        SetSharingOutcome::OnlySelfCanActivateSharing => Err(MemberUseCaseError::Domain(
            MembershipError::OnlySelfCanActivateSharing,
        )),
        SetSharingOutcome::MissingPublishCapability => Err(MemberUseCaseError::Domain(
            MembershipError::MissingPublishCapability,
        )),
        SetSharingOutcome::PermissionDenied => Err(MemberUseCaseError::PermissionDenied(
            "caller is not allowed to stop sharing for this member",
        )),
    }
}
