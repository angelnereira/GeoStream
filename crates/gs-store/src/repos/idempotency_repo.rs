//! Idempotency key storage (docs/07 §6, `migrations/0001_init.sql`).
//!
//! The repository implements the storage steps of the algorithm: an `IN_PROGRESS` record is
//! inserted with a controlled conflict, `COMPLETED` stores the HTTP status and the encrypted
//! response body, and a 5xx deletes the record so the client can retry.
#![forbid(unsafe_code)]

use sqlx::Row;
use time::OffsetDateTime;
use uuid::Uuid;

use gs_domain::ids::TenantId;

use crate::PgPool as Pool;
use crate::error::StoreError;

/// State of an idempotency record (docs/07 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdempotencyState {
    /// The first request is still being processed.
    InProgress,
    /// The response is stored and can be replayed.
    Completed,
}

impl IdempotencyState {
    /// Canonical database value.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::InProgress => "IN_PROGRESS",
            Self::Completed => "COMPLETED",
        }
    }

    /// Parses the stored value.
    pub fn parse(value: &str) -> Result<Self, StoreError> {
        match value {
            "IN_PROGRESS" => Ok(Self::InProgress),
            "COMPLETED" => Ok(Self::Completed),
            other => Err(StoreError::DataMapping(format!(
                "invalid idempotency state: {other}"
            ))),
        }
    }
}

/// A stored idempotency record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdempotencyRecord {
    /// Status of the stored response.
    pub response_status: Option<i16>,
    /// Encrypted response body (`v1:<key_id>:<nonce>:<ciphertext>`), when the state is
    /// `COMPLETED` and the response carried a body.
    pub response_body_enc: Option<Vec<u8>>,
    /// State of the record.
    pub state: IdempotencyState,
}

/// Outcome of trying to register a new idempotency key (docs/07 §6 steps 3 to 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BeginOutcome {
    /// The key was free; the caller processes the request.
    Started,
    /// A `COMPLETED` record exists with the same fingerprint: the stored response is replayed.
    Replay(Box<IdempotencyRecord>),
    /// A record exists with a different fingerprint: 422 `IDEMPOTENCY_KEY_REUSED`.
    FingerprintMismatch,
    /// The key is `IN_PROGRESS`: 409 `CONFLICT`.
    InProgress,
}

/// Idempotency key repository (docs/07 §6).
pub struct IdempotencyRepository;

impl IdempotencyRepository {
    /// Registers a request under `(tenant_id, principal_id, idempotency_key)`.
    ///
    /// The insert is `ON CONFLICT DO NOTHING`, so a concurrent request that wins the race is
    /// detected by the absence of the inserted row (docs/07 §6 step 4).
    pub async fn begin(
        pool: &Pool,
        tenant_id: TenantId,
        principal_id: Uuid,
        idempotency_key: &str,
        request_sha256: &[u8; 32],
        expires_at: OffsetDateTime,
    ) -> Result<BeginOutcome, StoreError> {
        let inserted = sqlx::query(
            r#"
            INSERT INTO idempotency_keys (
                tenant_id, principal_id, idempotency_key, request_sha256, state, expires_at
            )
            VALUES ($1, $2, $3, $4, 'IN_PROGRESS', $5)
            ON CONFLICT DO NOTHING
            RETURNING state;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(principal_id)
        .bind(idempotency_key)
        .bind(request_sha256.as_slice())
        .bind(expires_at)
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        if inserted.is_some() {
            return Ok(BeginOutcome::Started);
        }

        let row = sqlx::query(
            r#"
            SELECT state, response_status, response_body_enc, request_sha256
            FROM idempotency_keys
            WHERE tenant_id = $1 AND principal_id = $2 AND idempotency_key = $3;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(principal_id)
        .bind(idempotency_key)
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        let Some(row) = row else {
            // The row vanished between the conflict and the read: the key is free again.
            return Ok(BeginOutcome::Started);
        };

        let stored_fingerprint: Vec<u8> = row.get("request_sha256");
        if stored_fingerprint.as_slice() != request_sha256.as_slice() {
            return Ok(BeginOutcome::FingerprintMismatch);
        }

        let state: String = row.get("state");
        match IdempotencyState::parse(&state)? {
            IdempotencyState::InProgress => Ok(BeginOutcome::InProgress),
            IdempotencyState::Completed => Ok(BeginOutcome::Replay(Box::new(Self::map_row(row)?))),
        }
    }

    /// Reads a record.
    pub async fn find(
        pool: &Pool,
        tenant_id: TenantId,
        principal_id: Uuid,
        idempotency_key: &str,
    ) -> Result<Option<IdempotencyRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT state, response_status, response_body_enc
            FROM idempotency_keys
            WHERE tenant_id = $1 AND principal_id = $2 AND idempotency_key = $3;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(principal_id)
        .bind(idempotency_key)
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        row.map(Self::map_row).transpose()
    }

    /// Marks a record as `COMPLETED` with the stored response (docs/07 §6 step 4).
    pub async fn complete(
        pool: &Pool,
        tenant_id: TenantId,
        principal_id: Uuid,
        idempotency_key: &str,
        response_status: i16,
        response_body_enc: Option<&[u8]>,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE idempotency_keys
            SET state = 'COMPLETED', response_status = $4, response_body_enc = $5
            WHERE tenant_id = $1 AND principal_id = $2 AND idempotency_key = $3;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(principal_id)
        .bind(idempotency_key)
        .bind(response_status)
        .bind(response_body_enc)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(result.rows_affected() > 0)
    }

    /// Deletes a record so the request can be retried (docs/07 §6 step 8).
    pub async fn discard(
        pool: &Pool,
        tenant_id: TenantId,
        principal_id: Uuid,
        idempotency_key: &str,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            DELETE FROM idempotency_keys
            WHERE tenant_id = $1 AND principal_id = $2 AND idempotency_key = $3;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(principal_id)
        .bind(idempotency_key)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(result.rows_affected() > 0)
    }

    /// Deletes expired records (worker job, docs/07 §6 step 9).
    pub async fn purge_expired(pool: &Pool, now: OffsetDateTime) -> Result<u64, StoreError> {
        let result = sqlx::query(
            r#"
            DELETE FROM idempotency_keys
            WHERE expires_at <= $1;
            "#,
        )
        .bind(now)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(result.rows_affected())
    }

    fn map_row(row: sqlx::postgres::PgRow) -> Result<IdempotencyRecord, StoreError> {
        let state: String = row.get("state");
        let state = IdempotencyState::parse(&state)?;
        Ok(IdempotencyRecord {
            response_status: row.get("response_status"),
            response_body_enc: row.get("response_body_enc"),
            state,
        })
    }
}
