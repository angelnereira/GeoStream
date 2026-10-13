//! Session use cases: create, read, list, update and close (docs/05 §4, docs/07 §9.2,
//! FR-SES-001..FR-SES-005).
//!
//! Expiry is verified lazily on every read (INV-SES-2), closing is idempotent (INV-SES-3) and
//! a session that is not `ACTIVE` rejects publications, subscriptions, joins and changes
//! (INV-SES-1). Optimistic concurrency uses the `version` of `docs/05` §4.1 as `If-Match`.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::time::Duration;

use gs_domain::error::{AsErrorCode, ErrorCode};
use gs_domain::ids::{DeviceId, SessionId, SubjectId, TenantId};
use gs_domain::session::{Actor, ClosedReason, Session, SessionError, SessionMode, SessionStatus};
pub use gs_store::repos::{SessionFilters, SessionPage};
use gs_store::repos::{SessionRepository, SessionUpdate};
use gs_store::{PgPool, StoreError};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors of the session use cases, mapped to the catalog of `docs/09` §6.
#[derive(Debug, Error)]
pub enum SessionUseCaseError {
    /// The request body is invalid (docs/09 §2.1, docs/05 §4.1).
    #[error("invalid session request: {0}")]
    Validation(String),

    /// The caller lacks the required scope or capability.
    #[error("insufficient scope")]
    Forbidden,

    /// The caller lacks the required capability.
    #[error("member lacks manage capability")]
    PermissionDenied,

    /// The session does not exist in this tenant.
    #[error("session not found")]
    NotFound,

    /// The session is not `ACTIVE` (INV-SES-1).
    #[error("session is not active")]
    InactiveSession,

    /// `If-Match` did not match the current version (docs/07 §9.2).
    #[error("session version does not match the If-Match header")]
    PreconditionFailed,

    /// Reducing `max_members` below the active members count (docs/05 §4.1).
    #[error("max_members cannot be lower than the active members count")]
    MaxMembersConflict,

    /// The session is full (FR-SES-006).
    #[error("session is full")]
    SessionFull,

    /// A domain rule rejected the change.
    #[error("domain rule violated: {0}")]
    Domain(#[from] SessionError),

    /// A persistence operation failed.
    #[error("storage error: {0}")]
    Store(#[from] StoreError),
}

impl AsErrorCode for SessionUseCaseError {
    fn error_code(&self) -> ErrorCode {
        match self {
            Self::Validation(_) => ErrorCode::ValidationFailed,
            Self::Forbidden => ErrorCode::InsufficientScope,
            Self::PermissionDenied => ErrorCode::PermissionDenied,
            Self::NotFound => ErrorCode::NotFound,
            Self::InactiveSession | Self::Domain(_) => ErrorCode::Conflict,
            Self::PreconditionFailed => ErrorCode::PreconditionFailed,
            Self::MaxMembersConflict | Self::SessionFull => ErrorCode::Conflict,
            Self::Store(_) => ErrorCode::Internal,
        }
    }
}

/// Actor object matching OpenAPI `Actor` schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorObject {
    #[serde(rename = "type")]
    pub actor_type: String,
    pub id: String,
}

/// A session as seen by the API (docs/05 §4.1, OpenAPI `Session`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionResource {
    /// `ses_…` identifier.
    pub id: String,
    /// `ten_…` identifier.
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub tenant_id: String,
    /// Session mode.
    pub mode: SessionMode,
    /// Session status, with lazy expiry already applied (INV-SES-2).
    pub status: SessionStatus,
    /// Owner member id, or null when the session is application-owned (docs/07 §9.2).
    pub owner_member_id: Option<String>,
    /// Owner subject when a device created the session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_subject_id: Option<String>,
    /// Creating actor object matching OpenAPI.
    pub created_by: ActorObject,
    /// Flat actor kind for internal backwards compatibility.
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub created_by_kind: String,
    /// Flat actor id for internal backwards compatibility.
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub created_by_id: String,
    /// RFC 3339 creation instant.
    pub created_at: String,
    /// RFC 3339 update instant.
    pub updated_at: String,
    /// RFC 3339 expiry instant.
    pub expires_at: String,
    /// Member limit.
    pub max_members: u32,
    /// Snapshot TTL; `0` disables snapshots.
    pub snapshot_ttl_seconds: u32,
    /// Staleness hint for clients.
    pub stale_after_seconds: u32,
    /// Free-form metadata, never with personal data (docs/05 §4.1).
    pub metadata: BTreeMap<String, String>,
    /// Active member count.
    pub active_member_count: u32,
    /// Closure reason when the session is not `ACTIVE`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_reason: Option<ClosedReason>,
    /// Closure instant when the session is not `ACTIVE`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<String>,
    /// Version used for `If-Match` and `ETag`.
    pub version: u64,
}

impl SessionResource {
    /// Projects a stored session into the API resource.
    pub fn from_session(session: &Session) -> Self {
        let (created_by_kind, created_by_id) = match session.created_by {
            Actor::Device(device_id) => ("DEVICE".to_string(), device_id.to_string()),
            Actor::Application(application_id) => {
                ("APPLICATION".to_string(), application_id.to_string())
            }
        };

        Self {
            id: session.id.to_string(),
            tenant_id: session.tenant_id.to_string(),
            mode: session.mode,
            status: session.status,
            owner_member_id: None,
            owner_subject_id: session.owner_subject_id.map(|id| id.to_string()),
            created_by: ActorObject {
                actor_type: created_by_kind.clone(),
                id: created_by_id.clone(),
            },
            created_by_kind,
            created_by_id,
            created_at: format_rfc3339(session.created_at),
            updated_at: format_rfc3339(session.created_at),
            expires_at: format_rfc3339(session.expires_at),
            max_members: session.max_members,
            snapshot_ttl_seconds: session.snapshot_ttl_seconds,
            stale_after_seconds: session.stale_after_seconds,
            metadata: session.metadata.clone(),
            active_member_count: if session.owner_subject_id.is_some() {
                1
            } else {
                0
            },
            closed_reason: session.closed_reason,
            closed_at: None,
            version: session.version,
        }
    }

    /// Projects a stored session into the API resource with live counts from PostgreSQL.
    pub async fn from_session_with_counts(
        pool: &PgPool,
        session: &Session,
    ) -> Result<Self, StoreError> {
        let mut resource = Self::from_session(session);
        let active_count = gs_store::repos::MembershipRepository::count_active_in_session(
            pool,
            session.tenant_id,
            session.id,
        )
        .await?;
        resource.active_member_count = active_count as u32;

        if let Some(owner_sub_id) = session.owner_subject_id
            && let Some(membership) =
                gs_store::repos::MembershipRepository::find_active_by_session_and_subject(
                    pool,
                    session.tenant_id,
                    session.id,
                    owner_sub_id,
                )
                .await?
        {
            resource.owner_member_id = Some(membership.id.to_string());
        }
        Ok(resource)
    }

    /// ETag value of the resource (docs/07 §2).
    pub fn etag(&self) -> String {
        format!("\"{}\"", self.version)
    }
}

/// Initial member in `CreateSessionRequest` (OpenAPI `InitialMember`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitialMemberRequest {
    /// External subject identifier.
    pub external_subject_id: String,
    /// Explicit capabilities (defaults to the mode's defaults).
    #[serde(default)]
    pub capabilities: Option<gs_domain::membership::Capabilities>,
}

/// Body of `POST /v1/sessions` (OpenAPI `CreateSessionRequest`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateSessionRequest {
    /// Session mode.
    pub mode: SessionMode,
    /// Expiry duration in seconds (OpenAPI `CreateSessionRequest`).
    #[serde(default)]
    pub expires_in_seconds: Option<u64>,
    /// Expiry instant, RFC 3339.
    #[serde(default)]
    pub expires_at: Option<String>,
    /// Member limit; `ONE_TO_ONE` requires 2.
    #[serde(default)]
    pub max_members: Option<u32>,
    /// Snapshot TTL; defaults to the tenant policy.
    #[serde(default)]
    pub snapshot_ttl_seconds: Option<u32>,
    /// Staleness hint; defaults to the tenant policy.
    #[serde(default)]
    pub stale_after_seconds: Option<u32>,
    /// Free-form metadata (docs/05 §4.1 limits).
    #[serde(default)]
    pub metadata: Option<BTreeMap<String, String>>,
    /// Owner external subject id (application only, docs/07 §9.2).
    #[serde(default)]
    pub owner_external_subject_id: Option<String>,
    /// Initial members (application only, docs/07 §9.2).
    #[serde(default)]
    pub initial_members: Option<Vec<InitialMemberRequest>>,
}

/// Body of `PATCH /v1/sessions/{session_id}` (docs/07 §9.2).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateSessionRequest {
    /// New expiry instant.
    #[serde(default)]
    pub expires_at: Option<String>,
    /// New member limit.
    #[serde(default)]
    pub max_members: Option<u32>,
    /// New snapshot TTL.
    #[serde(default)]
    pub snapshot_ttl_seconds: Option<u32>,
    /// New staleness hint.
    #[serde(default)]
    pub stale_after_seconds: Option<u32>,
    /// New metadata (replaces the previous one).
    #[serde(default)]
    pub metadata: Option<BTreeMap<String, String>>,
}

/// Effective policy values a session needs (docs/17 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionPolicy {
    /// Default member limit of `ONE_TO_MANY`.
    pub default_max_members_one_to_many: u32,
    /// Default member limit of `MANY_TO_MANY`.
    pub default_max_members_many_to_many: u32,
    /// Member limit ceiling.
    pub max_members_cap: u32,
    /// Default snapshot TTL.
    pub snapshot_ttl_default_seconds: u32,
    /// Maximum snapshot TTL.
    pub snapshot_ttl_max_seconds: u32,
    /// Default staleness hint.
    pub stale_after_seconds_default: u32,
    /// Maximum session TTL.
    pub max_session_ttl_seconds: u32,
}

impl SessionPolicy {
    /// Extracts the values a session needs from the tenant policy.
    pub fn from_policy(policy: &gs_auth::TenantPolicy) -> Self {
        Self {
            default_max_members_one_to_many: policy.default_max_members_one_to_many,
            default_max_members_many_to_many: policy.default_max_members_many_to_many,
            max_members_cap: policy.max_members_cap,
            snapshot_ttl_default_seconds: policy.snapshot_ttl_default_seconds as u32,
            snapshot_ttl_max_seconds: policy.snapshot_ttl_max_seconds as u32,
            stale_after_seconds_default: policy.stale_after_seconds_default as u32,
            max_session_ttl_seconds: policy.max_session_ttl_seconds as u32,
        }
    }

    /// Default member limit of a mode (docs/05 §4.2).
    pub const fn default_max_members(&self, mode: SessionMode) -> u32 {
        match mode {
            SessionMode::OneToOne => 2,
            SessionMode::OneToMany => self.default_max_members_one_to_many,
            SessionMode::ManyToMany => self.default_max_members_many_to_many,
        }
    }
}

/// Reads a session applying lazy expiry (INV-SES-2).
pub async fn get_session(
    pool: &PgPool,
    tenant_id: TenantId,
    session_id: SessionId,
) -> Result<Session, SessionUseCaseError> {
    let session = SessionRepository::get_by_id(pool, tenant_id, session_id)
        .await?
        .ok_or(SessionUseCaseError::NotFound)?;

    apply_lazy_expiry(pool, session).await
}

/// Applies INV-SES-2: an `ACTIVE` session past its expiry is treated as `EXPIRED`.
async fn apply_lazy_expiry(
    pool: &PgPool,
    session: Session,
) -> Result<Session, SessionUseCaseError> {
    if session.status != SessionStatus::Active
        || session.expires_at > time::OffsetDateTime::now_utc()
    {
        return Ok(session);
    }

    let now = time::OffsetDateTime::now_utc();
    Ok(
        SessionRepository::mark_expired(pool, session.tenant_id, session.id, now)
            .await?
            .unwrap_or(session),
    )
}

/// Lists the sessions visible to a principal (docs/07 §9.2, FR-SES-002).
pub async fn list_sessions_for_subject(
    pool: &PgPool,
    tenant_id: TenantId,
    subject_id: SubjectId,
    limit: u32,
    cursor: Option<(time::OffsetDateTime, uuid::Uuid)>,
) -> Result<SessionPage, SessionUseCaseError> {
    Ok(SessionRepository::list_by_subject(
        pool,
        tenant_id,
        subject_id,
        i64::from(limit) + 1,
        cursor,
    )
    .await?)
}

/// Lists the sessions of a tenant with the documented filters (docs/07 §9.2).
pub async fn list_sessions_for_application(
    pool: &PgPool,
    tenant_id: TenantId,
    filters: &SessionFilters,
    limit: u32,
    cursor: Option<(time::OffsetDateTime, uuid::Uuid)>,
) -> Result<SessionPage, SessionUseCaseError> {
    Ok(
        SessionRepository::list_by_tenant(pool, tenant_id, filters, i64::from(limit) + 1, cursor)
            .await?,
    )
}

/// Creates a session and the owner membership when a device creates it (docs/05 §4,
/// FR-SES-001, FR-SES-006).
pub async fn create_session(
    pool: &PgPool,
    tenant_id: TenantId,
    actor: Actor,
    request: &CreateSessionRequest,
    policy: SessionPolicy,
) -> Result<Session, SessionUseCaseError> {
    let now = time::OffsetDateTime::now_utc();
    let expires_at = if let Some(secs) = request.expires_in_seconds {
        if secs < 60 {
            return Err(SessionUseCaseError::Validation(
                "expires_in_seconds must be >= 60".to_string(),
            ));
        }
        let ttl_secs = secs.min(u64::from(policy.max_session_ttl_seconds));
        now + time::Duration::seconds(ttl_secs as i64)
    } else if let Some(ref exp_str) = request.expires_at {
        parse_rfc3339(exp_str)?
    } else {
        now + time::Duration::seconds(i64::from(policy.max_session_ttl_seconds))
    };

    let max_members = match request.max_members {
        Some(max_members) => max_members,
        None => policy.default_max_members(request.mode),
    };

    let snapshot_ttl_seconds = match request.snapshot_ttl_seconds {
        Some(seconds) => seconds,
        None => policy.snapshot_ttl_default_seconds,
    };
    if u64::from(snapshot_ttl_seconds) > u64::from(policy.snapshot_ttl_max_seconds) {
        return Err(SessionUseCaseError::Validation(format!(
            "snapshot_ttl_seconds must be at most {}",
            policy.snapshot_ttl_max_seconds
        )));
    }

    let stale_after_seconds = match request.stale_after_seconds {
        Some(seconds) => seconds,
        None => policy.stale_after_seconds_default,
    };

    let metadata = request.metadata.clone().unwrap_or_default();
    validate_metadata(&metadata)?;

    let owner_subject_id = match actor {
        Actor::Device(device_id) => {
            if request.owner_external_subject_id.is_some() {
                return Err(SessionUseCaseError::Validation(
                    "owner_external_subject_id is only allowed for application callers".to_string(),
                ));
            }
            if request.initial_members.is_some() {
                return Err(SessionUseCaseError::Validation(
                    "initial_members is only allowed for application callers".to_string(),
                ));
            }
            Some(subject_of_device(pool, tenant_id, device_id).await?)
        }
        Actor::Application(_) => {
            if let Some(ref owner_ext_id) = request.owner_external_subject_id {
                let subject = gs_store::repos::SubjectRepository::get_or_create_active(
                    pool,
                    tenant_id,
                    owner_ext_id,
                    now,
                )
                .await?;
                Some(subject.id)
            } else {
                None
            }
        }
    };

    // `Session::new` re-applies every rule of docs/05 §4.1 against the effective policy.
    let session = Session::new(
        SessionId::new(),
        tenant_id,
        request.mode,
        owner_subject_id,
        actor.clone(),
        now,
        expires_at,
        max_members.min(policy.max_members_cap),
        snapshot_ttl_seconds,
        stale_after_seconds,
        metadata,
        policy.max_session_ttl_seconds,
    )?;

    SessionRepository::create(pool, &session).await?;

    // A session with an owner subject gets an owner membership (docs/05 §4.1, INV-MEM-1).
    if let Some(owner_sub_id) = owner_subject_id {
        let membership = gs_domain::membership::Membership::new_owner(
            gs_domain::ids::MemberId::new(),
            session.id,
            tenant_id,
            owner_sub_id,
            now,
        );
        gs_store::repos::MembershipRepository::create(pool, &membership).await?;
    }

    // Application caller can add initial members (docs/07 §9.2).
    if let Some(ref initial_members) = request.initial_members {
        for m in initial_members {
            let subject = gs_store::repos::SubjectRepository::get_or_create_active(
                pool,
                tenant_id,
                &m.external_subject_id,
                now,
            )
            .await?;

            if Some(subject.id) == owner_subject_id {
                continue;
            }

            let caps = m.capabilities.unwrap_or_else(|| {
                gs_domain::membership::Capabilities::default_for_mode(request.mode, false)
            });
            let membership = gs_domain::membership::Membership::new_member(
                gs_domain::ids::MemberId::new(),
                session.id,
                tenant_id,
                subject.id,
                caps,
                gs_domain::membership::JoinedVia::DirectAdd,
                now,
            );
            gs_store::repos::MembershipRepository::create(pool, &membership).await?;
        }
    }

    Ok(session)
}

/// Updates a session, honouring `If-Match` (docs/07 §9.2, FR-SES-003).
pub async fn update_session(
    pool: &PgPool,
    tenant_id: TenantId,
    session_id: SessionId,
    expected_version: Option<u64>,
    request: &UpdateSessionRequest,
    policy: SessionPolicy,
) -> Result<Session, SessionUseCaseError> {
    let current = get_session(pool, tenant_id, session_id).await?;
    if current.status != SessionStatus::Active {
        return Err(SessionUseCaseError::InactiveSession);
    }

    let expected_version = expected_version.unwrap_or(current.version);
    if expected_version != current.version {
        return Err(SessionUseCaseError::PreconditionFailed);
    }

    let expires_at = match request.expires_at.as_deref() {
        Some(value) => Some(parse_rfc3339(value)?),
        None => None,
    };
    if let Some(expires_at) = expires_at {
        let max_ttl =
            current.created_at + time::Duration::seconds(i64::from(policy.max_session_ttl_seconds));
        if expires_at <= time::OffsetDateTime::now_utc() {
            return Err(SessionUseCaseError::Validation(
                "expires_at must be in the future".to_string(),
            ));
        }
        if expires_at > max_ttl {
            return Err(SessionUseCaseError::Validation(format!(
                "expires_at must be within max_session_ttl_seconds ({})",
                policy.max_session_ttl_seconds
            )));
        }
    }

    if let Some(max_members) = request.max_members {
        if max_members < 2 {
            return Err(SessionUseCaseError::Domain(
                SessionError::InvalidMaxMembers("max_members must be >= 2".to_string()),
            ));
        }
        let active = SessionRepository::count_active_members(pool, tenant_id, session_id).await?;
        if u64::from(max_members) < active {
            return Err(SessionUseCaseError::MaxMembersConflict);
        }
    }

    if let Some(snapshot_ttl_seconds) = request.snapshot_ttl_seconds
        && u64::from(snapshot_ttl_seconds) > u64::from(policy.snapshot_ttl_max_seconds)
    {
        return Err(SessionUseCaseError::Validation(format!(
            "snapshot_ttl_seconds must be at most {}",
            policy.snapshot_ttl_max_seconds
        )));
    }

    if let Some(metadata) = &request.metadata {
        validate_metadata(metadata)?;
    }

    let update = SessionUpdate {
        expires_at,
        max_members: request.max_members,
        snapshot_ttl_seconds: request.snapshot_ttl_seconds,
        stale_after_seconds: request.stale_after_seconds,
        metadata: request.metadata.as_ref().map(|metadata| {
            serde_json::to_value(metadata).unwrap_or_else(|_| serde_json::json!({}))
        }),
    };

    let updated = SessionRepository::update_if_version_matches(
        pool,
        tenant_id,
        session_id,
        expected_version,
        &update,
        time::OffsetDateTime::now_utc(),
    )
    .await?
    .ok_or(SessionUseCaseError::PreconditionFailed)?;

    Ok(updated)
}

/// Closes a session: idempotent and with the effects of docs/05 §4.3 (FR-SES-004).
pub async fn close_session(
    pool: &PgPool,
    tenant_id: TenantId,
    session_id: SessionId,
    reason: ClosedReason,
) -> Result<Session, SessionUseCaseError> {
    let current = SessionRepository::get_by_id(pool, tenant_id, session_id)
        .await?
        .ok_or(SessionUseCaseError::NotFound)?;

    if current.status != SessionStatus::Active {
        // Closing a session that is already CLOSED or EXPIRED returns its state (INV-SES-3).
        return Ok(current);
    }

    let closed = SessionRepository::close(
        pool,
        tenant_id,
        session_id,
        reason,
        time::OffsetDateTime::now_utc(),
    )
    .await?;

    Ok(closed.unwrap_or(current))
}

/// Verifies that a device's subject is an active member of the session (docs/07 §9.2).
pub async fn verify_device_active_member(
    pool: &PgPool,
    tenant_id: TenantId,
    session_id: SessionId,
    subject_id: SubjectId,
) -> Result<(), SessionUseCaseError> {
    match gs_store::repos::MembershipRepository::find_active_by_session_and_subject(
        pool, tenant_id, session_id, subject_id,
    )
    .await?
    {
        Some(_) => Ok(()),
        None => Err(SessionUseCaseError::NotFound),
    }
}

/// Verifies that a device's subject is an active member with manage capability (docs/07 §9.2).
pub async fn verify_device_manage_capability(
    pool: &PgPool,
    tenant_id: TenantId,
    session_id: SessionId,
    subject_id: SubjectId,
) -> Result<(), SessionUseCaseError> {
    match gs_store::repos::MembershipRepository::find_active_by_session_and_subject(
        pool, tenant_id, session_id, subject_id,
    )
    .await?
    {
        Some(membership) => {
            if membership.capabilities.manage {
                Ok(())
            } else {
                Err(SessionUseCaseError::PermissionDenied)
            }
        }
        None => Err(SessionUseCaseError::NotFound),
    }
}

/// Expires the sessions past their expiry (worker job, FR-SES-005).
pub async fn expire_due_sessions(
    pool: &PgPool,
    batch_limit: u32,
) -> Result<Vec<SessionId>, SessionUseCaseError> {
    let now = time::OffsetDateTime::now_utc();
    Ok(SessionRepository::expire_due(pool, now, i64::from(batch_limit)).await?)
}

/// Expiry loop: marks sessions past their expiry until the task is cancelled (FR-SES-005).
pub async fn expiry_loop(pool: &PgPool, interval: Duration, batch_limit: u32) {
    loop {
        match expire_due_sessions(pool, batch_limit).await {
            Ok(expired) => {
                for session_id in expired {
                    tracing::info!(session_id = %session_id, "session expired");
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, "session expiry run failed");
            }
        }
        tokio::time::sleep(interval).await;
    }
}

/// Validates the metadata limits of docs/05 §4.1.
fn validate_metadata(metadata: &BTreeMap<String, String>) -> Result<(), SessionUseCaseError> {
    if metadata.len() > 32 {
        return Err(SessionUseCaseError::Validation(
            "metadata must have at most 32 entries".to_string(),
        ));
    }
    for (key, value) in metadata {
        if key.is_empty() || key.len() > 64 {
            return Err(SessionUseCaseError::Validation(format!(
                "metadata key '{key}' must be 1..=64 characters"
            )));
        }
        if !key.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'-')
        }) {
            return Err(SessionUseCaseError::Validation(format!(
                "metadata key '{key}' must match ^[a-z0-9_.-]+$"
            )));
        }
        if value.chars().count() > 512 {
            return Err(SessionUseCaseError::Validation(format!(
                "metadata value of '{key}' must be at most 512 characters"
            )));
        }
    }
    Ok(())
}

/// Parses an RFC 3339 instant (docs/00 §4.5).
fn parse_rfc3339(value: &str) -> Result<time::OffsetDateTime, SessionUseCaseError> {
    time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .map_err(|e| SessionUseCaseError::Validation(format!("invalid timestamp '{value}': {e}")))
}

/// Formats an instant as RFC 3339 (docs/00 §4.5).
fn format_rfc3339(instant: time::OffsetDateTime) -> String {
    instant
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| instant.to_string())
}

/// Resolves the subject of a device: the owner of a session it creates (docs/05 §4.1).
async fn subject_of_device(
    pool: &PgPool,
    tenant_id: TenantId,
    device_id: DeviceId,
) -> Result<SubjectId, SessionUseCaseError> {
    let device = gs_store::repos::DeviceRepository::get_by_id(pool, tenant_id, device_id)
        .await?
        .ok_or_else(|| SessionUseCaseError::Validation("device not found".to_string()))?;
    Ok(device.subject_id)
}

/// Human readable name of an actor for logs and audit trails.
pub fn actor_label(actor: &Actor) -> &'static str {
    match actor {
        Actor::Device(_) => "device",
        Actor::Application(_) => "application",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> SessionPolicy {
        SessionPolicy {
            default_max_members_one_to_many: 100,
            default_max_members_many_to_many: 50,
            max_members_cap: 1000,
            snapshot_ttl_default_seconds: 600,
            snapshot_ttl_max_seconds: 86_400,
            stale_after_seconds_default: 60,
            max_session_ttl_seconds: 604_800,
        }
    }

    fn request(mode: SessionMode) -> CreateSessionRequest {
        CreateSessionRequest {
            mode,
            expires_in_seconds: None,
            expires_at: Some("2026-10-13T12:00:00.000Z".to_string()),
            max_members: None,
            snapshot_ttl_seconds: None,
            stale_after_seconds: None,
            metadata: None,
            owner_external_subject_id: None,
            initial_members: None,
        }
    }

    #[test]
    fn test_defaults_follow_the_policy_of_docs_17_4() {
        // covers: FR-SES-001, docs/05 4.2
        assert_eq!(policy().default_max_members(SessionMode::OneToOne), 2);
        assert_eq!(policy().default_max_members(SessionMode::OneToMany), 100);
        assert_eq!(policy().default_max_members(SessionMode::ManyToMany), 50);
    }

    #[test]
    fn test_create_request_rejects_unknown_fields() {
        // covers: docs/07 1
        let error = serde_json::from_value::<CreateSessionRequest>(serde_json::json!({
            "mode": "ONE_TO_ONE",
            "expires_at": "2026-10-13T12:00:00.000Z",
            "surprise": 1
        }))
        .expect_err("unknown fields must be rejected");
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn test_metadata_limits_match_docs_05_4_1() {
        // covers: FR-SES-001, docs/05 4.1
        let mut metadata = BTreeMap::new();
        metadata.insert("fleet".to_string(), "taxis".to_string());
        assert!(validate_metadata(&metadata).is_ok());

        let mut too_many = BTreeMap::new();
        for index in 0..33 {
            too_many.insert(format!("key{index}"), "value".to_string());
        }
        assert!(validate_metadata(&too_many).is_err());

        let mut bad_key = BTreeMap::new();
        bad_key.insert("Fleet".to_string(), "taxis".to_string());
        assert!(validate_metadata(&bad_key).is_err());

        let mut long_value = BTreeMap::new();
        long_value.insert("fleet".to_string(), "x".repeat(513));
        assert!(validate_metadata(&long_value).is_err());
    }

    #[test]
    fn test_resource_projects_the_stored_session() {
        // covers: FR-SES-002, docs/07 9.2
        let session = Session::new(
            SessionId::new(),
            TenantId::new(),
            SessionMode::OneToOne,
            None,
            Actor::Device(DeviceId::new()),
            time::OffsetDateTime::now_utc(),
            time::OffsetDateTime::now_utc() + Duration::from_secs(3600),
            2,
            600,
            60,
            BTreeMap::new(),
            604_800,
        )
        .expect("session");
        let resource = SessionResource::from_session(&session);
        assert_eq!(resource.mode, SessionMode::OneToOne);
        assert_eq!(resource.status, SessionStatus::Active);
        assert_eq!(resource.created_by_kind, "DEVICE");
        assert!(resource.id.starts_with("ses_"));
        assert_eq!(resource.etag(), "\"1\"");
        let _ = request(SessionMode::OneToOne);
    }

    #[test]
    fn test_error_codes_match_docs_09_6() {
        // covers: docs/09 6, docs/07 9.2
        assert_eq!(
            SessionUseCaseError::Validation("x".to_string()).error_code(),
            ErrorCode::ValidationFailed
        );
        assert_eq!(
            SessionUseCaseError::PreconditionFailed.error_code(),
            ErrorCode::PreconditionFailed
        );
        assert_eq!(
            SessionUseCaseError::NotFound.error_code(),
            ErrorCode::NotFound
        );
        assert_eq!(
            SessionUseCaseError::InactiveSession.error_code(),
            ErrorCode::Conflict
        );
        assert_eq!(
            SessionUseCaseError::MaxMembersConflict.error_code(),
            ErrorCode::Conflict
        );
        assert_eq!(
            SessionUseCaseError::SessionFull.error_code(),
            ErrorCode::Conflict
        );
    }
}
