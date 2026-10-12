//! PostgreSQL repositories, transactions, outbox, audit and idempotency.
#![forbid(unsafe_code)]

pub mod error;
pub mod migrate;
pub mod models;
pub mod repos;

pub use error::StoreError;
// Re-exported so crates above `gs-store` can name the pool type without depending
// on `sqlx` directly (docs/04 §4.1: only `gs-store` talks to PostgreSQL).
pub use migrate::{
    MIGRATOR, MigrationStatus, check_schema_version, connect_pool, expected_schema_version,
    plan_migrations, run_migrations,
};
pub use models::*;
pub use repos::*;
pub use sqlx::PgPool;
