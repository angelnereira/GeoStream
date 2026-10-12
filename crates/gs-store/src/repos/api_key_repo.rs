//! API Key repository with strict tenant isolation (docs/11 §2, docs/06 §3, INV-TEN-1).
#![forbid(unsafe_code)]

use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

use gs_domain::ids::{ApiKeyId, ApplicationId, TenantId};

use crate::error::StoreError;
use crate::models::ApiKey;

pub struct ApiKeyRepository;

impl ApiKeyRepository {
    /// Inserts a new API key record.
    pub async fn create(pool: &PgPool, key: &ApiKey) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO api_keys (
                id, tenant_id, application_id, secret_sha256, scopes,
                label, status, expires_at, last_used_at, created_at, revoked_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11);
            "#,
        )
        .bind(key.id.into_uuid())
        .bind(key.tenant_id.into_uuid())
        .bind(key.application_id.into_uuid())
        .bind(key.secret_sha256.as_slice())
        .bind(&key.scopes)
        .bind(&key.label)
        .bind(key.status.as_str())
        .bind(key.expires_at)
        .bind(key.last_used_at)
        .bind(key.created_at)
        .bind(key.revoked_at)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(())
    }

    /// Fetches an API key by identifier, scoped to a specific tenant.
    pub async fn get_by_id(
        pool: &PgPool,
        tenant_id: TenantId,
        key_id: ApiKeyId,
    ) -> Result<Option<ApiKey>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, tenant_id, application_id, secret_sha256, scopes, label,
                   status, expires_at, last_used_at, created_at, revoked_at
            FROM api_keys
            WHERE id = $1 AND tenant_id = $2;
            "#,
        )
        .bind(key_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        row.map(Self::map_row).transpose()
    }

    /// Looks up an API key by identifier for authentication purposes.
    /// Used by the auth middleware when validating `Authorization: Bearer gsk_<key_id>.<secret>`.
    pub async fn find_by_id_for_auth(
        pool: &PgPool,
        key_id: ApiKeyId,
    ) -> Result<Option<ApiKey>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, tenant_id, application_id, secret_sha256, scopes, label,
                   status, expires_at, last_used_at, created_at, revoked_at
            FROM api_keys
            WHERE id = $1;
            "#,
        )
        .bind(key_id.into_uuid())
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        row.map(Self::map_row).transpose()
    }

    /// Lists all API keys for an application, scoped to a specific tenant.
    pub async fn list_by_application(
        pool: &PgPool,
        tenant_id: TenantId,
        application_id: ApplicationId,
    ) -> Result<Vec<ApiKey>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, tenant_id, application_id, secret_sha256, scopes, label,
                   status, expires_at, last_used_at, created_at, revoked_at
            FROM api_keys
            WHERE application_id = $1 AND tenant_id = $2
            ORDER BY created_at ASC;
            "#,
        )
        .bind(application_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .fetch_all(pool)
        .await
        .map_err(StoreError::Database)?;

        rows.into_iter().map(Self::map_row).collect()
    }

    /// Revokes an API key, setting status to REVOKED and recording revoked_at.
    pub async fn revoke(
        pool: &PgPool,
        tenant_id: TenantId,
        key_id: ApiKeyId,
        now: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let res = sqlx::query(
            r#"
            UPDATE api_keys
            SET status = 'REVOKED', revoked_at = $3
            WHERE id = $1 AND tenant_id = $2 AND status = 'ACTIVE';
            "#,
        )
        .bind(key_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .bind(now)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(res.rows_affected() > 0)
    }

    /// Records usage of an API key by updating `last_used_at`.
    pub async fn record_usage(
        pool: &PgPool,
        key_id: ApiKeyId,
        now: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let res = sqlx::query(
            r#"
            UPDATE api_keys
            SET last_used_at = $2
            WHERE id = $1;
            "#,
        )
        .bind(key_id.into_uuid())
        .bind(now)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(res.rows_affected() > 0)
    }

    fn map_row(row: sqlx::postgres::PgRow) -> Result<ApiKey, StoreError> {
        let id_raw: Uuid = row.get("id");
        let tenant_id_raw: Uuid = row.get("tenant_id");
        let application_id_raw: Uuid = row.get("application_id");
        let secret_sha256_vec: Vec<u8> = row.get("secret_sha256");
        let secret_sha256: [u8; 32] = secret_sha256_vec.try_into().map_err(|_| {
            StoreError::DataMapping("invalid secret_sha256 length in api_keys".to_string())
        })?;

        let status_str: String = row.get("status");
        let status = status_str.parse().map_err(|_| {
            StoreError::DataMapping(format!("invalid api key status: {status_str}"))
        })?;

        Ok(ApiKey {
            id: ApiKeyId::from_uuid(id_raw),
            tenant_id: TenantId::from_uuid(tenant_id_raw),
            application_id: ApplicationId::from_uuid(application_id_raw),
            secret_sha256,
            scopes: row.get("scopes"),
            label: row.get("label"),
            status,
            expires_at: row.get("expires_at"),
            last_used_at: row.get("last_used_at"),
            created_at: row.get("created_at"),
            revoked_at: row.get("revoked_at"),
        })
    }
}
