//! Membership repository (docs/05 §5, `migrations/0001_init.sql`).
//!
//! Enforces tenant scoping (INV-TEN-1), active membership uniqueness (INV-MEM-6),
//! member limit under lock (INV-MEM-7), and owner departure cascade (FR-SES-007).
#![forbid(unsafe_code)]

use sqlx::Row;
use time::OffsetDateTime;
use uuid::Uuid;

use gs_domain::ids::{DeviceId, MemberId, SessionId, SubjectId, TenantId};
use gs_domain::membership::{MemberRole, MemberStatusKind, Membership, SharingTransition};

use crate::PgPool as Pool;
use crate::error::StoreError;

/// Membership joined with its subject's external identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberWithSubject {
    pub membership: Membership,
    pub external_subject_id: String,
}

/// A page of active members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberPage {
    pub members: Vec<MemberWithSubject>,
    pub last_joined_at: Option<OffsetDateTime>,
    pub last_id: Option<Uuid>,
}

impl MemberPage {
    pub fn has_more(&self, limit: usize) -> bool {
        self.members.len() > limit
    }
}

/// Result of adding a new member directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddMemberOutcome {
    Success(MemberWithSubject),
    SessionNotFound,
    SessionNotActive,
    SessionFull,
    AlreadyActive,
    SessionAlreadyHasOwner,
}

/// Result of updating member capabilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCapabilitiesOutcome {
    Success(MemberWithSubject),
    NotFound,
    NotActive,
    PreconditionFailed,
}

/// Result of ownership transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferOwnershipOutcome {
    Success(MemberWithSubject),
    SessionNotFound,
    SessionNotActive,
    MemberNotFound,
    MemberNotActive,
    PreconditionFailed,
}

/// Result of member revocation.
/// Parameters for updating member capabilities.
#[derive(Debug, Clone)]
pub struct UpdateCapabilitiesParams {
    pub expected_version: Option<u64>,
    pub new_caps: gs_domain::membership::Capabilities,
    pub stop_sharing: bool,
    pub now: OffsetDateTime,
}

/// Result of member revocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevokeOutcome {
    NotFound,
    AlreadyEnded,
    Revoked {
        member: Box<MemberWithSubject>,
        promoted_owner: Option<Box<MemberWithSubject>>,
        session_closed: bool,
    },
}

/// Result of voluntary member departure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaveOutcome {
    NotFound,
    Left {
        member: Box<MemberWithSubject>,
        promoted_owner: Option<Box<MemberWithSubject>>,
        session_closed: bool,
    },
}

/// Parameters for setting member sharing state (FR-SHR-001..004, docs/05 §7).
#[derive(Debug, Clone)]
pub struct SetSharingParams {
    pub desired_sharing: bool,
    pub caller_subject_id: Option<SubjectId>,
    pub publisher_device_id: Option<DeviceId>,
    pub caller_is_app: bool,
    pub caller_has_manage: bool,
    pub now: OffsetDateTime,
}

/// Result of setting member sharing state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetSharingOutcome {
    Success {
        member: Box<MemberWithSubject>,
        transition: SharingTransition,
    },
    Unchanged(Box<MemberWithSubject>),
    SessionNotFound,
    SessionNotActive,
    MemberNotFound,
    MemberNotActive,
    OnlySelfCanActivateSharing,
    MissingPublishCapability,
    PermissionDenied,
}

/// Membership repository with tenant scoping (INV-TEN-1).
pub struct MembershipRepository;

impl MembershipRepository {
    /// Inserts a membership.
    pub async fn create(pool: &Pool, membership: &Membership) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO memberships (
                id, tenant_id, session_id, subject_id, role,
                can_publish, can_read, can_invite, can_manage,
                status, sharing, publisher_device_id, sharing_changed_at,
                joined_via, invitation_id, joined_at, version
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17);
            "#,
        )
        .bind(membership.id.into_uuid())
        .bind(membership.tenant_id.into_uuid())
        .bind(membership.session_id.into_uuid())
        .bind(membership.subject_id.into_uuid())
        .bind(membership.role.to_string())
        .bind(membership.capabilities.publish)
        .bind(membership.capabilities.read)
        .bind(membership.capabilities.invite)
        .bind(membership.capabilities.manage)
        .bind(membership.status.to_string())
        .bind(membership.sharing)
        .bind(membership.publisher_device_id.map(|id| id.into_uuid()))
        .bind(membership.sharing_changed_at)
        .bind(match &membership.joined_via {
            gs_domain::membership::JoinedVia::Creator => "CREATOR".to_string(),
            gs_domain::membership::JoinedVia::DirectAdd => "DIRECT_ADD".to_string(),
            gs_domain::membership::JoinedVia::Invitation(_) => "INVITATION".to_string(),
        })
        .bind(match &membership.joined_via {
            gs_domain::membership::JoinedVia::Invitation(invitation_id) => {
                Some(invitation_id.into_uuid())
            }
            _ => None,
        })
        .bind(membership.joined_at)
        .bind(i64::try_from(membership.version).unwrap_or(1))
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(())
    }

    /// Gets a membership by ID joined with its subject's external identifier.
    pub async fn get_by_id(
        pool: &Pool,
        tenant_id: TenantId,
        session_id: SessionId,
        member_id: MemberId,
    ) -> Result<Option<MemberWithSubject>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT m.id, m.tenant_id, m.session_id, m.subject_id, m.role,
                   m.can_publish, m.can_read, m.can_invite, m.can_manage,
                   m.status, m.sharing, m.publisher_device_id, m.sharing_changed_at,
                   m.joined_via, m.invitation_id, m.joined_at, m.ended_at, m.version,
                   s.external_subject_id
            FROM memberships m
            JOIN subjects s ON s.id = m.subject_id
            WHERE m.tenant_id = $1 AND m.session_id = $2 AND m.id = $3;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .bind(member_id.into_uuid())
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        row.as_ref().map(Self::map_row_with_subject).transpose()
    }

    /// Lists the active memberships of a subject (docs/07 §9.2).
    pub async fn list_active_by_subject(
        pool: &Pool,
        tenant_id: TenantId,
        subject_id: SubjectId,
    ) -> Result<Vec<Membership>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, tenant_id, session_id, subject_id, role,
                   can_publish, can_read, can_invite, can_manage,
                   status, sharing, publisher_device_id, sharing_changed_at,
                   joined_via, invitation_id, joined_at, ended_at, version
            FROM memberships
            WHERE tenant_id = $1 AND subject_id = $2 AND status = 'ACTIVE'
            ORDER BY joined_at ASC, id ASC;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(subject_id.into_uuid())
        .fetch_all(pool)
        .await
        .map_err(StoreError::Database)?;

        rows.iter().map(Self::map_row).collect()
    }

    /// Counts the active memberships of a session.
    pub async fn count_active_in_session(
        pool: &Pool,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<u64, StoreError> {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) FROM memberships
            WHERE tenant_id = $1 AND session_id = $2 AND status = 'ACTIVE';
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .fetch_one(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(u64::try_from(count).unwrap_or(0))
    }

    /// Finds an active membership of a subject in a session.
    pub async fn find_active_by_session_and_subject(
        pool: &Pool,
        tenant_id: TenantId,
        session_id: SessionId,
        subject_id: SubjectId,
    ) -> Result<Option<Membership>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, tenant_id, session_id, subject_id, role,
                   can_publish, can_read, can_invite, can_manage,
                   status, sharing, publisher_device_id, sharing_changed_at,
                   joined_via, invitation_id, joined_at, ended_at, version
            FROM memberships
            WHERE tenant_id = $1 AND session_id = $2 AND subject_id = $3 AND status = 'ACTIVE';
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .bind(subject_id.into_uuid())
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        row.as_ref().map(Self::map_row).transpose()
    }

    /// Finds an active membership of a subject in a session joined with external subject id.
    pub async fn find_active_with_subject(
        pool: &Pool,
        tenant_id: TenantId,
        session_id: SessionId,
        subject_id: SubjectId,
    ) -> Result<Option<MemberWithSubject>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT m.id, m.tenant_id, m.session_id, m.subject_id, m.role,
                   m.can_publish, m.can_read, m.can_invite, m.can_manage,
                   m.status, m.sharing, m.publisher_device_id, m.sharing_changed_at,
                   m.joined_via, m.invitation_id, m.joined_at, m.ended_at, m.version,
                   s.external_subject_id
            FROM memberships m
            JOIN subjects s ON s.id = m.subject_id
            WHERE m.tenant_id = $1 AND m.session_id = $2 AND m.subject_id = $3 AND m.status = 'ACTIVE';
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .bind(subject_id.into_uuid())
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        row.as_ref().map(Self::map_row_with_subject).transpose()
    }

    /// Finds the most recent membership of a subject in a session (for readmission check, FR-MEM-007).
    pub async fn find_latest_by_session_and_subject(
        pool: &Pool,
        tenant_id: TenantId,
        session_id: SessionId,
        subject_id: SubjectId,
    ) -> Result<Option<Membership>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, tenant_id, session_id, subject_id, role,
                   can_publish, can_read, can_invite, can_manage,
                   status, sharing, publisher_device_id, sharing_changed_at,
                   joined_via, invitation_id, joined_at, ended_at, version
            FROM memberships
            WHERE tenant_id = $1 AND session_id = $2 AND subject_id = $3
            ORDER BY joined_at DESC, id DESC
            LIMIT 1;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .bind(subject_id.into_uuid())
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        row.as_ref().map(Self::map_row).transpose()
    }

    /// Lists active members of a session, ordered by `joined_at ASC, id ASC` (docs/07 §9.3).
    pub async fn list_active_by_session(
        pool: &Pool,
        tenant_id: TenantId,
        session_id: SessionId,
        limit: i64,
        cursor: Option<(OffsetDateTime, Uuid)>,
    ) -> Result<MemberPage, StoreError> {
        let (cursor_time, cursor_id) = match cursor {
            Some((t, id)) => (Some(t), id),
            None => (None, Uuid::nil()),
        };

        let rows = sqlx::query(
            r#"
            SELECT m.id, m.tenant_id, m.session_id, m.subject_id, m.role,
                   m.can_publish, m.can_read, m.can_invite, m.can_manage,
                   m.status, m.sharing, m.publisher_device_id, m.sharing_changed_at,
                   m.joined_via, m.invitation_id, m.joined_at, m.ended_at, m.version,
                   s.external_subject_id
            FROM memberships m
            JOIN subjects s ON s.id = m.subject_id
            WHERE m.tenant_id = $1
              AND m.session_id = $2
              AND m.status = 'ACTIVE'
              AND (
                    $3::timestamptz IS NULL
                    OR (m.joined_at, m.id) > ($3::timestamptz, $4::uuid)
                  )
            ORDER BY m.joined_at ASC, m.id ASC
            LIMIT $5;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .bind(cursor_time)
        .bind(cursor_id)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(StoreError::Database)?;

        let mut members = Vec::with_capacity(rows.len());
        let mut last_joined_at = None;
        let mut last_id = None;

        for row in &rows {
            let item = Self::map_row_with_subject(row)?;
            last_joined_at = Some(item.membership.joined_at);
            last_id = Some(item.membership.id.into_uuid());
            members.push(item);
        }

        Ok(MemberPage {
            members,
            last_joined_at,
            last_id,
        })
    }

    /// Lists all active memberships where the given device is currently the publishing device.
    ///
    /// Used for presence offline grace and connectivity notifications (docs/04 §6.3, FR-PRS-002).
    pub async fn list_active_publishing_by_device(
        pool: &Pool,
        tenant_id: TenantId,
        device_id: gs_domain::ids::DeviceId,
    ) -> Result<Vec<MemberWithSubject>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT m.id, m.tenant_id, m.session_id, m.subject_id, m.role,
                   m.can_publish, m.can_read, m.can_invite, m.can_manage,
                   m.status, m.sharing, m.publisher_device_id, m.sharing_changed_at,
                   m.joined_via, m.invitation_id, m.joined_at, m.ended_at, m.version,
                   s.external_subject_id
            FROM memberships m
            JOIN subjects s ON s.id = m.subject_id AND s.tenant_id = m.tenant_id
            WHERE m.tenant_id = $1 AND m.publisher_device_id = $2 AND m.sharing = true AND m.status = 'ACTIVE'
            ORDER BY m.joined_at ASC, m.id ASC;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(device_id.into_uuid())
        .fetch_all(pool)
        .await
        .map_err(StoreError::Database)?;

        let mut members = Vec::with_capacity(rows.len());
        for row in &rows {
            members.push(Self::map_row_with_subject(row)?);
        }
        Ok(members)
    }

    /// Adds a member directly in a transaction with row-level session lock (INV-MEM-6, INV-MEM-7).
    pub async fn add_member(
        pool: &Pool,
        tenant_id: TenantId,
        session_id: SessionId,
        subject_id: SubjectId,
        role: MemberRole,
        capabilities: gs_domain::membership::Capabilities,
        now: OffsetDateTime,
    ) -> Result<AddMemberOutcome, StoreError> {
        let mut tx = pool.begin().await.map_err(StoreError::Database)?;

        // Lock session to prevent concurrency race on max_members (INV-MEM-7)
        let session_row = sqlx::query(
            r#"
            SELECT id, mode, status, expires_at, max_members, owner_subject_id
            FROM sessions
            WHERE tenant_id = $1 AND id = $2
            FOR UPDATE;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        let session_row = match session_row {
            Some(row) => row,
            None => return Ok(AddMemberOutcome::SessionNotFound),
        };

        let status: String = session_row.get("status");
        let expires_at: OffsetDateTime = session_row.get("expires_at");
        if status != "ACTIVE" || expires_at <= now {
            return Ok(AddMemberOutcome::SessionNotActive);
        }

        let max_members: i32 = session_row.get("max_members");

        // Count active members under session lock
        let active_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) FROM memberships
            WHERE tenant_id = $1 AND session_id = $2 AND status = 'ACTIVE';
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .fetch_one(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        if active_count >= max_members as i64 {
            return Ok(AddMemberOutcome::SessionFull);
        }

        // Check if subject already has active membership (INV-MEM-6)
        let existing_active: Option<Uuid> = sqlx::query_scalar(
            r#"
            SELECT id FROM memberships
            WHERE tenant_id = $1 AND session_id = $2 AND subject_id = $3 AND status = 'ACTIVE';
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .bind(subject_id.into_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        if existing_active.is_some() {
            return Ok(AddMemberOutcome::AlreadyActive);
        }

        // If role == Owner, verify if session already has an owner
        let owner_sub_id: Option<Uuid> = session_row.get("owner_subject_id");
        if role == MemberRole::Owner && owner_sub_id.is_some() {
            return Ok(AddMemberOutcome::SessionAlreadyHasOwner);
        }

        let member_id = MemberId::new();
        sqlx::query(
            r#"
            INSERT INTO memberships (
                id, tenant_id, session_id, subject_id, role,
                can_publish, can_read, can_invite, can_manage,
                status, sharing, publisher_device_id, sharing_changed_at,
                joined_via, invitation_id, joined_at, version
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'ACTIVE', false, NULL, NULL, 'DIRECT_ADD', NULL, $10, 1);
            "#,
        )
        .bind(member_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .bind(subject_id.into_uuid())
        .bind(role.to_string())
        .bind(capabilities.publish)
        .bind(capabilities.read)
        .bind(capabilities.invite)
        .bind(capabilities.manage)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        if role == MemberRole::Owner {
            sqlx::query(
                r#"
                UPDATE sessions
                SET owner_subject_id = $3,
                    version = version + 1,
                    updated_at = $4
                WHERE tenant_id = $1 AND id = $2;
                "#,
            )
            .bind(tenant_id.into_uuid())
            .bind(session_id.into_uuid())
            .bind(subject_id.into_uuid())
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(StoreError::Database)?;
        }

        tx.commit().await.map_err(StoreError::Database)?;

        let created = Self::get_by_id(pool, tenant_id, session_id, member_id)
            .await?
            .ok_or_else(|| StoreError::DataMapping("created member not found".to_string()))?;

        Ok(AddMemberOutcome::Success(created))
    }

    /// Updates member capabilities with optimistic concurrency check (INV-MEM-1..4).
    pub async fn update_capabilities(
        pool: &Pool,
        tenant_id: TenantId,
        session_id: SessionId,
        member_id: MemberId,
        params: UpdateCapabilitiesParams,
    ) -> Result<UpdateCapabilitiesOutcome, StoreError> {
        let mut tx = pool.begin().await.map_err(StoreError::Database)?;

        let row = sqlx::query(
            r#"
            SELECT id, role, status, version
            FROM memberships
            WHERE tenant_id = $1 AND session_id = $2 AND id = $3
            FOR UPDATE;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .bind(member_id.into_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        let row = match row {
            Some(row) => row,
            None => return Ok(UpdateCapabilitiesOutcome::NotFound),
        };

        let status: String = row.get("status");
        if status != "ACTIVE" {
            return Ok(UpdateCapabilitiesOutcome::NotActive);
        }

        let version: i64 = row.get("version");
        if let Some(expected) = params.expected_version
            && u64::try_from(version).unwrap_or(1) != expected
        {
            return Ok(UpdateCapabilitiesOutcome::PreconditionFailed);
        }

        sqlx::query(
            r#"
            UPDATE memberships
            SET can_publish = $4,
                can_read = $5,
                can_invite = $6,
                can_manage = $7,
                sharing = (CASE WHEN $8 THEN false ELSE sharing END),
                publisher_device_id = (CASE WHEN $8 THEN NULL ELSE publisher_device_id END),
                sharing_changed_at = (CASE WHEN $8 THEN $9 ELSE sharing_changed_at END),
                version = version + 1
            WHERE tenant_id = $1 AND session_id = $2 AND id = $3;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .bind(member_id.into_uuid())
        .bind(params.new_caps.publish)
        .bind(params.new_caps.read)
        .bind(params.new_caps.invite)
        .bind(params.new_caps.manage)
        .bind(params.stop_sharing)
        .bind(params.now)
        .execute(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        tx.commit().await.map_err(StoreError::Database)?;

        let updated = Self::get_by_id(pool, tenant_id, session_id, member_id)
            .await?
            .ok_or_else(|| StoreError::DataMapping("updated member not found".to_string()))?;

        Ok(UpdateCapabilitiesOutcome::Success(updated))
    }

    /// Transfers session ownership to target member in a single transaction (FR-MEM-005, docs/07 §9.3).
    pub async fn transfer_ownership(
        pool: &Pool,
        tenant_id: TenantId,
        session_id: SessionId,
        target_member_id: MemberId,
        expected_target_version: Option<u64>,
        now: OffsetDateTime,
    ) -> Result<TransferOwnershipOutcome, StoreError> {
        let mut tx = pool.begin().await.map_err(StoreError::Database)?;

        // Lock session
        let session_row = sqlx::query(
            r#"
            SELECT id, status, owner_subject_id
            FROM sessions
            WHERE tenant_id = $1 AND id = $2
            FOR UPDATE;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        let session_row = match session_row {
            Some(row) => row,
            None => return Ok(TransferOwnershipOutcome::SessionNotFound),
        };

        let session_status: String = session_row.get("status");
        if session_status != "ACTIVE" {
            return Ok(TransferOwnershipOutcome::SessionNotActive);
        }

        let current_owner_sub_id: Option<Uuid> = session_row.get("owner_subject_id");

        // Lock target member
        let target_row = sqlx::query(
            r#"
            SELECT id, subject_id, role, status, version
            FROM memberships
            WHERE tenant_id = $1 AND session_id = $2 AND id = $3
            FOR UPDATE;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .bind(target_member_id.into_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        let target_row = match target_row {
            Some(row) => row,
            None => return Ok(TransferOwnershipOutcome::MemberNotFound),
        };

        let target_status: String = target_row.get("status");
        if target_status != "ACTIVE" {
            return Ok(TransferOwnershipOutcome::MemberNotActive);
        }

        let target_version: i64 = target_row.get("version");
        if let Some(expected) = expected_target_version
            && u64::try_from(target_version).unwrap_or(1) != expected
        {
            return Ok(TransferOwnershipOutcome::PreconditionFailed);
        }

        let target_sub_id: Uuid = target_row.get("subject_id");
        let target_role: String = target_row.get("role");

        if target_role == "OWNER" {
            // Already owner - no-op
            tx.commit().await.map_err(StoreError::Database)?;
            let member = Self::get_by_id(pool, tenant_id, session_id, target_member_id)
                .await?
                .ok_or_else(|| StoreError::DataMapping("target member not found".to_string()))?;
            return Ok(TransferOwnershipOutcome::Success(member));
        }

        // Demote existing owner if one exists
        if let Some(prev_owner_sub) = current_owner_sub_id {
            sqlx::query(
                r#"
                UPDATE memberships
                SET role = 'MEMBER',
                    version = version + 1
                WHERE tenant_id = $1 AND session_id = $2 AND subject_id = $3 AND status = 'ACTIVE' AND role = 'OWNER';
                "#,
            )
            .bind(tenant_id.into_uuid())
            .bind(session_id.into_uuid())
            .bind(prev_owner_sub)
            .execute(&mut *tx)
            .await
            .map_err(StoreError::Database)?;
        }

        // Promote new owner with all 4 capabilities (INV-MEM-1)
        sqlx::query(
            r#"
            UPDATE memberships
            SET role = 'OWNER',
                can_publish = true,
                can_read = true,
                can_invite = true,
                can_manage = true,
                version = version + 1
            WHERE tenant_id = $1 AND session_id = $2 AND id = $3;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .bind(target_member_id.into_uuid())
        .execute(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        // Update sessions.owner_subject_id
        sqlx::query(
            r#"
            UPDATE sessions
            SET owner_subject_id = $3,
                version = version + 1,
                updated_at = $4
            WHERE tenant_id = $1 AND id = $2;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .bind(target_sub_id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        tx.commit().await.map_err(StoreError::Database)?;

        let updated_member = Self::get_by_id(pool, tenant_id, session_id, target_member_id)
            .await?
            .ok_or_else(|| StoreError::DataMapping("updated member not found".to_string()))?;

        Ok(TransferOwnershipOutcome::Success(updated_member))
    }

    /// Revokes a member from a session, handling owner departure if applicable (FR-MEM-006, FR-SES-007).
    pub async fn revoke_member(
        pool: &Pool,
        tenant_id: TenantId,
        session_id: SessionId,
        member_id: MemberId,
        now: OffsetDateTime,
    ) -> Result<RevokeOutcome, StoreError> {
        let mut tx = pool.begin().await.map_err(StoreError::Database)?;

        // Lock session
        let session_row = sqlx::query(
            r#"
            SELECT id, status, owner_subject_id, created_by_kind
            FROM sessions
            WHERE tenant_id = $1 AND id = $2
            FOR UPDATE;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        let session_row = match session_row {
            Some(row) => row,
            None => return Ok(RevokeOutcome::NotFound),
        };

        let created_by_kind: String = session_row.get("created_by_kind");

        // Lock target member
        let member_row = sqlx::query(
            r#"
            SELECT id, subject_id, role, status, sharing, version
            FROM memberships
            WHERE tenant_id = $1 AND session_id = $2 AND id = $3
            FOR UPDATE;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .bind(member_id.into_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        let member_row = match member_row {
            Some(row) => row,
            None => return Ok(RevokeOutcome::NotFound),
        };

        let current_status: String = member_row.get("status");
        if current_status != "ACTIVE" {
            // Idempotent: already REVOKED or LEFT (docs/07 §9.3)
            return Ok(RevokeOutcome::AlreadyEnded);
        }

        let is_owner = member_row.get::<String, _>("role") == "OWNER";
        let is_sharing: bool = member_row.get("sharing");

        // Update target member to REVOKED
        sqlx::query(
            r#"
            UPDATE memberships
            SET status = 'REVOKED',
                sharing = false,
                publisher_device_id = NULL,
                sharing_changed_at = (CASE WHEN $4 THEN $5 ELSE sharing_changed_at END),
                ended_at = $5,
                version = version + 1
            WHERE tenant_id = $1 AND session_id = $2 AND id = $3;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .bind(member_id.into_uuid())
        .bind(is_sharing)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        let mut promoted_owner = None;
        let mut session_closed = false;

        if is_owner {
            // Apply owner departure (FR-SES-007, docs/05 §5.4):
            // 1. Find oldest active member with can_manage = true
            let candidate_row = sqlx::query(
                r#"
                SELECT id, subject_id
                FROM memberships
                WHERE tenant_id = $1 AND session_id = $2 AND status = 'ACTIVE' AND can_manage = true AND id != $3
                ORDER BY joined_at ASC, id ASC
                LIMIT 1
                FOR UPDATE;
                "#,
            )
            .bind(tenant_id.into_uuid())
            .bind(session_id.into_uuid())
            .bind(member_id.into_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(StoreError::Database)?;

            if let Some(cand) = candidate_row {
                let candidate_id: Uuid = cand.get("id");
                let candidate_sub_id: Uuid = cand.get("subject_id");

                // Promote candidate to OWNER with all 4 capabilities
                sqlx::query(
                    r#"
                    UPDATE memberships
                    SET role = 'OWNER',
                        can_publish = true,
                        can_read = true,
                        can_invite = true,
                        can_manage = true,
                        version = version + 1
                    WHERE tenant_id = $1 AND session_id = $2 AND id = $3;
                    "#,
                )
                .bind(tenant_id.into_uuid())
                .bind(session_id.into_uuid())
                .bind(candidate_id)
                .execute(&mut *tx)
                .await
                .map_err(StoreError::Database)?;

                // Update sessions.owner_subject_id
                sqlx::query(
                    r#"
                    UPDATE sessions
                    SET owner_subject_id = $3,
                        version = version + 1,
                        updated_at = $4
                    WHERE tenant_id = $1 AND id = $2;
                    "#,
                )
                .bind(tenant_id.into_uuid())
                .bind(session_id.into_uuid())
                .bind(candidate_sub_id)
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(StoreError::Database)?;

                promoted_owner = Some(MemberId::from_uuid(candidate_id));
            } else if created_by_kind == "DEVICE" {
                // If created_by is a device: session closes with OWNER_LEFT (FR-SES-007, docs/05 §5.4)
                sqlx::query(
                    r#"
                    UPDATE sessions
                    SET status = 'CLOSED',
                        closed_reason = 'OWNER_LEFT',
                        closed_at = $3,
                        updated_at = $3,
                        version = version + 1
                    WHERE tenant_id = $1 AND id = $2;
                    "#,
                )
                .bind(tenant_id.into_uuid())
                .bind(session_id.into_uuid())
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(StoreError::Database)?;

                // Stop sharing on all memberships in closed session
                sqlx::query(
                    r#"
                    UPDATE memberships
                    SET sharing = false,
                        publisher_device_id = NULL
                    WHERE tenant_id = $1 AND session_id = $2 AND sharing = true;
                    "#,
                )
                .bind(tenant_id.into_uuid())
                .bind(session_id.into_uuid())
                .execute(&mut *tx)
                .await
                .map_err(StoreError::Database)?;

                session_closed = true;
            } else {
                // If created_by is an application: session stays open with owner_subject_id = NULL
                sqlx::query(
                    r#"
                    UPDATE sessions
                    SET owner_subject_id = NULL,
                        version = version + 1,
                        updated_at = $3
                    WHERE tenant_id = $1 AND id = $2;
                    "#,
                )
                .bind(tenant_id.into_uuid())
                .bind(session_id.into_uuid())
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(StoreError::Database)?;
            }
        }

        tx.commit().await.map_err(StoreError::Database)?;

        let revoked_member = Self::get_by_id(pool, tenant_id, session_id, member_id)
            .await?
            .ok_or_else(|| {
                StoreError::DataMapping("revoked member not found after commit".to_string())
            })?;

        let promoted_member = match promoted_owner {
            Some(pid) => Self::get_by_id(pool, tenant_id, session_id, pid).await?,
            None => None,
        };

        Ok(RevokeOutcome::Revoked {
            member: Box::new(revoked_member),
            promoted_owner: promoted_member.map(Box::new),
            session_closed,
        })
    }

    /// Voluntarily leaves a session, handling owner departure if applicable (FR-SES-007, docs/07 §9.3).
    pub async fn leave_session(
        pool: &Pool,
        tenant_id: TenantId,
        session_id: SessionId,
        subject_id: SubjectId,
        now: OffsetDateTime,
    ) -> Result<LeaveOutcome, StoreError> {
        let mut tx = pool.begin().await.map_err(StoreError::Database)?;

        // Lock session
        let session_row = sqlx::query(
            r#"
            SELECT id, status, owner_subject_id, created_by_kind
            FROM sessions
            WHERE tenant_id = $1 AND id = $2
            FOR UPDATE;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        let session_row = match session_row {
            Some(row) => row,
            None => return Ok(LeaveOutcome::NotFound),
        };

        let created_by_kind: String = session_row.get("created_by_kind");

        // Lock active member of caller subject
        let member_row = sqlx::query(
            r#"
            SELECT id, subject_id, role, status, sharing, version
            FROM memberships
            WHERE tenant_id = $1 AND session_id = $2 AND subject_id = $3 AND status = 'ACTIVE'
            FOR UPDATE;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .bind(subject_id.into_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        let member_row = match member_row {
            Some(row) => row,
            None => return Ok(LeaveOutcome::NotFound),
        };

        let member_id = MemberId::from_uuid(member_row.get("id"));
        let is_owner = member_row.get::<String, _>("role") == "OWNER";
        let is_sharing: bool = member_row.get("sharing");

        // Update member to LEFT
        sqlx::query(
            r#"
            UPDATE memberships
            SET status = 'LEFT',
                sharing = false,
                publisher_device_id = NULL,
                sharing_changed_at = (CASE WHEN $4 THEN $5 ELSE sharing_changed_at END),
                ended_at = $5,
                version = version + 1
            WHERE tenant_id = $1 AND session_id = $2 AND id = $3;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .bind(member_id.into_uuid())
        .bind(is_sharing)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        let mut promoted_owner = None;
        let mut session_closed = false;

        if is_owner {
            // Apply owner departure (FR-SES-007, docs/05 §5.4):
            // 1. Find oldest active member with can_manage = true
            let candidate_row = sqlx::query(
                r#"
                SELECT id, subject_id
                FROM memberships
                WHERE tenant_id = $1 AND session_id = $2 AND status = 'ACTIVE' AND can_manage = true AND id != $3
                ORDER BY joined_at ASC, id ASC
                LIMIT 1
                FOR UPDATE;
                "#,
            )
            .bind(tenant_id.into_uuid())
            .bind(session_id.into_uuid())
            .bind(member_id.into_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(StoreError::Database)?;

            if let Some(cand) = candidate_row {
                let candidate_id: Uuid = cand.get("id");
                let candidate_sub_id: Uuid = cand.get("subject_id");

                // Promote candidate to OWNER with all 4 capabilities
                sqlx::query(
                    r#"
                    UPDATE memberships
                    SET role = 'OWNER',
                        can_publish = true,
                        can_read = true,
                        can_invite = true,
                        can_manage = true,
                        version = version + 1
                    WHERE tenant_id = $1 AND session_id = $2 AND id = $3;
                    "#,
                )
                .bind(tenant_id.into_uuid())
                .bind(session_id.into_uuid())
                .bind(candidate_id)
                .execute(&mut *tx)
                .await
                .map_err(StoreError::Database)?;

                // Update sessions.owner_subject_id
                sqlx::query(
                    r#"
                    UPDATE sessions
                    SET owner_subject_id = $3,
                        version = version + 1,
                        updated_at = $4
                    WHERE tenant_id = $1 AND id = $2;
                    "#,
                )
                .bind(tenant_id.into_uuid())
                .bind(session_id.into_uuid())
                .bind(candidate_sub_id)
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(StoreError::Database)?;

                promoted_owner = Some(MemberId::from_uuid(candidate_id));
            } else if created_by_kind == "DEVICE" {
                // If created_by is a device: session closes with OWNER_LEFT (FR-SES-007, docs/05 §5.4)
                sqlx::query(
                    r#"
                    UPDATE sessions
                    SET status = 'CLOSED',
                        closed_reason = 'OWNER_LEFT',
                        closed_at = $3,
                        updated_at = $3,
                        version = version + 1
                    WHERE tenant_id = $1 AND id = $2;
                    "#,
                )
                .bind(tenant_id.into_uuid())
                .bind(session_id.into_uuid())
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(StoreError::Database)?;

                // Stop sharing on all memberships in closed session
                sqlx::query(
                    r#"
                    UPDATE memberships
                    SET sharing = false,
                        publisher_device_id = NULL
                    WHERE tenant_id = $1 AND session_id = $2 AND sharing = true;
                    "#,
                )
                .bind(tenant_id.into_uuid())
                .bind(session_id.into_uuid())
                .execute(&mut *tx)
                .await
                .map_err(StoreError::Database)?;

                session_closed = true;
            } else {
                // If created_by is an application: session stays open with owner_subject_id = NULL
                sqlx::query(
                    r#"
                    UPDATE sessions
                    SET owner_subject_id = NULL,
                        version = version + 1,
                        updated_at = $3
                    WHERE tenant_id = $1 AND id = $2;
                    "#,
                )
                .bind(tenant_id.into_uuid())
                .bind(session_id.into_uuid())
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(StoreError::Database)?;
            }
        }

        tx.commit().await.map_err(StoreError::Database)?;

        let left_member = Self::get_by_id(pool, tenant_id, session_id, member_id)
            .await?
            .ok_or_else(|| {
                StoreError::DataMapping("left member not found after commit".to_string())
            })?;

        let promoted_member = match promoted_owner {
            Some(pid) => Self::get_by_id(pool, tenant_id, session_id, pid).await?,
            None => None,
        };

        Ok(LeaveOutcome::Left {
            member: Box::new(left_member),
            promoted_owner: promoted_member.map(Box::new),
            session_closed,
        })
    }

    /// Sets member sharing state with strict invariant enforcement (FR-SHR-001..004, docs/05 §7).
    pub async fn set_sharing(
        pool: &Pool,
        tenant_id: TenantId,
        session_id: SessionId,
        member_id: MemberId,
        params: SetSharingParams,
    ) -> Result<SetSharingOutcome, StoreError> {
        let mut tx = pool.begin().await.map_err(StoreError::Database)?;

        // 1. Check session under lock
        let session_row = sqlx::query(
            r#"
            SELECT status, expires_at
            FROM sessions
            WHERE tenant_id = $1 AND id = $2
            FOR UPDATE;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        let session_row = match session_row {
            Some(r) => r,
            None => return Ok(SetSharingOutcome::SessionNotFound),
        };

        let session_status: String = session_row.get("status");
        let expires_at: OffsetDateTime = session_row.get("expires_at");
        if session_status != "ACTIVE" || expires_at <= params.now {
            return Ok(SetSharingOutcome::SessionNotActive);
        }

        // 2. Fetch target member under lock
        let member_row = sqlx::query(
            r#"
            SELECT m.id, m.tenant_id, m.session_id, m.subject_id, m.role,
                   m.can_publish, m.can_read, m.can_invite, m.can_manage,
                   m.status, m.sharing, m.publisher_device_id, m.sharing_changed_at,
                   m.joined_via, m.invitation_id, m.joined_at, m.ended_at, m.version,
                   s.external_subject_id
            FROM memberships m
            JOIN subjects s ON s.id = m.subject_id
            WHERE m.tenant_id = $1 AND m.session_id = $2 AND m.id = $3
            FOR UPDATE OF m;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(session_id.into_uuid())
        .bind(member_id.into_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(StoreError::Database)?;

        let member_row = match member_row {
            Some(r) => r,
            None => return Ok(SetSharingOutcome::MemberNotFound),
        };

        let mut current_member = Self::map_row_with_subject(&member_row)?;
        if current_member.membership.status != MemberStatusKind::Active {
            return Ok(SetSharingOutcome::MemberNotActive);
        }

        // 3. Evaluate sharing transition and permissions
        if params.desired_sharing {
            // INV-SHR-2: only the subject itself can activate sharing
            if params.caller_is_app
                || params.caller_subject_id != Some(current_member.membership.subject_id)
            {
                return Ok(SetSharingOutcome::OnlySelfCanActivateSharing);
            }

            // Must have publish
            if !current_member.membership.capabilities.publish {
                return Ok(SetSharingOutcome::MissingPublishCapability);
            }

            let new_device = match params.publisher_device_id {
                Some(d) => d,
                None => return Ok(SetSharingOutcome::OnlySelfCanActivateSharing),
            };

            // INV-SHR-3: idempotent if already sharing from the same device
            if current_member.membership.sharing
                && current_member.membership.publisher_device_id == Some(new_device)
            {
                return Ok(SetSharingOutcome::Unchanged(Box::new(current_member)));
            }

            let transition = if current_member.membership.sharing {
                let prev = current_member
                    .membership
                    .publisher_device_id
                    .unwrap_or(new_device);
                SharingTransition::Transferred {
                    previous_device: prev,
                    new_device,
                }
            } else {
                SharingTransition::Started(new_device)
            };

            let updated_row = sqlx::query(
                r#"
                UPDATE memberships
                SET sharing = true,
                    publisher_device_id = $4,
                    sharing_changed_at = $5,
                    version = version + 1
                WHERE tenant_id = $1 AND session_id = $2 AND id = $3
                RETURNING version;
                "#,
            )
            .bind(tenant_id.into_uuid())
            .bind(session_id.into_uuid())
            .bind(member_id.into_uuid())
            .bind(new_device.into_uuid())
            .bind(params.now)
            .fetch_one(&mut *tx)
            .await
            .map_err(StoreError::Database)?;

            let new_version: i64 = updated_row.get("version");
            current_member.membership.sharing = true;
            current_member.membership.publisher_device_id = Some(new_device);
            current_member.membership.sharing_changed_at = Some(params.now);
            current_member.membership.version = u64::try_from(new_version).unwrap_or(1);

            tx.commit().await.map_err(StoreError::Database)?;

            Ok(SetSharingOutcome::Success {
                member: Box::new(current_member),
                transition,
            })
        } else {
            // Stopping sharing (FR-SHR-003)
            let is_self = params.caller_subject_id == Some(current_member.membership.subject_id);
            let can_stop = params.caller_is_app || is_self || params.caller_has_manage;
            if !can_stop {
                return Ok(SetSharingOutcome::PermissionDenied);
            }

            if !current_member.membership.sharing {
                return Ok(SetSharingOutcome::Unchanged(Box::new(current_member)));
            }

            let updated_row = sqlx::query(
                r#"
                UPDATE memberships
                SET sharing = false,
                    publisher_device_id = NULL,
                    sharing_changed_at = $4,
                    version = version + 1
                WHERE tenant_id = $1 AND session_id = $2 AND id = $3
                RETURNING version;
                "#,
            )
            .bind(tenant_id.into_uuid())
            .bind(session_id.into_uuid())
            .bind(member_id.into_uuid())
            .bind(params.now)
            .fetch_one(&mut *tx)
            .await
            .map_err(StoreError::Database)?;

            let new_version: i64 = updated_row.get("version");
            current_member.membership.sharing = false;
            current_member.membership.publisher_device_id = None;
            current_member.membership.sharing_changed_at = Some(params.now);
            current_member.membership.version = u64::try_from(new_version).unwrap_or(1);

            tx.commit().await.map_err(StoreError::Database)?;

            Ok(SetSharingOutcome::Success {
                member: Box::new(current_member),
                transition: SharingTransition::Stopped,
            })
        }
    }

    fn map_row_with_subject(row: &sqlx::postgres::PgRow) -> Result<MemberWithSubject, StoreError> {
        let membership = Self::map_row(row)?;
        let external_subject_id: String = row.get("external_subject_id");
        Ok(MemberWithSubject {
            membership,
            external_subject_id,
        })
    }

    fn map_row(row: &sqlx::postgres::PgRow) -> Result<Membership, StoreError> {
        let role: String = row.get("role");
        let role = match role.as_str() {
            "OWNER" => MemberRole::Owner,
            "MEMBER" => MemberRole::Member,
            other => {
                return Err(StoreError::DataMapping(format!(
                    "invalid member role: {other}"
                )));
            }
        };
        let status: String = row.get("status");
        let status = match status.as_str() {
            "ACTIVE" => MemberStatusKind::Active,
            "LEFT" => MemberStatusKind::Left,
            "REVOKED" => MemberStatusKind::Revoked,
            other => {
                return Err(StoreError::DataMapping(format!(
                    "invalid member status: {other}"
                )));
            }
        };

        let joined_via: String = row.get("joined_via");
        let invitation_id: Option<Uuid> = row.get("invitation_id");
        let joined_via = match joined_via.as_str() {
            "CREATOR" => gs_domain::membership::JoinedVia::Creator,
            "DIRECT_ADD" => gs_domain::membership::JoinedVia::DirectAdd,
            "INVITATION" => match invitation_id {
                Some(id) => gs_domain::membership::JoinedVia::Invitation(
                    gs_domain::ids::InvitationId::from_uuid(id),
                ),
                None => {
                    return Err(StoreError::DataMapping(
                        "INVITATION membership without invitation_id".to_string(),
                    ));
                }
            },
            other => {
                return Err(StoreError::DataMapping(format!(
                    "invalid joined_via: {other}"
                )));
            }
        };

        Ok(Membership {
            id: MemberId::from_uuid(row.get("id")),
            tenant_id: TenantId::from_uuid(row.get("tenant_id")),
            session_id: SessionId::from_uuid(row.get("session_id")),
            subject_id: SubjectId::from_uuid(row.get("subject_id")),
            role,
            capabilities: gs_domain::membership::Capabilities {
                publish: row.get("can_publish"),
                read: row.get("can_read"),
                invite: row.get("can_invite"),
                manage: row.get("can_manage"),
            },
            status,
            sharing: row.get("sharing"),
            publisher_device_id: row
                .get::<Option<Uuid>, _>("publisher_device_id")
                .map(gs_domain::ids::DeviceId::from_uuid),
            sharing_changed_at: row.get("sharing_changed_at"),
            joined_at: row.get("joined_at"),
            ended_at: row.get("ended_at"),
            joined_via,
            version: u64::try_from(row.get::<i64, _>("version")).unwrap_or(1),
        })
    }
}
