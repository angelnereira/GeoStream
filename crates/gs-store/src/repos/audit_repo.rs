//! Audit event repository with strict tenant isolation (docs/11 §2, docs/10 §8.3, INV-TEN-1).
#![forbid(unsafe_code)]

use sqlx::{PgPool, Row};
use uuid::Uuid;

use gs_domain::ids::{AuditId, RequestId, SubjectId, TenantId};

use crate::error::StoreError;
use crate::models::AuditEvent;

pub struct AuditRepository;

impl AuditRepository {
    /// Records a new audit event.
    pub async fn record(pool: &PgPool, event: &AuditEvent) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO audit_events (
                id, tenant_id, occurred_at, actor_kind, actor_id, subject_id,
                action, target_kind, target_id, outcome, request_id, details
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12);
            "#,
        )
        .bind(event.id.into_uuid())
        .bind(event.tenant_id.into_uuid())
        .bind(event.occurred_at)
        .bind(event.actor_kind.as_str())
        .bind(event.actor_id)
        .bind(event.subject_id.map(|s| s.into_uuid()))
        .bind(&event.action)
        .bind(&event.target_kind)
        .bind(event.target_id)
        .bind(event.outcome.as_str())
        .bind(event.request_id.map(|r| r.into_uuid()))
        .bind(&event.details)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(())
    }

    /// Lists audit events for a tenant ordered by occurrence time descending.
    pub async fn list_by_tenant(
        pool: &PgPool,
        tenant_id: TenantId,
        limit: i64,
    ) -> Result<Vec<AuditEvent>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, tenant_id, occurred_at, actor_kind, actor_id, subject_id,
                   action, target_kind, target_id, outcome, request_id, details
            FROM audit_events
            WHERE tenant_id = $1
            ORDER BY occurred_at DESC, id DESC
            LIMIT $2;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(StoreError::Database)?;

        rows.into_iter().map(Self::map_row).collect()
    }

    /// Lists audit events associated with a subject within a tenant.
    pub async fn list_by_subject(
        pool: &PgPool,
        tenant_id: TenantId,
        subject_id: SubjectId,
        limit: i64,
    ) -> Result<Vec<AuditEvent>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, tenant_id, occurred_at, actor_kind, actor_id, subject_id,
                   action, target_kind, target_id, outcome, request_id, details
            FROM audit_events
            WHERE tenant_id = $1 AND subject_id = $2
            ORDER BY occurred_at DESC, id DESC
            LIMIT $3;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(subject_id.into_uuid())
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(StoreError::Database)?;

        rows.into_iter().map(Self::map_row).collect()
    }

    fn map_row(row: sqlx::postgres::PgRow) -> Result<AuditEvent, StoreError> {
        let id_raw: Uuid = row.get("id");
        let tenant_id_raw: Uuid = row.get("tenant_id");

        let actor_kind_str: String = row.get("actor_kind");
        let actor_kind = actor_kind_str.parse().map_err(|_| {
            StoreError::DataMapping(format!("invalid actor kind: {actor_kind_str}"))
        })?;

        let outcome_str: String = row.get("outcome");
        let outcome = outcome_str.parse().map_err(|_| {
            StoreError::DataMapping(format!("invalid audit outcome: {outcome_str}"))
        })?;

        let subject_id_raw: Option<Uuid> = row.get("subject_id");
        let subject_id = subject_id_raw.map(SubjectId::from_uuid);

        let request_id_raw: Option<Uuid> = row.get("request_id");
        let request_id = request_id_raw.map(RequestId::from_uuid);

        Ok(AuditEvent {
            id: AuditId::from_uuid(id_raw),
            tenant_id: TenantId::from_uuid(tenant_id_raw),
            occurred_at: row.get("occurred_at"),
            actor_kind,
            actor_id: row.get("actor_id"),
            subject_id,
            action: row.get("action"),
            target_kind: row.get("target_kind"),
            target_id: row.get("target_id"),
            outcome,
            request_id,
            details: row.get("details"),
        })
    }
}
