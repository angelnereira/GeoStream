//! Subject repository with strict tenant isolation (docs/11 §2, docs/05 §3.3, INV-TEN-1, INV-SUB-1).
#![forbid(unsafe_code)]

use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

use gs_domain::ids::{SubjectId, TenantId};

use crate::error::StoreError;
use crate::models::Subject;

pub struct SubjectRepository;

impl SubjectRepository {
    /// Inserts a new subject record.
    pub async fn create(pool: &PgPool, subject: &Subject) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO subjects (id, tenant_id, external_subject_id, status, token_epoch, created_at, updated_at, erased_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8);
            "#,
        )
        .bind(subject.id.into_uuid())
        .bind(subject.tenant_id.into_uuid())
        .bind(&subject.external_subject_id)
        .bind(subject.status.as_str())
        .bind(subject.token_epoch as i32)
        .bind(subject.created_at)
        .bind(subject.updated_at)
        .bind(subject.erased_at)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(())
    }

    /// Fetches a subject by identifier, scoped to a specific tenant.
    pub async fn get_by_id(
        pool: &PgPool,
        tenant_id: TenantId,
        subject_id: SubjectId,
    ) -> Result<Option<Subject>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, tenant_id, external_subject_id, status, token_epoch, created_at, updated_at, erased_at
            FROM subjects
            WHERE id = $1 AND tenant_id = $2;
            "#,
        )
        .bind(subject_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        row.map(Self::map_row).transpose()
    }

    /// Finds an active subject by external subject identifier within a tenant.
    pub async fn find_active_by_external_id(
        pool: &PgPool,
        tenant_id: TenantId,
        external_subject_id: &str,
    ) -> Result<Option<Subject>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, tenant_id, external_subject_id, status, token_epoch, created_at, updated_at, erased_at
            FROM subjects
            WHERE tenant_id = $1 AND external_subject_id = $2 AND status = 'ACTIVE';
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(external_subject_id)
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        row.map(Self::map_row).transpose()
    }

    /// Atomically finds or creates an active subject with the given external ID.
    /// If an active subject already exists, its existing record is returned.
    /// If none exists (or previous records were ERASED), a new active subject is created.
    pub async fn get_or_create_active(
        pool: &PgPool,
        tenant_id: TenantId,
        external_subject_id: &str,
        now: OffsetDateTime,
    ) -> Result<Subject, StoreError> {
        let new_id = SubjectId::new();
        let row = sqlx::query(
            r#"
            INSERT INTO subjects (id, tenant_id, external_subject_id, status, token_epoch, created_at, updated_at)
            VALUES ($1, $2, $3, 'ACTIVE', 0, $4, $4)
            ON CONFLICT (tenant_id, external_subject_id) WHERE status = 'ACTIVE'
            DO UPDATE SET updated_at = EXCLUDED.updated_at
            RETURNING id, tenant_id, external_subject_id, status, token_epoch, created_at, updated_at, erased_at;
            "#,
        )
        .bind(new_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .bind(external_subject_id)
        .bind(now)
        .fetch_one(pool)
        .await
        .map_err(StoreError::Database)?;

        Self::map_row(row)
    }

    /// Atomically increments the token epoch of an active subject (revoking all active tokens).
    pub async fn increment_token_epoch(
        pool: &PgPool,
        tenant_id: TenantId,
        subject_id: SubjectId,
        now: OffsetDateTime,
    ) -> Result<u32, StoreError> {
        let row = sqlx::query(
            r#"
            UPDATE subjects
            SET token_epoch = token_epoch + 1, updated_at = $3
            WHERE id = $1 AND tenant_id = $2 AND status = 'ACTIVE'
            RETURNING token_epoch;
            "#,
        )
        .bind(subject_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .bind(now)
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        match row {
            Some(r) => {
                let epoch_i32: i32 = r.get("token_epoch");
                Ok(u32::try_from(epoch_i32).unwrap_or(0))
            }
            None => Err(StoreError::NotFound {
                entity: "subject",
                id: subject_id.to_string(),
            }),
        }
    }

    /// Atomically increments the token epoch of an active subject (revoking all active tokens),
    /// returning the updated subject record.
    pub async fn revoke_tokens(
        pool: &PgPool,
        tenant_id: TenantId,
        subject_id: SubjectId,
        now: OffsetDateTime,
    ) -> Result<Subject, StoreError> {
        let row = sqlx::query(
            r#"
            UPDATE subjects
            SET token_epoch = token_epoch + 1, updated_at = $3
            WHERE id = $1 AND tenant_id = $2 AND status = 'ACTIVE'
            RETURNING id, tenant_id, external_subject_id, status, token_epoch, created_at, updated_at, erased_at;
            "#,
        )
        .bind(subject_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .bind(now)
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        match row {
            Some(r) => Self::map_row(r),
            None => Err(StoreError::NotFound {
                entity: "subject",
                id: subject_id.to_string(),
            }),
        }
    }

    /// Erases a subject, anonymizing its external identifier and marking status as ERASED.
    pub async fn erase(
        pool: &PgPool,
        tenant_id: TenantId,
        subject_id: SubjectId,
        now: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let res = sqlx::query(
            r#"
            UPDATE subjects
            SET status = 'ERASED', external_subject_id = NULL, erased_at = $3, updated_at = $3
            WHERE id = $1 AND tenant_id = $2 AND status = 'ACTIVE';
            "#,
        )
        .bind(subject_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .bind(now)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(res.rows_affected() > 0)
    }

    /// Lists subjects for a tenant with a pagination limit.
    pub async fn list_by_tenant(
        pool: &PgPool,
        tenant_id: TenantId,
        limit: i64,
    ) -> Result<Vec<Subject>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, tenant_id, external_subject_id, status, token_epoch, created_at, updated_at, erased_at
            FROM subjects
            WHERE tenant_id = $1
            ORDER BY created_at ASC
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

    fn map_row(row: sqlx::postgres::PgRow) -> Result<Subject, StoreError> {
        let id_raw: Uuid = row.get("id");
        let tenant_id_raw: Uuid = row.get("tenant_id");
        let status_str: String = row.get("status");
        let status = status_str.parse().map_err(|_| {
            StoreError::DataMapping(format!("invalid subject status: {status_str}"))
        })?;

        let epoch_i32: i32 = row.get("token_epoch");
        let token_epoch = u32::try_from(epoch_i32).unwrap_or(0);

        Ok(Subject {
            id: SubjectId::from_uuid(id_raw),
            tenant_id: TenantId::from_uuid(tenant_id_raw),
            external_subject_id: row.get("external_subject_id"),
            status,
            token_epoch,
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            erased_at: row.get("erased_at"),
        })
    }
}
