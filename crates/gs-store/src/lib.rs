//! PostgreSQL repositories, transactions, outbox, audit and idempotency.
#![forbid(unsafe_code)]

pub mod error;
pub mod migrate;
pub mod models;
pub mod repos;

pub use error::StoreError;
pub use migrate::{
    MIGRATOR, MigrationStatus, check_schema_version, connect_pool, expected_schema_version,
    plan_migrations, run_migrations,
};
pub use models::*;
pub use repos::*;
