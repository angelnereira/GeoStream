//! The worker jobs against PostgreSQL (FR-SES-005, docs/11 §5): sessions expire on their own and
//! the maintenance purges remove what can no longer be used, leaving the rest alone.
#![forbid(unsafe_code)]

use gs_service::admin::{create_application, create_tenant};
use gs_service::gs_domain::session::{Actor, SessionMode};
use gs_service::migrate::{connect_pool, run_migrations};
use gs_service::sessions::{CreateSessionRequest, SessionPolicy, create_session};
use gs_service::{BusBackend, EphemeralBackend};
use gs_worker::{expire_sessions_once, run_maintenance_once};
use sqlx::Row;
use time::OffsetDateTime;
use uuid::Uuid;

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

async fn pool() -> sqlx::PgPool {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");
    pool
}

fn policy() -> SessionPolicy {
    SessionPolicy {
        default_max_members_one_to_many: 100,
        default_max_members_many_to_many: 50,
        max_members_cap: 1000,
        snapshot_ttl_default_seconds: 30,
        snapshot_ttl_max_seconds: 300,
        stale_after_seconds_default: 30,
        max_session_ttl_seconds: 86400,
    }
}

async fn new_session(pool: &sqlx::PgPool) -> (uuid::Uuid, gs_service::gs_domain::ids::TenantId) {
    let tenant = create_tenant(pool, "Worker Tenant").await.expect("tenant");
    let app = create_application(pool, &tenant.id.to_string(), "worker-app", None, None)
        .await
        .expect("app");
    let session = create_session(
        pool,
        tenant.id,
        Actor::Application(app.id),
        &CreateSessionRequest {
            mode: SessionMode::OneToMany,
            expires_in_seconds: Some(3600),
            expires_at: None,
            max_members: Some(10),
            snapshot_ttl_seconds: Some(30),
            stale_after_seconds: Some(30),
            metadata: None,
            owner_external_subject_id: None,
            initial_members: None,
        },
        policy(),
    )
    .await
    .expect("session");
    (session.id.into_uuid(), tenant.id)
}

async fn status_of(pool: &sqlx::PgPool, id: uuid::Uuid) -> String {
    sqlx::query("SELECT status FROM sessions WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("row")
        .get("status")
}

#[tokio::test]
async fn test_fr_ses_005_a_session_past_its_expiry_becomes_expired_on_its_own() {
    // covers: FR-SES-005, docs/04 §7.5
    let pool = pool().await;
    let (due, _) = new_session(&pool).await;
    let (live, _) = new_session(&pool).await;
    sqlx::query("UPDATE sessions SET expires_at = $1 WHERE id = $2")
        .bind(OffsetDateTime::now_utc() - time::Duration::seconds(60))
        .bind(due)
        .execute(&pool)
        .await
        .expect("backdate");

    let ephemeral = EphemeralBackend::memory();
    let bus = BusBackend::memory();
    let expired = expire_sessions_once(&pool, &ephemeral, &bus, "gs", 500).await;

    assert!(expired >= 1, "at least the backdated session expired");
    assert_eq!(status_of(&pool, due).await, "EXPIRED");
    assert_eq!(status_of(&pool, live).await, "ACTIVE");
    // A second sweep finds nothing new for it.
    expire_sessions_once(&pool, &ephemeral, &bus, "gs", 500).await;
    assert_eq!(status_of(&pool, due).await, "EXPIRED");
}

#[tokio::test]
async fn test_the_maintenance_purge_removes_only_what_can_no_longer_be_used() {
    // covers: docs/11 §5 (idempotency cleanup), FR-CON-011 (no personal data kept past its use)
    let pool = pool().await;
    let (_, tenant) = new_session(&pool).await;

    let old_key = format!("old-{}", Uuid::new_v4().simple());
    let fresh_key = format!("fresh-{}", Uuid::new_v4().simple());
    for (key, expires) in [
        (
            &old_key,
            OffsetDateTime::now_utc() - time::Duration::hours(2),
        ),
        (
            &fresh_key,
            OffsetDateTime::now_utc() + time::Duration::hours(2),
        ),
    ] {
        sqlx::query(
            "INSERT INTO idempotency_keys (tenant_id, principal_id, idempotency_key, request_sha256, state, expires_at) \
             VALUES ($1, $2, $3, $4, 'IN_PROGRESS', $5);",
        )
        .bind(tenant.into_uuid())
        .bind(Uuid::new_v4())
        .bind(key)
        .bind(vec![0u8; 32])
        .bind(expires)
        .execute(&pool)
        .await
        .expect("idempotency row");
    }

    let expired_hash = Uuid::new_v4().as_bytes().repeat(2);
    let live_hash = Uuid::new_v4().as_bytes().repeat(2);
    for (hash, expires) in [
        (
            &expired_hash,
            OffsetDateTime::now_utc() - time::Duration::days(3),
        ),
        (
            &live_hash,
            OffsetDateTime::now_utc() + time::Duration::minutes(10),
        ),
    ] {
        sqlx::query(
            "INSERT INTO console_otp_codes (id, email_hash, code_hmac, expires_at) VALUES ($1, $2, $3, $4);",
        )
        .bind(Uuid::new_v4())
        .bind(hash)
        .bind(vec![1u8; 32])
        .bind(expires)
        .execute(&pool)
        .await
        .expect("otp row");
    }

    let report = run_maintenance_once(&pool).await;
    assert!(report.idempotency_records >= 1);
    assert!(report.console_rows >= 1);

    let remaining = |sql: &'static str, key: Vec<u8>| {
        let pool = pool.clone();
        async move {
            sqlx::query(sql)
                .bind(key)
                .fetch_one(&pool)
                .await
                .expect("count")
                .get::<i64, _>(0)
        }
    };
    assert_eq!(
        remaining(
            "SELECT count(*) FROM console_otp_codes WHERE email_hash = $1",
            expired_hash
        )
        .await,
        0
    );
    assert_eq!(
        remaining(
            "SELECT count(*) FROM console_otp_codes WHERE email_hash = $1",
            live_hash
        )
        .await,
        1
    );
    let count = |key: String| {
        let pool = pool.clone();
        async move {
            sqlx::query("SELECT count(*) FROM idempotency_keys WHERE idempotency_key = $1")
                .bind(key)
                .fetch_one(&pool)
                .await
                .expect("count")
                .get::<i64, _>(0)
        }
    };
    assert_eq!(count(old_key).await, 0);
    assert_eq!(count(fresh_key).await, 1);
}
