//! Invitation use cases (docs/05 §6, docs/07 §9.4, FR-INV-001..003).
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::time::Duration as StdDuration;
use time::{Duration, OffsetDateTime};

use gs_auth::TenantPolicy;
use gs_domain::error::{AsErrorCode, ErrorCode};
use gs_domain::ids::{
    ApplicationId, DeviceId, InvitationId, MemberId, SessionId, SubjectId, TenantId,
};
pub use gs_domain::invitation::InvitationStatus;
use gs_domain::invitation::{
    InvitationError, generate_invitation_code, hash_invitation_code_bytes,
    normalize_invitation_code,
};
use gs_domain::membership::{Capabilities, MemberRole};
use gs_domain::pagination::PageCursor;
use gs_domain::session::{Actor, SessionStatus};
use gs_ephemeral::{EphemeralError, EphemeralStore};
use gs_store::PgPool;
use gs_store::error::StoreError;
use gs_store::models::{ActorKind, AuditOutcome};
use gs_store::repos::{
    CreateInvitationOutcome, CreateInvitationParams, InvitationRepository, InvitationRow,
    RedeemOutcome, SessionRepository,
};

use crate::audit::record_audit;
use crate::members::MemberResource;
use crate::sessions::{ActorObject, SessionResource};

fn format_rfc3339(instant: OffsetDateTime) -> String {
    instant
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// OpenAPI `Invitation` resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvitationResource {
    pub id: String,
    pub session_id: String,
    pub grants: Capabilities,
    pub max_uses: u32,
    pub uses: u32,
    pub expires_at: String,
    pub status: InvitationStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<ActorObject>,
    pub created_at: String,
    pub revoked_at: Option<String>,
}

impl InvitationResource {
    pub fn from_row(row: &InvitationRow, now: OffsetDateTime) -> Self {
        let (actor_type, actor_id_str) = match row.created_by {
            Actor::Device(id) => ("DEVICE".to_string(), id.to_string()),
            Actor::Application(id) => ("APPLICATION".to_string(), id.to_string()),
        };

        Self {
            id: row.id.to_string(),
            session_id: row.session_id.to_string(),
            grants: row.grants,
            max_uses: row.max_uses,
            uses: row.uses,
            expires_at: format_rfc3339(row.expires_at),
            status: row.status_at(now),
            created_by: Some(ActorObject {
                actor_type,
                id: actor_id_str,
            }),
            created_at: format_rfc3339(row.created_at),
            revoked_at: row.revoked_at.map(format_rfc3339),
        }
    }
}

/// OpenAPI `InvitationCreated` resource: contains raw code presented only once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvitationCreatedResource {
    #[serde(flatten)]
    pub invitation: InvitationResource,
    pub code: String,
}

/// OpenAPI `InvitationList` resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvitationListResource {
    pub data: Vec<InvitationResource>,
    pub next_cursor: Option<String>,
}

/// OpenAPI `CreateInvitationRequest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CreateInvitationRequest {
    pub grants: Option<Capabilities>,
    pub max_uses: Option<u32>,
    pub expires_in_seconds: Option<u64>,
}

/// OpenAPI `RedeemInvitationRequest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedeemInvitationRequest {
    pub code: String,
}

/// OpenAPI `RedeemInvitationResponse`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedeemInvitationResponse {
    pub session: SessionResource,
    pub member: MemberResource,
}

/// Caller identity and capabilities for creating an invitation.
#[derive(Debug, Clone)]
pub enum CreateInvitationCaller {
    Application,
    Device {
        device_id: DeviceId,
        subject_id: SubjectId,
        member_id: MemberId,
        role: MemberRole,
        capabilities: Capabilities,
    },
}

/// Caller identity and capabilities for listing invitations.
#[derive(Debug, Clone)]
pub enum ListInvitationsCaller {
    Application,
    Device {
        member_id: MemberId,
        capabilities: Capabilities,
    },
}

/// Caller identity and capabilities for revoking an invitation.
#[derive(Debug, Clone)]
pub enum RevokeInvitationCaller {
    Application,
    Device {
        device_id: DeviceId,
        member_id: MemberId,
        capabilities: Capabilities,
    },
}

/// Use case errors for invitation operations.
#[derive(Debug, thiserror::Error)]
pub enum InvitationUseCaseError {
    #[error("validation error: {0}")]
    Validation(String),

    #[error("permission denied: {0}")]
    PermissionDenied(&'static str),

    #[error("not found")]
    NotFound,

    #[error("session is not active")]
    InactiveSession,

    #[error("session is full")]
    SessionFull,

    #[error("conflict: subject is already an active member")]
    Conflict,

    #[error("invitation code is invalid, expired, exhausted, or revoked")]
    InvitationInvalid,

    #[error("rate limit exceeded, retry after {retry_after} seconds")]
    RateLimited { retry_after: u64 },

    #[error("domain error: {0}")]
    Domain(#[from] InvitationError),

    #[error("storage error: {0}")]
    Store(#[from] StoreError),

    #[error("ephemeral store error: {0}")]
    Ephemeral(#[from] EphemeralError),
}

impl AsErrorCode for InvitationUseCaseError {
    fn error_code(&self) -> ErrorCode {
        match self {
            Self::Validation(_) => ErrorCode::ValidationFailed,
            Self::PermissionDenied(_) => ErrorCode::PermissionDenied,
            Self::NotFound => ErrorCode::NotFound,
            Self::InactiveSession => ErrorCode::SessionClosed,
            Self::SessionFull => ErrorCode::SessionFull,
            Self::Conflict => ErrorCode::Conflict,
            Self::InvitationInvalid => ErrorCode::InvitationInvalid,
            Self::RateLimited { .. } => ErrorCode::RateLimited,
            Self::Domain(d) => match d {
                InvitationError::InvalidGrants => ErrorCode::PermissionDenied,
                _ => ErrorCode::InvitationInvalid,
            },
            Self::Store(_) | Self::Ephemeral(_) => ErrorCode::Internal,
        }
    }
}

/// Creates a new invitation (FR-INV-001, docs/05 §6, docs/07 §9.4).
pub async fn create_invitation(
    pool: &PgPool,
    tenant_id: TenantId,
    session_id: SessionId,
    request: &CreateInvitationRequest,
    caller: CreateInvitationCaller,
    policy: &TenantPolicy,
    now: OffsetDateTime,
) -> Result<InvitationCreatedResource, InvitationUseCaseError> {
    // 1. Fetch session to check existence, mode and expiry
    let session = match SessionRepository::get_by_id(pool, tenant_id, session_id).await? {
        Some(s) => s,
        None => return Err(InvitationUseCaseError::NotFound),
    };

    if session.status != SessionStatus::Active || session.expires_at <= now {
        return Err(InvitationUseCaseError::InactiveSession);
    }

    // 2. Validate and compute effective grants (FR-INV-001, docs/05 §6)
    let effective_grants = match caller {
        CreateInvitationCaller::Application => match request.grants {
            Some(g) => g,
            None => Capabilities::default_for_mode(session.mode, false),
        },
        CreateInvitationCaller::Device {
            role,
            capabilities: issuer_caps,
            ..
        } => {
            // Must have invite capability (FR-INV-001)
            if !issuer_caps.invite {
                return Err(InvitationUseCaseError::PermissionDenied(
                    "caller lacks invite capability",
                ));
            }

            match request.grants {
                Some(g) => {
                    // Grants must be a subset of issuer capabilities (docs/05 §6)
                    if (g.publish && !issuer_caps.publish)
                        || (g.read && !issuer_caps.read)
                        || (g.invite && !issuer_caps.invite)
                        || (g.manage && !issuer_caps.manage)
                    {
                        return Err(InvitationUseCaseError::PermissionDenied(
                            "grants must be a subset of caller capabilities",
                        ));
                    }

                    // manage capability only if issuer is OWNER (docs/05 §6)
                    if g.manage && role != MemberRole::Owner {
                        return Err(InvitationUseCaseError::PermissionDenied(
                            "manage capability can only be granted by the session owner or application",
                        ));
                    }

                    g
                }
                None => {
                    // Mode default clipped to issuer capabilities (docs/07 §9.4)
                    let mode_default = Capabilities::default_for_mode(session.mode, false);
                    Capabilities {
                        publish: mode_default.publish && issuer_caps.publish,
                        read: mode_default.read && issuer_caps.read,
                        invite: mode_default.invite && issuer_caps.invite,
                        manage: false,
                    }
                }
            }
        }
    };

    // 3. Validate max_uses (docs/09 §5: 1..=invitation_max_uses, default 1)
    let max_uses = match request.max_uses {
        Some(m) => {
            if m < 1 || m > policy.invitation_max_uses {
                return Err(InvitationUseCaseError::Validation(format!(
                    "max_uses must be between 1 and {}",
                    policy.invitation_max_uses
                )));
            }
            m
        }
        None => 1,
    };

    // 4. Validate expires_in_seconds (docs/09 §5: 60..=invitation_max_ttl_seconds, default 86400 clipped)
    let default_ttl_secs = 86_400u64;
    let requested_ttl = request.expires_in_seconds.unwrap_or(default_ttl_secs);
    if requested_ttl < 60 || requested_ttl > policy.invitation_max_ttl_seconds {
        return Err(InvitationUseCaseError::Validation(format!(
            "expires_in_seconds must be between 60 and {}",
            policy.invitation_max_ttl_seconds
        )));
    }

    let calculated_expires_at = now + Duration::seconds(requested_ttl as i64);
    let expires_at = if calculated_expires_at > session.expires_at {
        session.expires_at
    } else {
        calculated_expires_at
    };

    if expires_at <= now {
        return Err(InvitationUseCaseError::InactiveSession);
    }

    // 5. Generate Crockford Base32 invitation code and compute SHA-256 hash (docs/05 §6)
    let code_formatted = generate_invitation_code();
    let normalized = normalize_invitation_code(&code_formatted)
        .map_err(|e| InvitationUseCaseError::Validation(e.to_string()))?;

    let code_sha256 = hash_invitation_code_bytes(&normalized).to_vec();

    // 6. Build Actor and parameters
    let (actor, creator_member_id) = match caller {
        CreateInvitationCaller::Application => (Actor::Application(ApplicationId::new()), None),
        CreateInvitationCaller::Device {
            device_id,
            member_id,
            ..
        } => (Actor::Device(device_id), Some(member_id)),
    };

    let invitation_id = InvitationId::new();
    let outcome = InvitationRepository::create(
        pool,
        CreateInvitationParams {
            id: invitation_id,
            tenant_id,
            session_id,
            code_sha256,
            grants: effective_grants,
            max_uses,
            expires_at,
            created_by: actor,
            created_by_member_id: creator_member_id,
            now,
        },
    )
    .await?;

    match outcome {
        CreateInvitationOutcome::Created(row) => {
            let (actor_kind, actor_id, sub_id) = match caller {
                CreateInvitationCaller::Application => (ActorKind::Application, None, None),
                CreateInvitationCaller::Device {
                    device_id,
                    subject_id,
                    ..
                } => (
                    ActorKind::Device,
                    Some(device_id.into_uuid()),
                    Some(subject_id),
                ),
            };
            let _ = record_audit(
                pool,
                tenant_id,
                actor_kind,
                actor_id,
                sub_id,
                "invitation.created",
                Some("invitation"),
                Some(invitation_id.into_uuid()),
                AuditOutcome::Success,
                None,
                serde_json::json!({
                    "session_id": session_id.to_string(),
                    "max_uses": max_uses,
                    "expires_at": format_rfc3339(expires_at),
                    "grant_publish": effective_grants.publish,
                    "grant_read": effective_grants.read,
                    "grant_invite": effective_grants.invite,
                    "grant_manage": effective_grants.manage,
                }),
            )
            .await;

            let resource = InvitationResource::from_row(&row, now);
            Ok(InvitationCreatedResource {
                invitation: resource,
                code: code_formatted,
            })
        }
        CreateInvitationOutcome::SessionNotFound => Err(InvitationUseCaseError::NotFound),
        CreateInvitationOutcome::SessionNotActive => Err(InvitationUseCaseError::InactiveSession),
    }
}

/// Lists invitations of a session (FR-INV-003, docs/07 §9.4).
pub async fn list_invitations(
    pool: &PgPool,
    tenant_id: TenantId,
    session_id: SessionId,
    limit: u32,
    cursor: Option<String>,
    caller: ListInvitationsCaller,
    now: OffsetDateTime,
) -> Result<InvitationListResource, InvitationUseCaseError> {
    // Session must exist in the caller's tenant; otherwise 404 (FR-API-006, NFR-SEC-004).
    if SessionRepository::get_by_id(pool, tenant_id, session_id)
        .await?
        .is_none()
    {
        return Err(InvitationUseCaseError::NotFound);
    }

    // Determine creator filter based on caller permissions (x-gs-capability: manage_or_invite_own)
    let creator_filter = match caller {
        ListInvitationsCaller::Application => None,
        ListInvitationsCaller::Device {
            member_id,
            capabilities,
        } => {
            if capabilities.manage {
                None
            } else if capabilities.invite {
                Some(member_id)
            } else {
                return Err(InvitationUseCaseError::PermissionDenied(
                    "caller requires manage or invite capability",
                ));
            }
        }
    };

    let page_cursor = match cursor {
        Some(raw) => Some(
            PageCursor::decode(&raw)
                .map_err(|e| InvitationUseCaseError::Validation(e.to_string()))?,
        ),
        None => None,
    };

    let fetch_limit = (limit as i64) + 1;
    let db_cursor = page_cursor.map(|c| (c.created_at, uuid::Uuid::from_u128(c.id)));

    let page = InvitationRepository::list(
        pool,
        tenant_id,
        session_id,
        fetch_limit,
        db_cursor,
        creator_filter,
    )
    .await?;

    let has_more = (page.invitations.len() as u32) > limit;
    let mut items = page.invitations;
    if has_more {
        items.pop();
    }

    let next_cursor = if has_more {
        items
            .last()
            .map(|inv| PageCursor::new(inv.created_at, inv.id.into_uuid().as_u128(), 0).encode())
    } else {
        None
    };

    let resources = items
        .iter()
        .map(|row| InvitationResource::from_row(row, now))
        .collect();

    Ok(InvitationListResource {
        data: resources,
        next_cursor,
    })
}

/// Revokes an invitation idempotently (FR-INV-003, docs/07 §9.4).
pub async fn revoke_invitation(
    pool: &PgPool,
    tenant_id: TenantId,
    session_id: SessionId,
    invitation_id: InvitationId,
    caller: RevokeInvitationCaller,
    now: OffsetDateTime,
) -> Result<(), InvitationUseCaseError> {
    // The invitation must exist in this tenant and session for every caller (FR-API-006).
    let inv =
        match InvitationRepository::find_by_id(pool, tenant_id, session_id, invitation_id).await? {
            Some(i) => i,
            None => return Err(InvitationUseCaseError::NotFound),
        };

    // Verify caller authorization (x-gs-capability: manage_or_issuer)
    match caller {
        RevokeInvitationCaller::Application => {}
        RevokeInvitationCaller::Device {
            device_id,
            member_id,
            capabilities,
        } => {
            if !capabilities.manage {
                // Must be the issuer
                let is_issuer = inv.created_by_member_id == Some(member_id)
                    || match inv.created_by {
                        Actor::Device(did) => did == device_id,
                        _ => false,
                    };

                if !is_issuer {
                    return Err(InvitationUseCaseError::PermissionDenied(
                        "caller is neither session manager nor invitation creator",
                    ));
                }
            }
        }
    }

    InvitationRepository::revoke(pool, tenant_id, session_id, invitation_id, now).await?;

    let (actor_kind, actor_id) = match caller {
        RevokeInvitationCaller::Application => (ActorKind::Application, None),
        RevokeInvitationCaller::Device { device_id, .. } => {
            (ActorKind::Device, Some(device_id.into_uuid()))
        }
    };
    let _ = record_audit(
        pool,
        tenant_id,
        actor_kind,
        actor_id,
        None,
        "invitation.revoked",
        Some("invitation"),
        Some(invitation_id.into_uuid()),
        AuditOutcome::Success,
        None,
        serde_json::json!({
            "session_id": session_id.to_string(),
        }),
    )
    .await;

    Ok(())
}

/// Parameters for redeeming an invitation.
pub struct RedeemInvitationParams<'a, E: EphemeralStore> {
    pub pool: &'a PgPool,
    pub ephemeral: &'a E,
    pub key_prefix: &'a str,
    pub tenant_id: TenantId,
    pub code: &'a str,
    pub device_id: DeviceId,
    pub subject_id: SubjectId,
    pub rate_limit_rpm: u32,
    pub now: OffsetDateTime,
}

/// Redeems an invitation code atomically with rate limiting and concurrency (FR-INV-002, docs/05 §6).
pub async fn redeem_invitation<E: EphemeralStore>(
    params: RedeemInvitationParams<'_, E>,
) -> Result<RedeemInvitationResponse, InvitationUseCaseError> {
    let RedeemInvitationParams {
        pool,
        ephemeral,
        key_prefix,
        tenant_id,
        code,
        device_id,
        subject_id,
        rate_limit_rpm,
        now,
    } = params;

    // 1. Device rate limit check (FR-INV-002, docs/07 §8: GS_INVITE_REDEEM_RPM)
    let now_unix = now.unix_timestamp();
    let window_bucket = now_unix / 60;
    let rl_key = format!("{key_prefix}:rl:invite_redeem:{device_id}:{window_bucket}");

    let count = ephemeral
        .increment_counter(&rl_key, StdDuration::from_secs(120))
        .await?;

    if count > (rate_limit_rpm as u64) {
        let retry_after = ((60 - (now_unix % 60)) as u64).max(1);
        return Err(InvitationUseCaseError::RateLimited { retry_after });
    }

    // 2. Normalize invitation code (docs/05 §6, docs/09 §5: 404 INVITATION_INVALID if invalid)
    let normalized = match normalize_invitation_code(code) {
        Ok(c) => c,
        Err(_) => return Err(InvitationUseCaseError::InvitationInvalid),
    };

    // 3. Compute SHA-256 hash
    let code_sha256 = hash_invitation_code_bytes(&normalized);

    // 4. Atomically redeem in database under row locks (FR-INV-002)
    let outcome =
        InvitationRepository::redeem(pool, tenant_id, &code_sha256, subject_id, now).await?;

    match outcome {
        RedeemOutcome::Success { session, member } => {
            // Increment mver in ephemeral store
            let mver_key = format!("{key_prefix}:mver:{}", session.id);
            let _ = ephemeral.increment_membership_version(&mver_key).await;

            let inv_id = match member.membership.joined_via {
                gs_domain::membership::JoinedVia::Invitation(id) => Some(id.into_uuid()),
                _ => None,
            };
            let _ = record_audit(
                pool,
                tenant_id,
                ActorKind::Device,
                Some(device_id.into_uuid()),
                Some(subject_id),
                "invitation.redeemed",
                Some("invitation"),
                inv_id,
                AuditOutcome::Success,
                None,
                serde_json::json!({
                    "session_id": session.id.to_string(),
                    "member_id": member.membership.id.to_string(),
                }),
            )
            .await;

            let session_res = SessionResource::from_session(&session);
            let member_res = MemberResource::from_member_with_subject(&member, false, None);

            Ok(RedeemInvitationResponse {
                session: session_res,
                member: member_res,
            })
        }
        RedeemOutcome::Invalid => Err(InvitationUseCaseError::InvitationInvalid),
        RedeemOutcome::AlreadyActiveMember => Err(InvitationUseCaseError::Conflict),
        RedeemOutcome::SessionFull => Err(InvitationUseCaseError::SessionFull),
    }
}
