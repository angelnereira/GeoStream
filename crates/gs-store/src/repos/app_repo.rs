//! Application repository with strict tenant isolation (docs/11 §2, docs/05 §3.2, INV-TEN-1).
#![forbid(unsafe_code)]

use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

use gs_domain::ids::{ApplicationId, TenantId};

use crate::error::StoreError;
use crate::models::{Application, ApplicationStatus};

pub struct ApplicationRepository;

impl ApplicationRepository {
    /// Inserts a new application record.
    pub async fn create(pool: &PgPool, app: &Application) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO applications (id, tenant_id, name, status, allowed_device_scopes, allowed_origins, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8);
            "#,
        )
        .bind(app.id.into_uuid())
        .bind(app.tenant_id.into_uuid())
        .bind(&app.name)
        .bind(app.status.as_str())
        .bind(&app.allowed_device_scopes)
        .bind(&app.allowed_origins)
        .bind(app.created_at)
        .bind(app.updated_at)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(())
    }

    /// Fetches an application by identifier, scoped to a specific tenant.
    pub async fn get_by_id(
        pool: &PgPool,
        tenant_id: TenantId,
        app_id: ApplicationId,
    ) -> Result<Option<Application>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, tenant_id, name, status, allowed_device_scopes, allowed_origins, created_at, updated_at
            FROM applications
            WHERE id = $1 AND tenant_id = $2;
            "#,
        )
        .bind(app_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        row.map(Self::map_row).transpose()
    }

    /// Lists all applications belonging to a specific tenant.
    pub async fn list_by_tenant(
        pool: &PgPool,
        tenant_id: TenantId,
    ) -> Result<Vec<Application>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, tenant_id, name, status, allowed_device_scopes, allowed_origins, created_at, updated_at
            FROM applications
            WHERE tenant_id = $1
            ORDER BY created_at ASC;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .fetch_all(pool)
        .await
        .map_err(StoreError::Database)?;

        rows.into_iter().map(Self::map_row).collect()
    }

    /// Resolves an application by identifier without a tenant filter.
    ///
    /// Used by `geostream admin app …` and `geostream admin key …` to derive the owning
    /// tenant from the referenced entity (docs/16 §3). Mutations performed afterwards are
    /// still scoped to the resolved tenant (INV-TEN-1).
    pub async fn find_by_id(
        pool: &PgPool,
        app_id: ApplicationId,
    ) -> Result<Option<Application>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, tenant_id, name, status, allowed_device_scopes, allowed_origins, created_at, updated_at
            FROM applications
            WHERE id = $1;
            "#,
        )
        .bind(app_id.into_uuid())
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        row.map(Self::map_row).transpose()
    }

    /// Updates application status (ACTIVE / DISABLED).
    pub async fn update_status(
        pool: &PgPool,
        tenant_id: TenantId,
        app_id: ApplicationId,
        status: ApplicationStatus,
        now: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let res = sqlx::query(
            r#"
            UPDATE applications
            SET status = $3, updated_at = $4
            WHERE id = $1 AND tenant_id = $2;
            "#,
        )
        .bind(app_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .bind(status.as_str())
        .bind(now)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(res.rows_affected() > 0)
    }

    /// Updates application scopes and origins configuration.
    pub async fn update_scopes_and_origins(
        pool: &PgPool,
        tenant_id: TenantId,
        app_id: ApplicationId,
        allowed_device_scopes: &[String],
        allowed_origins: &[String],
        now: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let res = sqlx::query(
            r#"
            UPDATE applications
            SET allowed_device_scopes = $3, allowed_origins = $4, updated_at = $5
            WHERE id = $1 AND tenant_id = $2;
            "#,
        )
        .bind(app_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .bind(allowed_device_scopes)
        .bind(allowed_origins)
        .bind(now)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(res.rows_affected() > 0)
    }

    fn map_row(row: sqlx::postgres::PgRow) -> Result<Application, StoreError> {
        let id_raw: Uuid = row.get("id");
        let tenant_id_raw: Uuid = row.get("tenant_id");
        let status_str: String = row.get("status");
        let status = status_str.parse().map_err(|_| {
            StoreError::DataMapping(format!("invalid application status: {status_str}"))
        })?;

        Ok(Application {
            id: ApplicationId::from_uuid(id_raw),
            tenant_id: TenantId::from_uuid(tenant_id_raw),
            name: row.get("name"),
            status,
            allowed_device_scopes: row.get("allowed_device_scopes"),
            allowed_origins: row.get("allowed_origins"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }
}
