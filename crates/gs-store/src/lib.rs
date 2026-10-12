//! PostgreSQL repositories, transactions, outbox, audit and idempotency.
#![forbid(unsafe_code)]

pub mod error;
pub mod migrate;

pub use error::StoreError;
pub use migrate::{
    MIGRATOR, MigrationStatus, check_schema_version, connect_pool, expected_schema_version,
    plan_migrations, run_migrations,
};
