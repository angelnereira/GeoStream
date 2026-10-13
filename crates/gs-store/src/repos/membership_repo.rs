//! Membership repository (docs/05 §5, `migrations/0001_init.sql`).
//!
//! Only the operations the session use case needs are here: inserting the owner membership of
//! a new session and reading the active memberships of a subject. The rest of the membership
//! lifecycle (add, revoke, leave, transfer, readmit) belongs to T-F3-04.
#![forbid(unsafe_code)]

use sqlx::Row;
use uuid::Uuid;

use gs_domain::ids::{MemberId, SessionId, SubjectId, TenantId};
use gs_domain::membership::{MemberRole, MemberStatusKind, Membership};

use crate::PgPool as Pool;
use crate::error::StoreError;

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
