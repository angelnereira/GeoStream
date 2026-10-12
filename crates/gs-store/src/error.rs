//! Storage error types.
#![forbid(unsafe_code)]

use thiserror::Error;

/// Storage and persistence error.
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("schema version mismatch: expected version {expected}, found {found:?}")]
    SchemaVersionMismatch { expected: i64, found: Option<i64> },

    #[error("tenant not found or inactive: {0}")]
    TenantNotFound(String),

    #[error("entity not found: {entity} id={id}")]
    NotFound { entity: &'static str, id: String },

    #[error("data mapping error: {0}")]
    DataMapping(String),
}
