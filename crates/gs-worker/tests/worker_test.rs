//! The worker jobs against PostgreSQL (FR-SES-005, docs/11 §5): sessions expire on their own and
//! the maintenance purges remove what can no longer be used, leaving the rest alone.
#![forbid(unsafe_code)]

use gs_service::admin::{create_application, create_tenant};
use gs_service::gs_domain::session::{Actor, SessionMode};
use gs_service::migrate::{connect_pool, run_migrations};
use gs_service::sessions::{CreateSessionRequest, SessionPolicy, create_session};
use gs_service::{BusBackend, EphemeralBackend};
use gs_worker::{WorkerConfig, expire_sessions_once, run_maintenance_once, run_retention_once};
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

/// One recorder per test process: `metrics` allows a single global recorder.
fn metrics_handle() -> &'static metrics_exporter_prometheus::PrometheusHandle {
    static HANDLE: std::sync::OnceLock<metrics_exporter_prometheus::PrometheusHandle> =
        std::sync::OnceLock::new();
    HANDLE.get_or_init(|| {
        metrics_exporter_prometheus::PrometheusBuilder::new()
            .install_recorder()
            .expect("install recorder")
    })
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
    // The sweep is global: another test of this file may expire the session first. The state is what counts.
    expire_sessions_once(&pool, &ephemeral, &bus, "gs", 500).await;

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

    // The purges are global and the tests of this file run in parallel, so another test may remove
    // these rows first: what is asserted is the state of the rows, not who deleted them.
    run_maintenance_once(&pool).await;

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

#[tokio::test]
async fn test_the_jobs_report_duration_rows_and_never_an_identifier() {
    // covers: docs/16 §4.2 (gs_worker_job_*), NFR-OBS-002 (no identifiers in labels)
    let handle = metrics_handle();
    let pool = pool().await;
    let (due, _) = new_session(&pool).await;
    sqlx::query("UPDATE sessions SET expires_at = $1 WHERE id = $2")
        .bind(OffsetDateTime::now_utc() - time::Duration::seconds(60))
        .bind(due)
        .execute(&pool)
        .await
        .expect("backdate");
    expire_sessions_once(
        &pool,
        &EphemeralBackend::memory(),
        &BusBackend::memory(),
        "gs",
        500,
    )
    .await;
    run_maintenance_once(&pool).await;

    let text = handle.render();
    for expected in [
        "gs_worker_job_duration_seconds_count{job=\"session_expiry\"}",
        "gs_worker_job_rows_total{job=\"session_expiry\"}",
        "gs_worker_job_rows_total{job=\"idempotency_purge\"}",
        "gs_worker_job_rows_total{job=\"console_purge\"}",
    ] {
        assert!(text.contains(expected), "missing {expected} in:\n{text}");
    }
    assert!(
        !text.contains(&due.to_string()) && !text.contains("ses_"),
        "a session id reached the metrics"
    );
}

async fn end_session(pool: &sqlx::PgPool, id: uuid::Uuid, status: &str, ended_days_ago: i64) {
    sqlx::query(
        "UPDATE sessions SET status = $1, closed_at = $2, \
         closed_reason = CASE WHEN $1 = 'CLOSED' THEN 'CLOSED_BY_APPLICATION' END WHERE id = $3",
    )
    .bind(status)
    .bind(OffsetDateTime::now_utc() - time::Duration::days(ended_days_ago))
    .bind(id)
    .execute(pool)
    .await
    .expect("end session");
}

async fn add_invitation(
    pool: &sqlx::PgPool,
    tenant: gs_service::gs_domain::ids::TenantId,
    session: uuid::Uuid,
    expired_days_ago: i64,
    revoked: bool,
) -> uuid::Uuid {
    let id = Uuid::new_v4();
    let at = OffsetDateTime::now_utc() - time::Duration::days(expired_days_ago);
    sqlx::query(
        "INSERT INTO invitations (id, tenant_id, session_id, code_sha256, grant_publish, grant_read, \
         grant_invite, grant_manage, max_uses, expires_at, created_by_kind, created_by_id, revoked_at) \
         VALUES ($1, $2, $3, $4, false, true, false, false, 1, $5, 'APPLICATION', $6, $7);",
    )
    .bind(id)
    .bind(tenant.into_uuid())
    .bind(session)
    .bind(Uuid::new_v4().as_bytes().repeat(2))
    .bind(at)
    .bind(Uuid::new_v4())
    .bind(revoked.then_some(at))
    .execute(pool)
    .await
    .expect("invitation");
    id
}

async fn exists(pool: &sqlx::PgPool, table: &'static str, id: uuid::Uuid) -> bool {
    let sql = match table {
        "sessions" => "SELECT count(*) FROM sessions WHERE id = $1",
        _ => "SELECT count(*) FROM invitations WHERE id = $1",
    };
    sqlx::query(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("count")
        .get::<i64, _>(0)
        > 0
}

#[tokio::test]
async fn test_retention_deletes_what_outlived_its_window_in_batches_and_spares_the_rest() {
    // covers: docs/10 §7.1, docs/11 §5 (GS_RETENTION_CLOSED_SESSIONS_D, GS_RETENTION_INVITATIONS_D)
    let pool = pool().await;
    let (old_closed, tenant) = new_session(&pool).await;
    let (old_expired, _) = new_session(&pool).await;
    let (recent_closed, _) = new_session(&pool).await;
    let (old_but_active, _) = new_session(&pool).await;
    end_session(&pool, old_closed, "CLOSED", 100).await;
    end_session(&pool, old_expired, "EXPIRED", 95).await;
    end_session(&pool, recent_closed, "CLOSED", 10).await;
    // An active session is never retention material, however old its row is.
    sqlx::query("UPDATE sessions SET created_at = $1 WHERE id = $2")
        .bind(OffsetDateTime::now_utc() - time::Duration::days(400))
        .bind(old_but_active)
        .execute(&pool)
        .await
        .expect("age");

    let cascades = add_invitation(&pool, tenant, old_closed, 1, false).await;
    let spent = add_invitation(&pool, tenant, old_but_active, 45, false).await;
    let revoked_long_ago = add_invitation(&pool, tenant, old_but_active, 45, true).await;
    let fresh = add_invitation(&pool, tenant, old_but_active, -5, false).await;
    let expired_recently = add_invitation(&pool, tenant, old_but_active, 3, false).await;

    // A batch of one forces several rounds over the two purgeable sessions.
    let config = WorkerConfig {
        batch_size: 1,
        ..WorkerConfig::default()
    };
    let report = run_retention_once(&pool, &config).await;
    assert!(report.ended_sessions >= 2);
    assert!(report.spent_invitations >= 2);

    assert!(!exists(&pool, "sessions", old_closed).await);
    assert!(!exists(&pool, "sessions", old_expired).await);
    assert!(exists(&pool, "sessions", recent_closed).await);
    assert!(exists(&pool, "sessions", old_but_active).await);

    assert!(
        !exists(&pool, "invitations", cascades).await,
        "goes with its session"
    );
    assert!(!exists(&pool, "invitations", spent).await);
    assert!(!exists(&pool, "invitations", revoked_long_ago).await);
    assert!(exists(&pool, "invitations", fresh).await);
    assert!(exists(&pool, "invitations", expired_recently).await);

    // Running it again changes nothing.
    let again = run_retention_once(&pool, &config).await;
    assert!(exists(&pool, "sessions", recent_closed).await);
    assert_eq!(again.ended_sessions + again.spent_invitations, 0);
}
