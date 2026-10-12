//! Database schema migrations and version validation (docs/11 §2, §4, FR-OPS-004).
#![forbid(unsafe_code)]

use sqlx::migrate::Migrator;
use sqlx::{PgPool, Row};

use crate::error::StoreError;

/// Embedded migrations from the repository `migrations/` directory.
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

/// The expected latest schema version according to embedded migrations.
pub fn expected_schema_version() -> i64 {
    MIGRATOR.iter().map(|m| m.version).max().unwrap_or(0)
}

/// Applies all pending migrations up to the latest embedded version.
///
/// Idempotent: re-running on an up-to-date database is a no-op (FR-OPS-004).
pub async fn run_migrations(pool: &PgPool) -> Result<(), StoreError> {
    MIGRATOR.run(pool).await.map_err(StoreError::Migration)
}

/// Checks that the database has all embedded migrations applied.
///
/// Used by `geostream serve` on startup to ensure the schema matches the binary version
/// without automatically applying migrations (docs/11 §4).
pub async fn check_schema_version(pool: &PgPool) -> Result<(), StoreError> {
    let expected = expected_schema_version();

    // Check if _sqlx_migrations table exists
    let table_exists: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT FROM information_schema.tables 
            WHERE table_schema = 'public' 
            AND table_name = '_sqlx_migrations'
        );
        "#,
    )
    .fetch_one(pool)
    .await
    .map_err(StoreError::Database)?;

    if !table_exists {
        return Err(StoreError::SchemaVersionMismatch {
            expected,
            found: None,
        });
    }

    let latest_version: Option<i64> = sqlx::query(
        r#"
        SELECT version FROM _sqlx_migrations 
        WHERE success = true 
        ORDER BY version DESC 
        LIMIT 1;
        "#,
    )
    .fetch_optional(pool)
    .await
    .map_err(StoreError::Database)?
    .map(|row| row.get::<i64, _>("version"));

    match latest_version {
        Some(v) if v == expected => Ok(()),
        other => Err(StoreError::SchemaVersionMismatch {
            expected,
            found: other,
        }),
    }
}

/// Migration plan status for dry-run inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationStatus {
    pub expected_version: i64,
    pub current_version: Option<i64>,
    pub pending_count: usize,
}

/// Plans migrations, returning the current version, expected version, and number of pending migrations.
pub async fn plan_migrations(pool: &PgPool) -> Result<MigrationStatus, StoreError> {
    let expected = expected_schema_version();

    let table_exists: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT FROM information_schema.tables 
            WHERE table_schema = 'public' 
            AND table_name = '_sqlx_migrations'
        );
        "#,
    )
    .fetch_one(pool)
    .await
    .map_err(StoreError::Database)?;

    if !table_exists {
        return Ok(MigrationStatus {
            expected_version: expected,
            current_version: None,
            pending_count: MIGRATOR.iter().count(),
        });
    }

    let latest_version: Option<i64> = sqlx::query(
        r#"
        SELECT version FROM _sqlx_migrations 
        WHERE success = true 
        ORDER BY version DESC 
        LIMIT 1;
        "#,
    )
    .fetch_optional(pool)
    .await
    .map_err(StoreError::Database)?
    .map(|row| row.get::<i64, _>("version"));

    let applied_floor = latest_version.unwrap_or(0);
    let pending_count = MIGRATOR
        .iter()
        .filter(|m| m.version > applied_floor)
        .count();

    Ok(MigrationStatus {
        expected_version: expected,
        current_version: latest_version,
        pending_count,
    })
}

/// Connects a new PgPool using standard options for administrative commands and testing.
pub async fn connect_pool(database_url: &str) -> Result<PgPool, StoreError> {
    sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(database_url)
        .await
        .map_err(StoreError::Database)
}
