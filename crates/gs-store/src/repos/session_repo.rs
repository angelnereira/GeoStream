//! Session repository (docs/05 §4, `migrations/0001_init.sql`).
//!
//! Every query filters by `tenant_id` (INV-TEN-1). Updates are optimistic: the `version` of
//! `docs/05` §4.1 is the `If-Match` precondition of `docs/07` §9.2, so a stale update fails
//! with `PRECONDITION_FAILED` instead of overwriting a newer state.
#![forbid(unsafe_code)]

use sqlx::Row;
use time::OffsetDateTime;
use uuid::Uuid;

use gs_domain::ids::{ApplicationId, DeviceId, SessionId, SubjectId, TenantId};
use gs_domain::session::{Actor, ClosedReason, Session, SessionMode, SessionStatus};

use crate::PgPool as Pool;
use crate::error::StoreError;

/// Builds an `Actor` from the stored kind and id.
fn map_actor(kind: &str, id: Uuid) -> Result<Actor, StoreError> {
    match kind {
        "DEVICE" => Ok(Actor::Device(DeviceId::from_uuid(id))),
        "APPLICATION" => Ok(Actor::Application(ApplicationId::from_uuid(id))),
        other => Err(StoreError::DataMapping(format!(
            "unknown actor kind '{other}'"
        ))),
    }
}

/// Identifier of an actor as stored in the database.
fn actor_id(actor: &Actor) -> Uuid {
    match actor {
        Actor::Device(device_id) => device_id.into_uuid(),
        Actor::Application(application_id) => application_id.into_uuid(),
    }
}

/// Kind of an actor as stored in the database.
fn actor_kind(actor: &Actor) -> &'static str {
    match actor {
        Actor::Device(_) => "DEVICE",
        Actor::Application(_) => "APPLICATION",
    }
}

/// Session repository with tenant scoping and optimistic concurrency.
pub struct SessionRepository;

/// Filters of a session listing (docs/07 §9.2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionFilters {
    /// Only sessions in this status (`ACTIVE`, `CLOSED`, `EXPIRED`).
    pub status: Option<SessionStatus>,
    /// Only sessions where this external subject is a member.
    pub external_subject_id: Option<String>,
}

/// A page of sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPage {
    /// Sessions of the page.
    pub sessions: Vec<Session>,
    /// `created_at` of the last session of the page.
    pub last_created_at: Option<OffsetDateTime>,
    /// `id` of the last session of the page.
    pub last_id: Option<Uuid>,
}

impl SessionPage {
    /// Whether there may be more pages.
    pub fn has_more(&self, limit: usize) -> bool {
        self.sessions.len() > limit
    }
}

/// Values of a session update (docs/05 §4.1, docs/07 §9.2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionUpdate {
    /// New expiry instant.
    pub expires_at: Option<OffsetDateTime>,
    /// New member limit.
    pub max_members: Option<u32>,
    /// New snapshot TTL.
    pub snapshot_ttl_seconds: Option<u32>,
    /// New staleness hint.
    pub stale_after_seconds: Option<u32>,
    /// New metadata, as the JSON object the column stores.
    pub metadata: Option<serde_json::Value>,
}

impl SessionRepository {
    /// Inserts a session.
    pub async fn create(pool: &Pool, session: &Session) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO sessions (
                id, tenant_id, mode, status, owner_subject_id, created_by_kind, created_by_id,
                expires_at, max_members, snapshot_ttl_seconds, stale_after_seconds, metadata,
                closed_reason, version, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16);
            "#,
        )
        .bind(session.id.into_uuid())
        .bind(session.tenant_id.into_uuid())
        .bind(session.mode.to_string())
        .bind(session.status.to_string())
        .bind(session.owner_subject_id.map(|id| id.into_uuid()))
        .bind(actor_kind(&session.created_by))
        .bind(actor_id(&session.created_by))
        .bind(session.expires_at)
        .bind(i64::from(session.max_members))
        .bind(i64::from(session.snapshot_ttl_seconds))
        .bind(i64::from(session.stale_after_seconds))
        .bind(
            serde_json::to_value(&session.metadata)
                .map_err(|e| StoreError::DataMapping(e.to_string()))?,
        )
        .bind(session.closed_reason.map(|reason| reason.to_string()))
        .bind(i64::try_from(session.version).unwrap_or(1))
        .bind(session.created_at)
        .bind(session.created_at)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(())
    }

    /// Fetches a session, scoped to its tenant.
    pub async fn get_by_id(
        pool: &Pool,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<Option<Session>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, tenant_id, mode, status, owner_subject_id, created_by_kind, created_by_id,
                   expires_at, max_members, snapshot_ttl_seconds, stale_after_seconds, metadata,
                   closed_reason, closed_at, version, created_at, updated_at
            FROM sessions
            WHERE id = $1 AND tenant_id = $2;
            "#,
        )
        .bind(session_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        row.map(Self::map_row).transpose()
    }

    /// Lists the sessions of a tenant with the documented filters and stable order.
    pub async fn list_by_tenant(
        pool: &Pool,
        tenant_id: TenantId,
        filters: &SessionFilters,
        limit: i64,
        cursor: Option<(OffsetDateTime, Uuid)>,
    ) -> Result<SessionPage, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, tenant_id, mode, status, owner_subject_id, created_by_kind, created_by_id,
                   expires_at, max_members, snapshot_ttl_seconds, stale_after_seconds, metadata,
                   closed_reason, closed_at, version, created_at, updated_at
            FROM sessions
            WHERE tenant_id = $1
              AND ($2::text IS NULL OR status = $2)
              AND (
                    $3::text IS NULL OR EXISTS (
                        SELECT 1 FROM memberships m
                        JOIN subjects s ON s.id = m.subject_id
                        WHERE m.session_id = sessions.id
                          AND m.status = 'ACTIVE'
                          AND s.external_subject_id = $3
                    )
                  )
              AND (
                    $6::timestamptz IS NULL
                    OR (created_at, id) < ($6::timestamptz, $7::uuid)
                  )
            ORDER BY created_at DESC, id DESC
            LIMIT $4;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(filters.status.map(|status| status.to_string()))
        .bind(filters.external_subject_id.clone())
        .bind(limit)
        .bind(Uuid::nil())
        .bind(cursor.map(|(created_at, _)| created_at))
        .bind(cursor.map(|(_, id)| id).unwrap_or_else(Uuid::nil))
        .fetch_all(pool)
        .await
        .map_err(StoreError::Database)?;

        let mut sessions = Vec::with_capacity(rows.len());
        for row in rows {
            sessions.push(Self::map_row(row)?);
        }
        let last = sessions
            .last()
            .map(|session| (session.created_at, session.id.into_uuid()));

        Ok(SessionPage {
            sessions,
            last_created_at: last.map(|(created_at, _)| created_at),
            last_id: last.map(|(_, id)| id),
        })
    }

    /// Lists the sessions where a subject is an active member (docs/07 §9.2).
    pub async fn list_by_subject(
        pool: &Pool,
        tenant_id: TenantId,
        subject_id: SubjectId,
        limit: i64,
        cursor: Option<(OffsetDateTime, Uuid)>,
    ) -> Result<SessionPage, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT s.id, s.tenant_id, s.mode, s.status, s.owner_subject_id, s.created_by_kind,
                   s.created_by_id, s.expires_at, s.max_members, s.snapshot_ttl_seconds,
                   s.stale_after_seconds, s.metadata, s.closed_reason, s.closed_at, s.version,
                   s.created_at, s.updated_at
            FROM sessions s
            JOIN memberships m ON m.session_id = s.id
            WHERE s.tenant_id = $1
              AND m.subject_id = $2
              AND m.status = 'ACTIVE'
              AND (
                    $5::timestamptz IS NULL
                    OR (s.created_at, s.id) < ($5::timestamptz, $6::uuid)
                  )
            ORDER BY s.created_at DESC, s.id DESC
            LIMIT $3;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(subject_id.into_uuid())
        .bind(limit)
        .bind(Uuid::nil())
        .bind(cursor.map(|(created_at, _)| created_at))
        .bind(cursor.map(|(_, id)| id).unwrap_or_else(Uuid::nil))
        .fetch_all(pool)
        .await
        .map_err(StoreError::Database)?;

        let mut sessions = Vec::with_capacity(rows.len());
        for row in rows {
            sessions.push(Self::map_row(row)?);
        }
        let last = sessions
            .last()
            .map(|session| (session.created_at, session.id.into_uuid()));

        Ok(SessionPage {
            sessions,
            last_created_at: last.map(|(created_at, _)| created_at),
            last_id: last.map(|(_, id)| id),
        })
    }

    /// Updates a session when its version matches (docs/07 §9.2 `If-Match`).
    ///
    /// Returns the updated session, or `None` when the version moved: the caller answers 412.
    pub async fn update_if_version_matches(
        pool: &Pool,
        tenant_id: TenantId,
        session_id: SessionId,
        expected_version: u64,
        update: &SessionUpdate,
        now: OffsetDateTime,
    ) -> Result<Option<Session>, StoreError> {
        let row = sqlx::query(
            r#"
            UPDATE sessions
            SET expires_at           = COALESCE($4, expires_at),
                max_members          = COALESCE($5, max_members),
                snapshot_ttl_seconds = COALESCE($6, snapshot_ttl_seconds),
                stale_after_seconds  = COALESCE($7, stale_after_seconds),
                metadata             = COALESCE($8, metadata),
                version              = version + 1,
                updated_at           = $9
            WHERE id = $1 AND tenant_id = $2 AND version = $3
            RETURNING id, tenant_id, mode, status, owner_subject_id, created_by_kind, created_by_id,
                      expires_at, max_members, snapshot_ttl_seconds, stale_after_seconds, metadata,
                      closed_reason, closed_at, version, created_at, updated_at;
            "#,
        )
        .bind(session_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .bind(i64::try_from(expected_version).unwrap_or(0))
        .bind(update.expires_at)
        .bind(update.max_members.map(i64::from))
        .bind(update.snapshot_ttl_seconds.map(i64::from))
        .bind(update.stale_after_seconds.map(i64::from))
        .bind(update.metadata.clone())
        .bind(now)
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        row.map(Self::map_row).transpose()
    }

    /// Marks a session as `CLOSED` with all its memberships stopped (docs/05 §4.3).
    ///
    /// Returns `None` when the session was already `CLOSED` or `EXPIRED`: closing is
    /// idempotent (INV-SES-3).
    pub async fn close(
        pool: &Pool,
        tenant_id: TenantId,
        session_id: SessionId,
        reason: ClosedReason,
        now: OffsetDateTime,
    ) -> Result<Option<Session>, StoreError> {
        let row = sqlx::query(
            r#"
            UPDATE sessions
            SET status = 'CLOSED', closed_reason = $4, closed_at = $5, updated_at = $5
            WHERE id = $1 AND tenant_id = $2 AND status = 'ACTIVE'
            RETURNING id, tenant_id, mode, status, owner_subject_id, created_by_kind, created_by_id,
                      expires_at, max_members, snapshot_ttl_seconds, stale_after_seconds, metadata,
                      closed_reason, closed_at, version, created_at, updated_at;
            "#,
        )
        .bind(session_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .bind(true)
        .bind(reason.to_string())
        .bind(now)
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        match row {
            Some(row) => Ok(Some(Self::map_row(row)?)),
            None => Self::get_by_id(pool, tenant_id, session_id).await,
        }
    }

    /// Marks one session as `EXPIRED` (INV-SES-2, lazy verification).
    pub async fn mark_expired(
        pool: &Pool,
        tenant_id: TenantId,
        session_id: SessionId,
        now: OffsetDateTime,
    ) -> Result<Option<Session>, StoreError> {
        let row = sqlx::query(
            r#"
            UPDATE sessions
            SET status = 'EXPIRED', closed_at = $4, updated_at = $4
            WHERE id = $1 AND tenant_id = $2 AND status = 'ACTIVE'
            RETURNING id, tenant_id, mode, status, owner_subject_id, created_by_kind, created_by_id,
                      expires_at, max_members, snapshot_ttl_seconds, stale_after_seconds, metadata,
                      closed_reason, closed_at, version, created_at, updated_at;
            "#,
        )
        .bind(session_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .bind(true)
        .bind(now)
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        row.map(Self::map_row).transpose()
    }

    /// Marks expired sessions as `EXPIRED` (docs/05 §4.3, worker job).
    pub async fn expire_due(
        pool: &Pool,
        now: OffsetDateTime,
        batch_limit: i64,
    ) -> Result<Vec<SessionId>, StoreError> {
        let rows = sqlx::query(
            r#"
            UPDATE sessions
            SET status = 'EXPIRED', closed_at = $2, updated_at = $2
            WHERE id IN (
                SELECT id FROM sessions
                WHERE status = 'ACTIVE' AND expires_at <= $2
                ORDER BY expires_at ASC
                LIMIT $1
                FOR UPDATE SKIP LOCKED
            )
            RETURNING id;
            "#,
        )
        .bind(batch_limit)
        .bind(now)
        .fetch_all(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(rows
            .iter()
            .map(|row| SessionId::from_uuid(row.get("id")))
            .collect())
    }

    /// Counts the active memberships of a session (docs/05 §4.1 `max_members`).
    pub async fn count_active_members(
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

    fn map_row(row: sqlx::postgres::PgRow) -> Result<Session, StoreError> {
        let mode: String = row.get("mode");
        let mode = match mode.as_str() {
            "ONE_TO_ONE" => SessionMode::OneToOne,
            "ONE_TO_MANY" => SessionMode::OneToMany,
            "MANY_TO_MANY" => SessionMode::ManyToMany,
            other => {
                return Err(StoreError::DataMapping(format!(
                    "invalid session mode: {other}"
                )));
            }
        };
        let status: String = row.get("status");
        let status = match status.as_str() {
            "ACTIVE" => SessionStatus::Active,
            "CLOSED" => SessionStatus::Closed,
            "EXPIRED" => SessionStatus::Expired,
            other => {
                return Err(StoreError::DataMapping(format!(
                    "invalid session status: {other}"
                )));
            }
        };
        let created_by_kind: String = row.get("created_by_kind");
        let created_by_id: Uuid = row.get("created_by_id");
        let created_by = map_actor(&created_by_kind, created_by_id)?;
        let closed_reason: Option<String> = row.get("closed_reason");
        let closed_reason = closed_reason
            .as_deref()
            .map(map_closed_reason)
            .transpose()?;

        let metadata: serde_json::Value = row.get("metadata");
        let metadata: std::collections::BTreeMap<String, String> = serde_json::from_value(metadata)
            .map_err(|e| StoreError::DataMapping(format!("metadata: {e}")))?;

        Ok(Session {
            id: SessionId::from_uuid(row.get("id")),
            tenant_id: TenantId::from_uuid(row.get("tenant_id")),
            mode,
            status,
            owner_subject_id: row
                .get::<Option<Uuid>, _>("owner_subject_id")
                .map(SubjectId::from_uuid),
            created_by,
            created_at: row.get("created_at"),
            expires_at: row.get("expires_at"),
            max_members: u32::try_from(row.get::<i32, _>("max_members")).unwrap_or(0),
            snapshot_ttl_seconds: u32::try_from(row.get::<i32, _>("snapshot_ttl_seconds"))
                .unwrap_or(0),
            stale_after_seconds: u32::try_from(row.get::<i32, _>("stale_after_seconds"))
                .unwrap_or(0),
            metadata,
            closed_reason,
            version: u64::try_from(row.get::<i64, _>("version")).unwrap_or(1),
        })
    }
}

/// Maps a stored closed reason to its canonical value.
fn map_closed_reason(value: &str) -> Result<ClosedReason, StoreError> {
    match value {
        "CLOSED_BY_MEMBER" => Ok(ClosedReason::ClosedByMember),
        "CLOSED_BY_APPLICATION" => Ok(ClosedReason::ClosedByApplication),
        "OWNER_LEFT" => Ok(ClosedReason::OwnerLeft),
        "ADMIN" => Ok(ClosedReason::Admin),
        other => Err(StoreError::DataMapping(format!(
            "invalid closed reason: {other}"
        ))),
    }
}
