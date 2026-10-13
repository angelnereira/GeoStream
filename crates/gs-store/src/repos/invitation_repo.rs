//! Invitation repository (docs/05 §6, docs/07 §9.4, FR-INV-001..003).
#![forbid(unsafe_code)]

use sqlx::Row;
use time::OffsetDateTime;
use uuid::Uuid;

use gs_domain::ids::{
    ApplicationId, DeviceId, InvitationId, MemberId, SessionId, SubjectId, TenantId,
};
use gs_domain::invitation::InvitationStatus;
use gs_domain::membership::{Capabilities, JoinedVia, MemberRole, MemberStatusKind, Membership};
use gs_domain::session::{Actor, Session, SessionStatus};

use crate::PgPool as Pool;
use crate::error::StoreError;
use crate::repos::membership_repo::MemberWithSubject;
use crate::repos::session_repo::SessionRepository;

/// Invitation row stored in PostgreSQL `invitations` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvitationRow {
    pub id: InvitationId,
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub code_sha256: Vec<u8>,
    pub grants: Capabilities,
    pub max_uses: u32,
    pub uses: u32,
    pub expires_at: OffsetDateTime,
    pub created_by: Actor,
    pub created_by_member_id: Option<MemberId>,
    pub created_at: OffsetDateTime,
    pub revoked_at: Option<OffsetDateTime>,
}

impl InvitationRow {
    /// Status calculated by precedence: REVOKED > EXPIRED > EXHAUSTED > ACTIVE (docs/07 §9.4).
    pub fn status_at(&self, now: OffsetDateTime) -> InvitationStatus {
        if self.revoked_at.is_some() {
            InvitationStatus::Revoked
        } else if self.expires_at <= now {
            InvitationStatus::Expired
        } else if self.uses >= self.max_uses {
            InvitationStatus::Exhausted
        } else {
            InvitationStatus::Active
        }
    }
}

/// Parameters for creating an invitation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateInvitationParams {
    pub id: InvitationId,
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub code_sha256: Vec<u8>,
    pub grants: Capabilities,
    pub max_uses: u32,
    pub expires_at: OffsetDateTime,
    pub created_by: Actor,
    pub created_by_member_id: Option<MemberId>,
    pub now: OffsetDateTime,
}

/// Outcome of creating an invitation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateInvitationOutcome {
    Created(InvitationRow),
    SessionNotFound,
    SessionNotActive,
}

/// A page of invitations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvitationPage {
    pub invitations: Vec<InvitationRow>,
    pub last_created_at: Option<OffsetDateTime>,
    pub last_id: Option<Uuid>,
}

/// Outcome of redeeming an invitation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedeemOutcome {
    Success {
        session: Box<Session>,
        member: Box<MemberWithSubject>,
    },
    Invalid,
    AlreadyActiveMember,
    SessionFull,
}

/// Helper converting stored actor strings to domain `Actor`.
fn actor_from_parts(kind: &str, id: Uuid) -> Result<Actor, StoreError> {
    match kind {
        "DEVICE" => Ok(Actor::Device(DeviceId::from_uuid(id))),
        "APPLICATION" => Ok(Actor::Application(ApplicationId::from_uuid(id))),
        other => Err(StoreError::DataMapping(format!(
            "unknown actor kind '{other}'"
        ))),
    }
}

fn actor_id(actor: &Actor) -> Uuid {
    match actor {
        Actor::Device(device_id) => device_id.into_uuid(),
        Actor::Application(application_id) => application_id.into_uuid(),
    }
}

fn actor_kind(actor: &Actor) -> &'static str {
    match actor {
        Actor::Device(_) => "DEVICE",
        Actor::Application(_) => "APPLICATION",
    }
}

/// Repository for invitation management and atomic redemption.
pub struct InvitationRepository;

impl InvitationRepository {
    /// Maps a raw database row into `InvitationRow`.
    fn map_row(row: sqlx::postgres::PgRow) -> Result<InvitationRow, StoreError> {
        use sqlx::Row;

        let id_raw: Uuid = row.try_get("id").map_err(StoreError::Database)?;
        let tenant_id_raw: Uuid = row.try_get("tenant_id").map_err(StoreError::Database)?;
        let session_id_raw: Uuid = row.try_get("session_id").map_err(StoreError::Database)?;
        let code_sha256: Vec<u8> = row.try_get("code_sha256").map_err(StoreError::Database)?;

        let grant_publish: bool = row.try_get("grant_publish").map_err(StoreError::Database)?;
        let grant_read: bool = row.try_get("grant_read").map_err(StoreError::Database)?;
        let grant_invite: bool = row.try_get("grant_invite").map_err(StoreError::Database)?;
        let grant_manage: bool = row.try_get("grant_manage").map_err(StoreError::Database)?;

        let max_uses_raw: i32 = row.try_get("max_uses").map_err(StoreError::Database)?;
        let uses_raw: i32 = row.try_get("uses").map_err(StoreError::Database)?;
        let expires_at: OffsetDateTime = row.try_get("expires_at").map_err(StoreError::Database)?;

        let created_by_kind: String = row
            .try_get("created_by_kind")
            .map_err(StoreError::Database)?;
        let created_by_id: Uuid = row.try_get("created_by_id").map_err(StoreError::Database)?;
        let created_by_member_id_raw: Option<Uuid> = row
            .try_get("created_by_member_id")
            .map_err(StoreError::Database)?;

        let created_at: OffsetDateTime = row.try_get("created_at").map_err(StoreError::Database)?;
        let revoked_at: Option<OffsetDateTime> =
            row.try_get("revoked_at").map_err(StoreError::Database)?;

        let created_by = actor_from_parts(&created_by_kind, created_by_id)?;
        let created_by_member_id = created_by_member_id_raw.map(MemberId::from_uuid);

        Ok(InvitationRow {
            id: InvitationId::from_uuid(id_raw),
            tenant_id: TenantId::from_uuid(tenant_id_raw),
            session_id: SessionId::from_uuid(session_id_raw),
            code_sha256,
            grants: Capabilities {
                publish: grant_publish,
                read: grant_read,
                invite: grant_invite,
                manage: grant_manage,
            },
            max_uses: max_uses_raw.max(0) as u32,
            uses: uses_raw.max(0) as u32,
            expires_at,
            created_by,
            created_by_member_id,
            created_at,
            revoked_at,
        })
    }

    /// Creates an invitation under tenant isolation and active session check (FR-INV-001).
    pub async fn create(
        pool: &Pool,
        params: CreateInvitationParams,
    ) -> Result<CreateInvitationOutcome, StoreError> {
        let mut tx = pool.begin().await.map_err(StoreError::Database)?;

        // Verify session is active and not expired (docs/05 §4.3 INV-SES-1, INV-SES-2)
        let session_opt = sqlx::query(
            r#"
            SELECT status, expires_at
            FROM sessions
            WHERE id = $1 AND tenant_id = $2
            FOR UPDATE;
            "#,
        )
        .bind(params.session_id.into_uuid())
        .bind(params.tenant_id.into_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        let session_row = match session_opt {
            Some(r) => r,
            None => return Ok(CreateInvitationOutcome::SessionNotFound),
        };

        let status_str: String = session_row.get("status");
        let expires_at: OffsetDateTime = session_row.get("expires_at");

        if status_str != "ACTIVE" || expires_at <= params.now {
            return Ok(CreateInvitationOutcome::SessionNotActive);
        }

        let created_by_kind_str = actor_kind(&params.created_by);
        let created_by_id_uuid = actor_id(&params.created_by);
        let created_by_member_uuid = params.created_by_member_id.map(|m| m.into_uuid());

        let row = sqlx::query(
            r#"
            INSERT INTO invitations (
                id, tenant_id, session_id, code_sha256,
                grant_publish, grant_read, grant_invite, grant_manage,
                max_uses, uses, expires_at,
                created_by_kind, created_by_id, created_by_member_id,
                created_at, revoked_at
            ) VALUES (
                $1, $2, $3, $4,
                $5, $6, $7, $8,
                $9, 0, $10,
                $11, $12, $13,
                $14, NULL
            )
            RETURNING id, tenant_id, session_id, code_sha256,
                      grant_publish, grant_read, grant_invite, grant_manage,
                      max_uses, uses, expires_at,
                      created_by_kind, created_by_id, created_by_member_id,
                      created_at, revoked_at;
            "#,
        )
        .bind(params.id.into_uuid())
        .bind(params.tenant_id.into_uuid())
        .bind(params.session_id.into_uuid())
        .bind(&params.code_sha256)
        .bind(params.grants.publish)
        .bind(params.grants.read)
        .bind(params.grants.invite)
        .bind(params.grants.manage)
        .bind(params.max_uses as i32)
        .bind(params.expires_at)
        .bind(created_by_kind_str)
        .bind(created_by_id_uuid)
        .bind(created_by_member_uuid)
        .bind(params.now)
        .fetch_one(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        tx.commit().await.map_err(StoreError::Database)?;

        let invitation = Self::map_row(row)?;
        Ok(CreateInvitationOutcome::Created(invitation))
    }

    /// Finds an invitation by ID within a tenant and session.
    pub async fn find_by_id(
        pool: &Pool,
        tenant_id: TenantId,
        session_id: SessionId,
        invitation_id: InvitationId,
    ) -> Result<Option<InvitationRow>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, tenant_id, session_id, code_sha256,
                   grant_publish, grant_read, grant_invite, grant_manage,
                   max_uses, uses, expires_at,
                   created_by_kind, created_by_id, created_by_member_id,
                   created_at, revoked_at
            FROM invitations
            WHERE id = $1 AND tenant_id = $2 AND session_id = $3;
            "#,
        )
        .bind(invitation_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        row.map(Self::map_row).transpose()
    }

    /// Lists invitations of a session, ordered by `created_at DESC, id DESC` (docs/07 §9.4).
    /// If `creator_filter` is specified, only invitations created by that actor or member are returned.
    pub async fn list(
        pool: &Pool,
        tenant_id: TenantId,
        session_id: SessionId,
        limit: i64,
        cursor: Option<(OffsetDateTime, Uuid)>,
        creator_member_filter: Option<MemberId>,
    ) -> Result<InvitationPage, StoreError> {
        let (cursor_time, cursor_id) = match cursor {
            Some((t, id)) => (Some(t), id),
            None => (None, Uuid::nil()),
        };

        let filter_member_uuid = creator_member_filter.map(|m| m.into_uuid());

        let rows = sqlx::query(
            r#"
            SELECT id, tenant_id, session_id, code_sha256,
                   grant_publish, grant_read, grant_invite, grant_manage,
                   max_uses, uses, expires_at,
                   created_by_kind, created_by_id, created_by_member_id,
                   created_at, revoked_at
            FROM invitations
            WHERE tenant_id = $1
              AND session_id = $2
              AND ($3::uuid IS NULL OR created_by_member_id = $3)
              AND (
                  $4::timestamptz IS NULL
                  OR (created_at, id) < ($4, $5)
              )
            ORDER BY created_at DESC, id DESC
            LIMIT $6;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .bind(filter_member_uuid)
        .bind(cursor_time)
        .bind(cursor_id)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(StoreError::Database)?;

        let mut invitations = Vec::with_capacity(rows.len());
        for row in rows {
            invitations.push(Self::map_row(row)?);
        }

        let last = invitations
            .last()
            .map(|inv| (inv.created_at, inv.id.into_uuid()));

        Ok(InvitationPage {
            invitations,
            last_created_at: last.map(|(created_at, _)| created_at),
            last_id: last.map(|(_, id)| id),
        })
    }

    /// Revokes an invitation idempotently (FR-INV-003, docs/07 §9.4).
    /// Returns true if revoked by this call, false if already revoked.
    pub async fn revoke(
        pool: &Pool,
        tenant_id: TenantId,
        session_id: SessionId,
        invitation_id: InvitationId,
        now: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE invitations
            SET revoked_at = $4
            WHERE id = $1 AND tenant_id = $2 AND session_id = $3 AND revoked_at IS NULL;
            "#,
        )
        .bind(invitation_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .bind(now)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(result.rows_affected() > 0)
    }

    /// Redeems an invitation atomically under row locks (FR-INV-002, docs/05 §6).
    pub async fn redeem(
        pool: &Pool,
        tenant_id: TenantId,
        code_sha256: &[u8],
        subject_id: SubjectId,
        now: OffsetDateTime,
    ) -> Result<RedeemOutcome, StoreError> {
        let mut tx = pool.begin().await.map_err(StoreError::Database)?;

        // 1. Lock and fetch invitation by hash within tenant (INV-TEN-1)
        let inv_row_opt = sqlx::query(
            r#"
            SELECT id, tenant_id, session_id, code_sha256,
                   grant_publish, grant_read, grant_invite, grant_manage,
                   max_uses, uses, expires_at,
                   created_by_kind, created_by_id, created_by_member_id,
                   created_at, revoked_at
            FROM invitations
            WHERE tenant_id = $1 AND code_sha256 = $2
            FOR UPDATE;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(code_sha256)
        .fetch_optional(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        let inv_row = match inv_row_opt {
            Some(r) => Self::map_row(r)?,
            None => return Ok(RedeemOutcome::Invalid),
        };

        // 2. Validate invitation status (revoked, expired, exhausted -> INVITATION_INVALID)
        if inv_row.revoked_at.is_some()
            || inv_row.expires_at <= now
            || inv_row.uses >= inv_row.max_uses
        {
            return Ok(RedeemOutcome::Invalid);
        }

        // 3. Lock and fetch session
        let session_opt = sqlx::query(
            r#"
            SELECT id, tenant_id, mode, status, owner_subject_id, created_by_kind, created_by_id,
                   expires_at, max_members, snapshot_ttl_seconds, stale_after_seconds, metadata,
                   closed_reason, closed_at, version, created_at, updated_at
            FROM sessions
            WHERE id = $1 AND tenant_id = $2
            FOR UPDATE;
            "#,
        )
        .bind(inv_row.session_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        let session_sql_row = match session_opt {
            Some(r) => r,
            None => return Ok(RedeemOutcome::Invalid),
        };

        let session = SessionRepository::map_row(session_sql_row)?;

        // Session must be ACTIVE and not expired (docs/05 §4.3)
        if session.status != SessionStatus::Active || session.expires_at <= now {
            return Ok(RedeemOutcome::Invalid);
        }

        // 4. Check existing memberships for this subject in this session
        // a) Subject already has an ACTIVE membership -> 409 CONFLICT (FR-INV-002, docs/05 §6)
        let active_mem_opt = sqlx::query(
            r#"
            SELECT id
            FROM memberships
            WHERE tenant_id = $1 AND session_id = $2 AND subject_id = $3 AND status = 'ACTIVE'
            FOR UPDATE;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(inv_row.session_id.into_uuid())
        .bind(subject_id.into_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        if active_mem_opt.is_some() {
            return Ok(RedeemOutcome::AlreadyActiveMember);
        }

        // b) Subject's most recent membership was REVOKED -> INVITATION_INVALID (FR-MEM-007, docs/05 §6)
        use sqlx::Row;
        let latest_status_opt = sqlx::query(
            r#"
            SELECT status
            FROM memberships
            WHERE tenant_id = $1 AND session_id = $2 AND subject_id = $3
            ORDER BY joined_at DESC, id DESC
            LIMIT 1;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(inv_row.session_id.into_uuid())
        .bind(subject_id.into_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        if let Some(latest_row) = latest_status_opt {
            let latest_status: String =
                latest_row.try_get("status").map_err(StoreError::Database)?;
            if latest_status == "REVOKED" {
                return Ok(RedeemOutcome::Invalid);
            }
        }

        // 5. Check capacity: active_count < session.max_members (INV-MEM-7)
        let count_row = sqlx::query(
            r#"
            SELECT count(*)::bigint AS active_count
            FROM memberships
            WHERE tenant_id = $1 AND session_id = $2 AND status = 'ACTIVE';
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(inv_row.session_id.into_uuid())
        .fetch_one(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        let active_count: i64 = count_row
            .try_get("active_count")
            .map_err(StoreError::Database)?;
        if (active_count as u32) >= session.max_members {
            return Ok(RedeemOutcome::SessionFull);
        }

        // 6. Increment uses on invitation
        sqlx::query(
            r#"
            UPDATE invitations
            SET uses = uses + 1
            WHERE id = $1;
            "#,
        )
        .bind(inv_row.id.into_uuid())
        .execute(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        // 7. Insert new active membership with granted capabilities
        let new_member_id = MemberId::new();
        sqlx::query(
            r#"
            INSERT INTO memberships (
                id, tenant_id, session_id, subject_id, role,
                can_publish, can_read, can_invite, can_manage,
                status, sharing, publisher_device_id, sharing_changed_at,
                joined_via, invitation_id, joined_at, ended_at, version
            ) VALUES (
                $1, $2, $3, $4, 'MEMBER',
                $5, $6, $7, $8,
                'ACTIVE', false, NULL, $9,
                'INVITATION', $10, $9, NULL, 1
            );
            "#,
        )
        .bind(new_member_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .bind(inv_row.session_id.into_uuid())
        .bind(subject_id.into_uuid())
        .bind(inv_row.grants.publish)
        .bind(inv_row.grants.read)
        .bind(inv_row.grants.invite)
        .bind(inv_row.grants.manage)
        .bind(now)
        .bind(inv_row.id.into_uuid())
        .execute(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        // Fetch subject to construct MemberWithSubject
        let subject_row = sqlx::query(
            r#"
            SELECT external_subject_id
            FROM subjects
            WHERE id = $1 AND tenant_id = $2;
            "#,
        )
        .bind(subject_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .fetch_one(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        let external_subject_id: String = subject_row
            .try_get("external_subject_id")
            .map_err(StoreError::Database)?;

        tx.commit().await.map_err(StoreError::Database)?;

        let membership = Membership {
            id: new_member_id,
            tenant_id,
            session_id: inv_row.session_id,
            subject_id,
            role: MemberRole::Member,
            capabilities: inv_row.grants,
            status: MemberStatusKind::Active,
            sharing: false,
            publisher_device_id: None,
            sharing_changed_at: None,
            joined_via: JoinedVia::Invitation(inv_row.id),
            joined_at: now,
            ended_at: None,
            version: 1,
        };

        Ok(RedeemOutcome::Success {
            session: Box::new(session),
            member: Box::new(MemberWithSubject {
                membership,
                external_subject_id,
            }),
        })
    }
}
