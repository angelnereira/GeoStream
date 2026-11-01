//! Console accounts, one-time codes, sessions and organization memberships
//! (docs/11 §2.0.1, ADR-0033 §3, ADR-0036).
//!
//! Every limit that must hold under concurrency — cooldown, attempts, single use and the daily
//! e-mail budget — is enforced by one atomic SQL statement, not by a read followed by a write.
//! Secrets never reach this layer in clear: it receives the HMAC of a code and the SHA-256 of a
//! session token (docs/10 §5.6), and the e-mail only as the normalized address the account is
//! keyed by.
#![forbid(unsafe_code)]

use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

use gs_domain::ids::{AccountId, TenantId};

use crate::error::StoreError;
use crate::models::{AccountStatus, ConsoleAccount, ConsoleRole, Tenant};

/// Result of asking for a new one-time code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtpIssue {
    /// A new code was stored; the caller may send the e-mail.
    Issued,
    /// A code was issued less than the cooldown ago: nothing stored, nothing to send.
    CoolingDown,
}

/// A live code after one attempt was counted against it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtpAttempt {
    pub id: Uuid,
    /// HMAC of the code to compare, in constant time, with the HMAC of the submitted one.
    pub code_hmac: Vec<u8>,
    /// Attempts used, this one included.
    pub attempts: i32,
}

/// An organization of an account and the role the account has in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrganizationMembership {
    pub tenant: Tenant,
    pub role: ConsoleRole,
}

pub struct ConsoleRepository;

impl ConsoleRepository {
    // ----- one-time codes -------------------------------------------------------------------

    /// Stores a new code for `email_hash`, replacing the live one unless it is younger than the
    /// cooldown. One statement: two concurrent requests cannot both pass the cooldown.
    pub async fn issue_otp(
        pool: &PgPool,
        email_hash: &[u8],
        code_hmac: &[u8],
        ttl_seconds: f64,
        cooldown_seconds: f64,
    ) -> Result<OtpIssue, StoreError> {
        let issued = sqlx::query(
            r#"
            INSERT INTO console_otp_codes (id, email_hash, code_hmac, expires_at, attempts, created_at)
            VALUES ($1, $2, $3, now() + make_interval(secs => $4), 0, now())
            ON CONFLICT (email_hash) WHERE consumed_at IS NULL DO UPDATE
               SET id = EXCLUDED.id,
                   code_hmac = EXCLUDED.code_hmac,
                   expires_at = EXCLUDED.expires_at,
                   attempts = 0,
                   created_at = EXCLUDED.created_at
             WHERE console_otp_codes.created_at <= now() - make_interval(secs => $5)
            RETURNING id;
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(email_hash)
        .bind(code_hmac)
        .bind(ttl_seconds)
        .bind(cooldown_seconds)
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;
        Ok(if issued.is_some() {
            OtpIssue::Issued
        } else {
            OtpIssue::CoolingDown
        })
    }

    /// Counts one attempt against the live, unexpired code of `email_hash` and returns it.
    ///
    /// The attempt is counted **before** the comparison, in the same statement that selects the
    /// code, so concurrent guesses cannot exceed `max_attempts`. `None` when there is no usable
    /// code (none, expired, consumed or out of attempts): the caller answers the same 401.
    pub async fn take_otp_attempt(
        pool: &PgPool,
        email_hash: &[u8],
        max_attempts: i32,
    ) -> Result<Option<OtpAttempt>, StoreError> {
        let row = sqlx::query(
            r#"
            UPDATE console_otp_codes
               SET attempts = attempts + 1
             WHERE email_hash = $1
               AND consumed_at IS NULL
               AND expires_at > now()
               AND attempts < $2
            RETURNING id, code_hmac, attempts;
            "#,
        )
        .bind(email_hash)
        .bind(max_attempts)
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;
        Ok(row.map(|row| OtpAttempt {
            id: row.get("id"),
            code_hmac: row.get("code_hmac"),
            attempts: row.get("attempts"),
        }))
    }

    /// Marks a code as used. `false` if somebody else consumed it first: single use.
    pub async fn consume_otp(pool: &PgPool, otp_id: Uuid) -> Result<bool, StoreError> {
        let consumed = sqlx::query(
            r#"
            UPDATE console_otp_codes
               SET consumed_at = now()
             WHERE id = $1 AND consumed_at IS NULL AND expires_at > now()
            RETURNING id;
            "#,
        )
        .bind(otp_id)
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;
        Ok(consumed.is_some())
    }

    /// Spends one e-mail of today's global budget. `false` when `daily_cap` is already reached.
    pub async fn take_email_budget(pool: &PgPool, daily_cap: i32) -> Result<bool, StoreError> {
        let granted = sqlx::query(
            r#"
            INSERT INTO console_email_counters (day, sent)
            VALUES ((now() AT TIME ZONE 'UTC')::date, 1)
            ON CONFLICT (day) DO UPDATE SET sent = console_email_counters.sent + 1
             WHERE console_email_counters.sent < $1
            RETURNING sent;
            "#,
        )
        .bind(daily_cap)
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;
        Ok(granted.is_some())
    }

    // ----- accounts -------------------------------------------------------------------------

    /// Finds the account of a normalized e-mail.
    pub async fn get_account_by_email(
        pool: &PgPool,
        email_normalized: &str,
    ) -> Result<Option<ConsoleAccount>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, email_normalized, status, terms_accepted_at, created_at, updated_at
              FROM console_accounts
             WHERE email_normalized = $1;
            "#,
        )
        .bind(email_normalized)
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;
        row.map(map_account).transpose()
    }

    /// Returns the account of the e-mail, creating it when it does not exist.
    ///
    /// The second value is `true` when this call created it. Creating requires
    /// `accept_terms`; without it a missing account yields `None`. Two concurrent calls for the
    /// same e-mail end with one account and one `created = true`.
    pub async fn find_or_create_account(
        pool: &PgPool,
        email_normalized: &str,
        email_hash: &[u8],
        accept_terms: bool,
    ) -> Result<Option<(ConsoleAccount, bool)>, StoreError> {
        if let Some(account) = Self::get_account_by_email(pool, email_normalized).await? {
            return Ok(Some((account, false)));
        }
        if !accept_terms {
            return Ok(None);
        }
        let inserted = sqlx::query(
            r#"
            INSERT INTO console_accounts (id, email_normalized, email_hash, status, terms_accepted_at)
            VALUES ($1, $2, $3, 'ACTIVE', now())
            ON CONFLICT (email_normalized) DO NOTHING
            RETURNING id, email_normalized, status, terms_accepted_at, created_at, updated_at;
            "#,
        )
        .bind(AccountId::new().into_uuid())
        .bind(email_normalized)
        .bind(email_hash)
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;
        match inserted {
            Some(row) => Ok(Some((map_account(row)?, true))),
            // Lost the race: the other request created it.
            None => Ok(Self::get_account_by_email(pool, email_normalized)
                .await?
                .map(|account| (account, false))),
        }
    }

    /// Deletes an account with its sessions, identities and memberships (docs/10 §7).
    pub async fn delete_account(pool: &PgPool, account_id: AccountId) -> Result<bool, StoreError> {
        let deleted = sqlx::query("DELETE FROM console_accounts WHERE id = $1 RETURNING id;")
            .bind(account_id.into_uuid())
            .fetch_optional(pool)
            .await
            .map_err(StoreError::Database)?;
        Ok(deleted.is_some())
    }

    /// Whether the account is the only `OWNER` of an organization that still has an active
    /// project. Deleting it would leave that organization without anyone able to manage it.
    pub async fn is_sole_owner_of_active_projects(
        pool: &PgPool,
        account_id: AccountId,
    ) -> Result<bool, StoreError> {
        let blocked: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS (
                SELECT 1
                  FROM tenant_memberships me
                 WHERE me.account_id = $1
                   AND me.role = 'OWNER'
                   AND NOT EXISTS (
                       SELECT 1 FROM tenant_memberships other
                        WHERE other.tenant_id = me.tenant_id
                          AND other.role = 'OWNER'
                          AND other.account_id <> me.account_id)
                   AND EXISTS (
                       SELECT 1 FROM applications a
                        WHERE a.tenant_id = me.tenant_id AND a.status = 'ACTIVE')
            );
            "#,
        )
        .bind(account_id.into_uuid())
        .fetch_one(pool)
        .await
        .map_err(StoreError::Database)?;
        Ok(blocked)
    }

    // ----- sessions -------------------------------------------------------------------------

    /// Stores a session by the SHA-256 of its token.
    pub async fn create_session(
        pool: &PgPool,
        account_id: AccountId,
        token_sha256: &[u8],
        ttl_seconds: f64,
    ) -> Result<OffsetDateTime, StoreError> {
        let expires_at: OffsetDateTime = sqlx::query_scalar(
            r#"
            INSERT INTO console_sessions (id, account_id, token_sha256, expires_at)
            VALUES ($1, $2, $3, now() + make_interval(secs => $4))
            RETURNING expires_at;
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(account_id.into_uuid())
        .bind(token_sha256)
        .bind(ttl_seconds)
        .fetch_one(pool)
        .await
        .map_err(StoreError::Database)?;
        Ok(expires_at)
    }

    /// The account of a token, only if the session is live and the account active.
    pub async fn find_active_session(
        pool: &PgPool,
        token_sha256: &[u8],
    ) -> Result<Option<ConsoleAccount>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT a.id, a.email_normalized, a.status, a.terms_accepted_at, a.created_at, a.updated_at
              FROM console_sessions s
              JOIN console_accounts a ON a.id = s.account_id
             WHERE s.token_sha256 = $1
               AND s.revoked_at IS NULL
               AND s.expires_at > now()
               AND a.status = 'ACTIVE';
            "#,
        )
        .bind(token_sha256)
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;
        row.map(map_account).transpose()
    }

    /// Revokes one session. `false` if it did not exist or was already revoked.
    pub async fn revoke_session(pool: &PgPool, token_sha256: &[u8]) -> Result<bool, StoreError> {
        let revoked = sqlx::query(
            r#"
            UPDATE console_sessions SET revoked_at = now()
             WHERE token_sha256 = $1 AND revoked_at IS NULL
            RETURNING id;
            "#,
        )
        .bind(token_sha256)
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;
        Ok(revoked.is_some())
    }

    /// Changes the status of an account; a disabled account loses its sessions at once because
    /// `find_active_session` requires an active account.
    pub async fn set_account_status(
        pool: &PgPool,
        account_id: AccountId,
        status: AccountStatus,
    ) -> Result<bool, StoreError> {
        let updated = sqlx::query(
            "UPDATE console_accounts SET status = $2, updated_at = now() WHERE id = $1 RETURNING id;",
        )
        .bind(account_id.into_uuid())
        .bind(status.as_str())
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;
        Ok(updated.is_some())
    }

    // ----- organizations --------------------------------------------------------------------

    /// How many organizations the account owns.
    pub async fn count_owned_organizations(
        pool: &PgPool,
        account_id: AccountId,
    ) -> Result<i64, StoreError> {
        sqlx::query_scalar(
            "SELECT count(*) FROM tenant_memberships WHERE account_id = $1 AND role = 'OWNER';",
        )
        .bind(account_id.into_uuid())
        .fetch_one(pool)
        .await
        .map_err(StoreError::Database)
    }

    /// Creates a tenant and makes `account_id` its `OWNER`, atomically.
    ///
    /// Refuses (returns `false`) when the account already owns `max_owned` organizations. The
    /// check and the insert share one transaction with a per-account lock, so concurrent
    /// requests cannot pass the limit together.
    pub async fn create_organization(
        pool: &PgPool,
        account_id: AccountId,
        tenant: &Tenant,
        max_owned: i64,
    ) -> Result<bool, StoreError> {
        let mut tx = pool.begin().await.map_err(StoreError::Database)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 0));")
            .bind(account_id.into_uuid().to_string())
            .execute(&mut *tx)
            .await
            .map_err(StoreError::Database)?;
        let owned: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM tenant_memberships WHERE account_id = $1 AND role = 'OWNER';",
        )
        .bind(account_id.into_uuid())
        .fetch_one(&mut *tx)
        .await
        .map_err(StoreError::Database)?;
        if owned >= max_owned {
            return Ok(false);
        }
        sqlx::query(
            r#"
            INSERT INTO tenants (id, name, status, policy, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6);
            "#,
        )
        .bind(tenant.id.into_uuid())
        .bind(&tenant.name)
        .bind(tenant.status.as_str())
        .bind(&tenant.policy)
        .bind(tenant.created_at)
        .bind(tenant.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(StoreError::Database)?;
        sqlx::query(
            "INSERT INTO tenant_memberships (account_id, tenant_id, role) VALUES ($1, $2, 'OWNER');",
        )
        .bind(account_id.into_uuid())
        .bind(tenant.id.into_uuid())
        .execute(&mut *tx)
        .await
        .map_err(StoreError::Database)?;
        tx.commit().await.map_err(StoreError::Database)?;
        Ok(true)
    }

    /// The role of the account in an organization, `None` if it does not belong to it.
    ///
    /// Callers answer 404, not 403, for `None`: an organization of somebody else must not be
    /// distinguishable from one that does not exist (docs/06 §8.6).
    pub async fn get_role(
        pool: &PgPool,
        account_id: AccountId,
        tenant_id: TenantId,
    ) -> Result<Option<ConsoleRole>, StoreError> {
        let role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM tenant_memberships WHERE account_id = $1 AND tenant_id = $2;",
        )
        .bind(account_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;
        role.map(|role| role.parse::<ConsoleRole>().map_err(StoreError::DataMapping))
            .transpose()
    }

    /// Organizations of the account with its role, oldest first.
    pub async fn list_organizations(
        pool: &PgPool,
        account_id: AccountId,
    ) -> Result<Vec<OrganizationMembership>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT t.id, t.name, t.status, t.policy, t.created_at, t.updated_at, m.role
              FROM tenant_memberships m
              JOIN tenants t ON t.id = m.tenant_id
             WHERE m.account_id = $1
             ORDER BY m.created_at, t.id;
            "#,
        )
        .bind(account_id.into_uuid())
        .fetch_all(pool)
        .await
        .map_err(StoreError::Database)?;
        rows.into_iter()
            .map(|row| {
                let role: String = row.get("role");
                let status: String = row.get("status");
                Ok(OrganizationMembership {
                    tenant: Tenant {
                        id: TenantId::from_uuid(row.get("id")),
                        name: row.get("name"),
                        status: status.parse().map_err(|()| {
                            StoreError::DataMapping("unknown tenant status".to_string())
                        })?,
                        policy: row.get("policy"),
                        created_at: row.get("created_at"),
                        updated_at: row.get("updated_at"),
                    },
                    role: role.parse().map_err(StoreError::DataMapping)?,
                })
            })
            .collect()
    }

    // ----- housekeeping ---------------------------------------------------------------------

    /// Deletes what can no longer be used: codes and sessions expired or consumed more than a
    /// day ago and counters older than a week. Returns the number of rows removed.
    pub async fn purge_expired(pool: &PgPool) -> Result<u64, StoreError> {
        let mut removed = 0;
        for statement in [
            "DELETE FROM console_otp_codes WHERE expires_at < now() - interval '1 day' OR consumed_at < now() - interval '1 day';",
            "DELETE FROM console_sessions WHERE expires_at < now() - interval '1 day' OR revoked_at < now() - interval '1 day';",
            "DELETE FROM console_email_counters WHERE day < (now() AT TIME ZONE 'UTC')::date - 7;",
        ] {
            removed += sqlx::query(statement)
                .execute(pool)
                .await
                .map_err(StoreError::Database)?
                .rows_affected();
        }
        Ok(removed)
    }
}

fn map_account(row: sqlx::postgres::PgRow) -> Result<ConsoleAccount, StoreError> {
    let status: String = row.get("status");
    Ok(ConsoleAccount {
        id: AccountId::from_uuid(row.get("id")),
        email: row.get("email_normalized"),
        status: status.parse().map_err(StoreError::DataMapping)?,
        terms_accepted_at: row.get("terms_accepted_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}
