//! Retention purges of the worker role (`docs/10` §7.1, `docs/11` §5).
//!
//! Every purge deletes in batches that lock their rows with `FOR UPDATE SKIP LOCKED`, so several
//! worker instances can run it at once without waiting for each other or deleting a row twice. They
//! return how many rows they removed and never anything about them (`docs/10` §8.2).
use sqlx::PgPool;
use time::OffsetDateTime;

use crate::StoreError;

/// Deletes of data that has outlived its retention window.
pub struct RetentionRepository;

impl RetentionRepository {
    /// Deletes up to `batch` closed or expired sessions that ended before `older_than`. Their
    /// memberships and invitations go with them (`ON DELETE CASCADE`). Active sessions are never
    /// touched, whatever their age.
    pub async fn purge_ended_sessions(
        pool: &PgPool,
        older_than: OffsetDateTime,
        batch: i64,
    ) -> Result<u64, StoreError> {
        let result = sqlx::query(
            r#"
            DELETE FROM sessions
             WHERE id IN (
                SELECT id FROM sessions
                 WHERE status IN ('CLOSED', 'EXPIRED') AND closed_at < $1
                 ORDER BY closed_at
                 LIMIT $2
                 FOR UPDATE SKIP LOCKED
             );
            "#,
        )
        .bind(older_than)
        .bind(batch)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;
        Ok(result.rows_affected())
    }

    /// Deletes up to `batch` invitations that expired or were revoked before `older_than`.
    pub async fn purge_spent_invitations(
        pool: &PgPool,
        older_than: OffsetDateTime,
        batch: i64,
    ) -> Result<u64, StoreError> {
        let result = sqlx::query(
            r#"
            DELETE FROM invitations
             WHERE id IN (
                SELECT id FROM invitations
                 WHERE expires_at < $1 OR revoked_at < $1
                 ORDER BY created_at
                 LIMIT $2
                 FOR UPDATE SKIP LOCKED
             );
            "#,
        )
        .bind(older_than)
        .bind(batch)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;
        Ok(result.rows_affected())
    }
}
