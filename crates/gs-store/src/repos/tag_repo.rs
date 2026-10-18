//! Tags repository (`docs/25` §2.2, `docs/11` §2.0, E8).
//!
//! **INV-EDGE-1**: there is no column for the `EIK`. The key lives in the tag
//! and in the owner device; the server never receives or stores it.
#![forbid(unsafe_code)]

use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

use gs_domain::ids::{SubjectId, TagId, TenantId};

use crate::error::StoreError;

/// A registered tag.
#[derive(Debug, Clone)]
pub struct Tag {
    pub id: TagId,
    pub tenant_id: TenantId,
    pub owner_subject_id: SubjectId,
    pub model: String,
    pub name: Option<String>,
    pub state: String,
    pub separated_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

pub struct TagRepository;

impl TagRepository {
    /// Registers a tag. The `EIK` is deliberately not a parameter.
    pub async fn create(pool: &PgPool, tag: &Tag) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO tags (id, tenant_id, owner_subject_id, model, name, state, created_at, updated_at)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8);
            "#,
        )
        .bind(tag.id.into_uuid())
        .bind(tag.tenant_id.into_uuid())
        .bind(tag.owner_subject_id.into_uuid())
        .bind(&tag.model)
        .bind(&tag.name)
        .bind(&tag.state)
        .bind(tag.created_at)
        .bind(tag.updated_at)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;
        Ok(())
    }

    /// Fetches a tag scoped to its tenant.
    pub async fn get(
        pool: &PgPool,
        tenant_id: TenantId,
        tag_id: TagId,
    ) -> Result<Option<Tag>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, tenant_id, owner_subject_id, model, name, state, separated_at,
                   created_at, updated_at
            FROM tags WHERE id = $1 AND tenant_id = $2;
            "#,
        )
        .bind(tag_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;
        row.map(map_tag).transpose()
    }

    /// Lists the tags of one owner, optionally filtered by state.
    pub async fn list_by_owner(
        pool: &PgPool,
        tenant_id: TenantId,
        owner_subject_id: SubjectId,
        state: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Tag>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, tenant_id, owner_subject_id, model, name, state, separated_at,
                   created_at, updated_at
            FROM tags
            WHERE tenant_id = $1 AND owner_subject_id = $2
              AND ($3::text IS NULL OR state = $3)
            ORDER BY created_at DESC, id DESC
            LIMIT $4;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(owner_subject_id.into_uuid())
        .bind(state)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(StoreError::Database)?;
        rows.into_iter().map(map_tag).collect()
    }

    /// Lists every tag of a tenant, optionally filtered by state (application view).
    pub async fn list_by_tenant(
        pool: &PgPool,
        tenant_id: TenantId,
        state: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Tag>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, tenant_id, owner_subject_id, model, name, state, separated_at,
                   created_at, updated_at
            FROM tags
            WHERE tenant_id = $1 AND ($2::text IS NULL OR state = $2)
            ORDER BY created_at DESC, id DESC
            LIMIT $3;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(state)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(StoreError::Database)?;
        rows.into_iter().map(map_tag).collect()
    }

    /// Changes the state of a tag (for `SEPARATED`, `separated_at` is set).
    pub async fn set_state(
        pool: &PgPool,
        tenant_id: TenantId,
        tag_id: TagId,
        state: &str,
        separated_at: Option<OffsetDateTime>,
        updated_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let affected = sqlx::query(
            "UPDATE tags SET state = $1, separated_at = $2, updated_at = $3 \
             WHERE id = $4 AND tenant_id = $5;",
        )
        .bind(state)
        .bind(separated_at)
        .bind(updated_at)
        .bind(tag_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .execute(pool)
        .await
        .map_err(StoreError::Database)?
        .rows_affected();
        Ok(affected > 0)
    }
}

fn map_tag(row: sqlx::postgres::PgRow) -> Result<Tag, StoreError> {
    let id: Uuid = row.get("id");
    let tenant_id: Uuid = row.get("tenant_id");
    let owner_subject_id: Uuid = row.get("owner_subject_id");
    Ok(Tag {
        id: TagId::from_uuid(id),
        tenant_id: TenantId::from_uuid(tenant_id),
        owner_subject_id: SubjectId::from_uuid(owner_subject_id),
        model: row.get("model"),
        name: row.get("name"),
        state: row.get("state"),
        separated_at: row.get("separated_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}
