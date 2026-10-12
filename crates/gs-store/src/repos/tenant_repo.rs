//! Tenant repository with strict tenant isolation (docs/11 §2, INV-TEN-1).
#![forbid(unsafe_code)]

use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

use gs_domain::ids::TenantId;

use crate::error::StoreError;
use crate::models::{Tenant, TenantStatus};

pub struct TenantRepository;

impl TenantRepository {
    /// Inserts a new tenant record.
    pub async fn create(pool: &PgPool, tenant: &Tenant) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO tenants (id, name, status, policy, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6);
            "#,
        )
        .bind(tenant.id.into_uuid())
        .bind(&tenant.name)
        .bind(tenant.status.as_str())
        .bind(&tenant.policy)
        .bind(tenant.created_at)
        .bind(tenant.updated_at)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(())
    }

    /// Fetches a tenant by identifier.
    pub async fn get_by_id(
        pool: &PgPool,
        tenant_id: TenantId,
    ) -> Result<Option<Tenant>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, name, status, policy, created_at, updated_at
            FROM tenants
            WHERE id = $1;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(row.map(Self::map_row))
    }

    /// Updates tenant status (ACTIVE / DISABLED).
    pub async fn update_status(
        pool: &PgPool,
        tenant_id: TenantId,
        status: TenantStatus,
        now: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let res = sqlx::query(
            r#"
            UPDATE tenants
            SET status = $2, updated_at = $3
            WHERE id = $1;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(status.as_str())
        .bind(now)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(res.rows_affected() > 0)
    }

    /// Updates tenant policy configuration JSON.
    pub async fn update_policy(
        pool: &PgPool,
        tenant_id: TenantId,
        policy: &serde_json::Value,
        now: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let res = sqlx::query(
            r#"
            UPDATE tenants
            SET policy = $2, updated_at = $3
            WHERE id = $1;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(policy)
        .bind(now)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(res.rows_affected() > 0)
    }

    /// Lists all tenants ordered by creation time.
    pub async fn list(pool: &PgPool) -> Result<Vec<Tenant>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, status, policy, created_at, updated_at
            FROM tenants
            ORDER BY created_at ASC;
            "#,
        )
        .fetch_all(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(rows.into_iter().map(Self::map_row).collect())
    }

    fn map_row(row: sqlx::postgres::PgRow) -> Tenant {
        let id_raw: Uuid = row.get("id");
        let status_str: String = row.get("status");
        let status = status_str.parse().unwrap_or(TenantStatus::Active);

        Tenant {
            id: TenantId::from_uuid(id_raw),
            name: row.get("name"),
            status,
            policy: row.get("policy"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        }
    }
}
