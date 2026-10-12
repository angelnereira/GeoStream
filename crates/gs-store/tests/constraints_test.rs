//! Integration and constraint tests for migrations/0001_init.sql (docs/11 §2, FR-OPS-004).
#![forbid(unsafe_code)]

use sqlx::PgPool;
use uuid::Uuid;

use gs_store::migrate::{
    check_schema_version, connect_pool, expected_schema_version, plan_migrations, run_migrations,
};

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

async fn setup_db() -> PgPool {
    let url = test_database_url();
    let pool = connect_pool(&url).await.expect("connect to test database");
    run_migrations(&pool).await.expect("apply migrations");
    pool
}

#[tokio::test]
async fn test_schema_version_and_migration_idempotency() {
    let pool = setup_db().await;

    // Schema version check must pass on up-to-date database
    let ver_res = check_schema_version(&pool).await;
    assert!(ver_res.is_ok(), "schema version check failed: {ver_res:?}");

    // FR-OPS-004: running migrations again is strictly idempotent
    let rerun = run_migrations(&pool).await;
    assert!(rerun.is_ok(), "re-running migrations failed: {rerun:?}");

    // Plan migrations reports 0 pending
    let plan = plan_migrations(&pool).await.expect("plan migrations");
    assert_eq!(plan.expected_version, expected_schema_version());
    assert_eq!(plan.current_version, Some(1));
    assert_eq!(plan.pending_count, 0);
}

#[tokio::test]
async fn test_tenants_constraints() {
    let pool = setup_db().await;
    let tenant_id = Uuid::now_v7();

    // Valid tenant insertion succeeds
    sqlx::query(
        "INSERT INTO tenants (id, name, status, policy) VALUES ($1, $2, 'ACTIVE', '{}'::jsonb);",
    )
    .bind(tenant_id)
    .bind("Acme Logistics")
    .execute(&pool)
    .await
    .expect("valid tenant insert");

    // Empty name fails (name length < 1)
    let res_empty =
        sqlx::query("INSERT INTO tenants (id, name, status) VALUES ($1, '', 'ACTIVE');")
            .bind(Uuid::now_v7())
            .execute(&pool)
            .await;
    assert!(res_empty.is_err(), "empty tenant name must be rejected");

    // Name > 100 fails
    let res_too_long =
        sqlx::query("INSERT INTO tenants (id, name, status) VALUES ($1, $2, 'ACTIVE');")
            .bind(Uuid::now_v7())
            .bind("a".repeat(101))
            .execute(&pool)
            .await;
    assert!(
        res_too_long.is_err(),
        "tenant name > 100 chars must be rejected"
    );

    // Invalid status fails
    let res_bad_status =
        sqlx::query("INSERT INTO tenants (id, name, status) VALUES ($1, 'Acme', 'UNKNOWN');")
            .bind(Uuid::now_v7())
            .execute(&pool)
            .await;
    assert!(res_bad_status.is_err(), "invalid status must be rejected");
}

#[tokio::test]
async fn test_applications_constraints() {
    let pool = setup_db().await;
    let tenant_id = Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id, name) VALUES ($1, 'App Tenant');")
        .bind(tenant_id)
        .execute(&pool)
        .await
        .unwrap();

    let app_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO applications (id, tenant_id, name, status) VALUES ($1, $2, 'Tracker App', 'ACTIVE');"
    )
    .bind(app_id)
    .bind(tenant_id)
    .execute(&pool)
    .await
    .unwrap();

    // Foreign key constraint: non-existent tenant fails
    let res_bad_fk = sqlx::query(
        "INSERT INTO applications (id, tenant_id, name) VALUES ($1, $2, 'Bad FK App');",
    )
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .execute(&pool)
    .await;
    assert!(
        res_bad_fk.is_err(),
        "foreign key to non-existent tenant must fail"
    );
}

#[tokio::test]
async fn test_api_keys_constraints() {
    let pool = setup_db().await;
    let tenant_id = Uuid::now_v7();
    let app_id = Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id, name) VALUES ($1, 'Key Tenant');")
        .bind(tenant_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO applications (id, tenant_id, name) VALUES ($1, $2, 'Key App');")
        .bind(app_id)
        .bind(tenant_id)
        .execute(&pool)
        .await
        .unwrap();

    let valid_sha = [0xabu8; 32];

    // Valid active api key succeeds
    sqlx::query(
        "INSERT INTO api_keys (id, tenant_id, application_id, secret_sha256, scopes, status) VALUES ($1, $2, $3, $4, ARRAY['sessions:create'], 'ACTIVE');"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(app_id)
    .bind(&valid_sha[..])
    .execute(&pool)
    .await
    .unwrap();

    // secret_sha256 != 32 bytes fails
    let res_bad_sha = sqlx::query(
        "INSERT INTO api_keys (id, tenant_id, application_id, secret_sha256, scopes, status) VALUES ($1, $2, $3, $4, ARRAY['sessions:create'], 'ACTIVE');"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(app_id)
    .bind(&[0xabu8; 16][..])
    .execute(&pool)
    .await;
    assert!(
        res_bad_sha.is_err(),
        "secret_sha256 must be exactly 32 bytes"
    );

    // REVOKED requires revoked_at IS NOT NULL
    let res_revoked_no_ts = sqlx::query(
        "INSERT INTO api_keys (id, tenant_id, application_id, secret_sha256, scopes, status, revoked_at) VALUES ($1, $2, $3, $4, ARRAY['sessions:create'], 'REVOKED', NULL);"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(app_id)
    .bind(&valid_sha[..])
    .execute(&pool)
    .await;
    assert!(
        res_revoked_no_ts.is_err(),
        "REVOKED status without revoked_at must fail"
    );

    // ACTIVE with revoked_at IS NOT NULL fails
    let res_active_with_ts = sqlx::query(
        "INSERT INTO api_keys (id, tenant_id, application_id, secret_sha256, scopes, status, revoked_at) VALUES ($1, $2, $3, $4, ARRAY['sessions:create'], 'ACTIVE', now());"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(app_id)
    .bind(&valid_sha[..])
    .execute(&pool)
    .await;
    assert!(
        res_active_with_ts.is_err(),
        "ACTIVE status with revoked_at must fail"
    );
}

#[tokio::test]
async fn test_subjects_constraints() {
    let pool = setup_db().await;
    let tenant_id = Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id, name) VALUES ($1, 'Subject Tenant');")
        .bind(tenant_id)
        .execute(&pool)
        .await
        .unwrap();

    let sub_id1 = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO subjects (id, tenant_id, external_subject_id, status) VALUES ($1, $2, 'driver.123_abc', 'ACTIVE');"
    )
    .bind(sub_id1)
    .bind(tenant_id)
    .execute(&pool)
    .await
    .unwrap();

    // Invalid regex in external_subject_id (e.g. spaces or '@') fails
    let res_bad_chars = sqlx::query(
        "INSERT INTO subjects (id, tenant_id, external_subject_id, status) VALUES ($1, $2, 'user@domain.com', 'ACTIVE');"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .execute(&pool)
    .await;
    assert!(
        res_bad_chars.is_err(),
        "external_subject_id containing '@' must be rejected"
    );

    // Duplicate active external_subject_id for same tenant fails
    let res_dup = sqlx::query(
        "INSERT INTO subjects (id, tenant_id, external_subject_id, status) VALUES ($1, $2, 'driver.123_abc', 'ACTIVE');"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .execute(&pool)
    .await;
    assert!(
        res_dup.is_err(),
        "duplicate active external_subject_id for same tenant must fail"
    );

    // ERASED requires external_subject_id IS NULL and erased_at IS NOT NULL
    let res_erased_ok = sqlx::query(
        "INSERT INTO subjects (id, tenant_id, external_subject_id, status, erased_at) VALUES ($1, $2, NULL, 'ERASED', now());"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .execute(&pool)
    .await;
    assert!(res_erased_ok.is_ok(), "valid erased subject must succeed");

    let res_erased_with_ext = sqlx::query(
        "INSERT INTO subjects (id, tenant_id, external_subject_id, status, erased_at) VALUES ($1, $2, 'driver.999', 'ERASED', now());"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .execute(&pool)
    .await;
    assert!(
        res_erased_with_ext.is_err(),
        "ERASED status with non-null external_subject_id must fail"
    );
}

#[tokio::test]
async fn test_devices_constraints() {
    let pool = setup_db().await;
    let tenant_id = Uuid::now_v7();
    let app_id = Uuid::now_v7();
    let sub_id = Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id, name) VALUES ($1, 'Dev Tenant');")
        .bind(tenant_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO applications (id, tenant_id, name) VALUES ($1, $2, 'Dev App');")
        .bind(app_id)
        .bind(tenant_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO subjects (id, tenant_id, external_subject_id, status) VALUES ($1, $2, 'sub.dev.1', 'ACTIVE');")
        .bind(sub_id)
        .bind(tenant_id)
        .execute(&pool)
        .await
        .unwrap();

    let installation_id = Uuid::now_v7();
    let dev_id = Uuid::now_v7();

    // Valid device insertion
    sqlx::query(
        "INSERT INTO devices (id, tenant_id, subject_id, application_id, installation_id, platform, status) VALUES ($1, $2, $3, $4, $5, 'ANDROID', 'ACTIVE');"
    )
    .bind(dev_id)
    .bind(tenant_id)
    .bind(sub_id)
    .bind(app_id)
    .bind(installation_id)
    .execute(&pool)
    .await
    .unwrap();

    // Invalid platform fails
    let res_bad_plat = sqlx::query(
        "INSERT INTO devices (id, tenant_id, subject_id, application_id, installation_id, platform, status) VALUES ($1, $2, $3, $4, $5, 'WINDOWS', 'ACTIVE');"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(sub_id)
    .bind(app_id)
    .bind(Uuid::now_v7())
    .execute(&pool)
    .await;
    assert!(res_bad_plat.is_err(), "invalid platform must be rejected");

    // Duplicate active device identity fails
    let res_dup_dev = sqlx::query(
        "INSERT INTO devices (id, tenant_id, subject_id, application_id, installation_id, platform, status) VALUES ($1, $2, $3, $4, $5, 'IOS', 'ACTIVE');"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(sub_id)
    .bind(app_id)
    .bind(installation_id)
    .execute(&pool)
    .await;
    assert!(
        res_dup_dev.is_err(),
        "duplicate active device identity must fail"
    );

    // INACTIVE requires deactivated_at IS NOT NULL
    let res_inact_no_ts = sqlx::query(
        "INSERT INTO devices (id, tenant_id, subject_id, application_id, installation_id, platform, status, deactivated_at) VALUES ($1, $2, $3, $4, $5, 'WEB', 'INACTIVE', NULL);"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(sub_id)
    .bind(app_id)
    .bind(Uuid::now_v7())
    .execute(&pool)
    .await;
    assert!(
        res_inact_no_ts.is_err(),
        "INACTIVE status without deactivated_at must fail"
    );
}

#[tokio::test]
async fn test_sessions_constraints() {
    let pool = setup_db().await;
    let tenant_id = Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id, name) VALUES ($1, 'Session Tenant');")
        .bind(tenant_id)
        .execute(&pool)
        .await
        .unwrap();

    let ses_id = Uuid::now_v7();
    let now = time::OffsetDateTime::now_utc();
    let exp = now + time::Duration::seconds(3600);

    // ONE_TO_ONE mode with max_members != 2 fails
    let res_o2o_bad = sqlx::query(
        "INSERT INTO sessions (id, tenant_id, mode, status, created_by_kind, created_by_id, expires_at, max_members, snapshot_ttl_seconds, stale_after_seconds) VALUES ($1, $2, 'ONE_TO_ONE', 'ACTIVE', 'APPLICATION', $3, $4, 5, 600, 60);"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(Uuid::now_v7())
    .bind(exp)
    .execute(&pool)
    .await;
    assert!(
        res_o2o_bad.is_err(),
        "ONE_TO_ONE mode with max_members != 2 must fail"
    );

    // Valid ONE_TO_ONE session succeeds
    sqlx::query(
        "INSERT INTO sessions (id, tenant_id, mode, status, created_by_kind, created_by_id, expires_at, max_members, snapshot_ttl_seconds, stale_after_seconds) VALUES ($1, $2, 'ONE_TO_ONE', 'ACTIVE', 'APPLICATION', $3, $4, 2, 600, 60);"
    )
    .bind(ses_id)
    .bind(tenant_id)
    .bind(Uuid::now_v7())
    .bind(exp)
    .execute(&pool)
    .await
    .unwrap();

    // CLOSED status requires closed_reason
    let res_closed_no_reason = sqlx::query(
        "INSERT INTO sessions (id, tenant_id, mode, status, created_by_kind, created_by_id, expires_at, max_members, snapshot_ttl_seconds, stale_after_seconds, closed_at, closed_reason) VALUES ($1, $2, 'ONE_TO_MANY', 'CLOSED', 'APPLICATION', $3, $4, 10, 600, 60, now(), NULL);"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(Uuid::now_v7())
    .bind(exp)
    .execute(&pool)
    .await;
    assert!(
        res_closed_no_reason.is_err(),
        "CLOSED status without closed_reason must fail"
    );

    // stale_after_seconds < 5 fails
    let res_bad_stale = sqlx::query(
        "INSERT INTO sessions (id, tenant_id, mode, status, created_by_kind, created_by_id, expires_at, max_members, snapshot_ttl_seconds, stale_after_seconds) VALUES ($1, $2, 'ONE_TO_MANY', 'ACTIVE', 'APPLICATION', $3, $4, 10, 600, 2);"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(Uuid::now_v7())
    .bind(exp)
    .execute(&pool)
    .await;
    assert!(res_bad_stale.is_err(), "stale_after_seconds < 5 must fail");
}

#[tokio::test]
async fn test_memberships_constraints() {
    let pool = setup_db().await;
    let tenant_id = Uuid::now_v7();
    let sub_id1 = Uuid::now_v7();
    let sub_id2 = Uuid::now_v7();
    let ses_id = Uuid::now_v7();
    let dev_id = Uuid::now_v7();
    let app_id = Uuid::now_v7();

    sqlx::query("INSERT INTO tenants (id, name) VALUES ($1, 'Mem Tenant');")
        .bind(tenant_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO applications (id, tenant_id, name) VALUES ($1, $2, 'Mem App');")
        .bind(app_id)
        .bind(tenant_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO subjects (id, tenant_id, external_subject_id, status) VALUES ($1, $2, 'sub.mem.1', 'ACTIVE'), ($3, $2, 'sub.mem.2', 'ACTIVE');")
        .bind(sub_id1)
        .bind(tenant_id)
        .bind(sub_id2)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO devices (id, tenant_id, subject_id, application_id, installation_id, platform) VALUES ($1, $2, $3, $4, $5, 'ANDROID');")
        .bind(dev_id)
        .bind(tenant_id)
        .bind(sub_id1)
        .bind(app_id)
        .bind(Uuid::now_v7())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO sessions (id, tenant_id, mode, created_by_kind, created_by_id, expires_at, max_members, snapshot_ttl_seconds, stale_after_seconds) VALUES ($1, $2, 'ONE_TO_MANY', 'APPLICATION', $3, now() + interval '1 hour', 10, 600, 60);")
        .bind(ses_id)
        .bind(tenant_id)
        .bind(Uuid::now_v7())
        .execute(&pool)
        .await
        .unwrap();

    let owner_mem_id = Uuid::now_v7();
    // Valid owner membership
    sqlx::query(
        "INSERT INTO memberships (id, tenant_id, session_id, subject_id, role, can_publish, can_read, can_invite, can_manage, status, joined_via) VALUES ($1, $2, $3, $4, 'OWNER', true, true, true, true, 'ACTIVE', 'CREATOR');"
    )
    .bind(owner_mem_id)
    .bind(tenant_id)
    .bind(ses_id)
    .bind(sub_id1)
    .execute(&pool)
    .await
    .unwrap();

    // Owner role with missing capability fails (INV-MEM-1)
    let res_owner_missing_cap = sqlx::query(
        "INSERT INTO memberships (id, tenant_id, session_id, subject_id, role, can_publish, can_read, can_invite, can_manage, status, joined_via) VALUES ($1, $2, $3, $4, 'OWNER', false, true, true, true, 'ACTIVE', 'CREATOR');"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(ses_id)
    .bind(sub_id2)
    .execute(&pool)
    .await;
    assert!(
        res_owner_missing_cap.is_err(),
        "OWNER role with missing publish must fail"
    );

    // Single active owner index fails on second active owner
    let res_second_owner = sqlx::query(
        "INSERT INTO memberships (id, tenant_id, session_id, subject_id, role, can_publish, can_read, can_invite, can_manage, status, joined_via) VALUES ($1, $2, $3, $4, 'OWNER', true, true, true, true, 'ACTIVE', 'DIRECT_ADD');"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(ses_id)
    .bind(sub_id2)
    .execute(&pool)
    .await;
    assert!(
        res_second_owner.is_err(),
        "second active OWNER in same session must fail"
    );

    // sharing = true but publisher_device_id IS NULL fails
    let res_sharing_no_dev = sqlx::query(
        "INSERT INTO memberships (id, tenant_id, session_id, subject_id, role, can_publish, can_read, can_invite, can_manage, status, sharing, publisher_device_id, joined_via) VALUES ($1, $2, $3, $4, 'MEMBER', true, true, false, false, 'ACTIVE', true, NULL, 'DIRECT_ADD');"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(ses_id)
    .bind(sub_id2)
    .execute(&pool)
    .await;
    assert!(
        res_sharing_no_dev.is_err(),
        "sharing=true without publisher_device_id must fail"
    );

    // joined_via = 'INVITATION' but invitation_id IS NULL fails
    let res_inv_no_id = sqlx::query(
        "INSERT INTO memberships (id, tenant_id, session_id, subject_id, role, can_publish, can_read, can_invite, can_manage, status, joined_via, invitation_id) VALUES ($1, $2, $3, $4, 'MEMBER', true, true, false, false, 'ACTIVE', 'INVITATION', NULL);"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(ses_id)
    .bind(sub_id2)
    .execute(&pool)
    .await;
    assert!(
        res_inv_no_id.is_err(),
        "joined_via=INVITATION without invitation_id must fail"
    );
}

#[tokio::test]
async fn test_invitations_constraints() {
    let pool = setup_db().await;
    let tenant_id = Uuid::now_v7();
    let ses_id = Uuid::now_v7();

    sqlx::query("INSERT INTO tenants (id, name) VALUES ($1, 'Inv Tenant');")
        .bind(tenant_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO sessions (id, tenant_id, mode, created_by_kind, created_by_id, expires_at, max_members, snapshot_ttl_seconds, stale_after_seconds) VALUES ($1, $2, 'ONE_TO_MANY', 'APPLICATION', $3, now() + interval '1 hour', 10, 600, 60);")
        .bind(ses_id)
        .bind(tenant_id)
        .bind(Uuid::now_v7())
        .execute(&pool)
        .await
        .unwrap();

    let mut code_sha = [0u8; 32];
    code_sha[0..16].copy_from_slice(Uuid::now_v7().as_bytes());
    code_sha[16..32].copy_from_slice(Uuid::now_v7().as_bytes());
    // Valid invitation insertion
    sqlx::query(
        "INSERT INTO invitations (id, tenant_id, session_id, code_sha256, grant_publish, grant_read, grant_invite, grant_manage, max_uses, uses, expires_at, created_by_kind, created_by_id) VALUES ($1, $2, $3, $4, true, true, false, false, 5, 0, now() + interval '1 day', 'APPLICATION', $5);"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(ses_id)
    .bind(&code_sha[..])
    .bind(Uuid::now_v7())
    .execute(&pool)
    .await
    .unwrap();

    // Duplicate code_sha256 fails
    let res_dup_code = sqlx::query(
        "INSERT INTO invitations (id, tenant_id, session_id, code_sha256, grant_publish, grant_read, grant_invite, grant_manage, max_uses, uses, expires_at, created_by_kind, created_by_id) VALUES ($1, $2, $3, $4, true, true, false, false, 5, 0, now() + interval '1 day', 'APPLICATION', $5);"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(ses_id)
    .bind(&code_sha[..])
    .bind(Uuid::now_v7())
    .execute(&pool)
    .await;
    assert!(
        res_dup_code.is_err(),
        "duplicate invitation code_sha256 must fail"
    );

    // uses > max_uses fails
    let res_uses_exceeded = sqlx::query(
        "INSERT INTO invitations (id, tenant_id, session_id, code_sha256, grant_publish, grant_read, grant_invite, grant_manage, max_uses, uses, expires_at, created_by_kind, created_by_id) VALUES ($1, $2, $3, $4, true, true, false, false, 5, 6, now() + interval '1 day', 'APPLICATION', $5);"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(ses_id)
    .bind(&[0x88u8; 32][..])
    .bind(Uuid::now_v7())
    .execute(&pool)
    .await;
    assert!(res_uses_exceeded.is_err(), "uses > max_uses must fail");
}

#[tokio::test]
async fn test_idempotency_keys_constraints() {
    let pool = setup_db().await;
    let tenant_id = Uuid::now_v7();
    let principal_id = Uuid::now_v7();
    let req_sha = [0x55u8; 32];

    // IN_PROGRESS with response_status NULL succeeds
    sqlx::query(
        "INSERT INTO idempotency_keys (tenant_id, principal_id, idempotency_key, request_sha256, state, response_status, expires_at) VALUES ($1, $2, 'key-123', $3, 'IN_PROGRESS', NULL, now() + interval '1 hour');"
    )
    .bind(tenant_id)
    .bind(principal_id)
    .bind(&req_sha[..])
    .execute(&pool)
    .await
    .unwrap();

    // COMPLETED with response_status NULL fails
    let res_completed_no_status = sqlx::query(
        "INSERT INTO idempotency_keys (tenant_id, principal_id, idempotency_key, request_sha256, state, response_status, expires_at) VALUES ($1, $2, 'key-456', $3, 'COMPLETED', NULL, now() + interval '1 hour');"
    )
    .bind(tenant_id)
    .bind(principal_id)
    .bind(&req_sha[..])
    .execute(&pool)
    .await;
    assert!(
        res_completed_no_status.is_err(),
        "COMPLETED state without response_status must fail"
    );
}

#[tokio::test]
async fn test_webhook_endpoints_and_deliveries_constraints() {
    let pool = setup_db().await;
    let tenant_id = Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id, name) VALUES ($1, 'Whk Tenant');")
        .bind(tenant_id)
        .execute(&pool)
        .await
        .unwrap();

    let endpoint_id = Uuid::now_v7();
    let secret = [0x11u8; 32];

    // Empty event_types fails (cardinality >= 1)
    let res_empty_events = sqlx::query(
        "INSERT INTO webhook_endpoints (id, tenant_id, url, event_types, secret_enc, status) VALUES ($1, $2, 'https://example.com/wh', ARRAY[]::text[], $3, 'ACTIVE');"
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(&secret[..])
    .execute(&pool)
    .await;
    assert!(
        res_empty_events.is_err(),
        "empty event_types array must fail"
    );

    // Valid webhook endpoint
    sqlx::query(
        "INSERT INTO webhook_endpoints (id, tenant_id, url, event_types, secret_enc, status) VALUES ($1, $2, 'https://example.com/wh', ARRAY['session.created'], $3, 'ACTIVE');"
    )
    .bind(endpoint_id)
    .bind(tenant_id)
    .bind(&secret[..])
    .execute(&pool)
    .await
    .unwrap();

    let event_id = Uuid::now_v7();
    let delivery_id = Uuid::now_v7();

    // Valid delivery insert
    sqlx::query(
        "INSERT INTO webhook_deliveries (id, event_id, tenant_id, endpoint_id, event_type, payload, status) VALUES ($1, $2, $3, $4, 'session.created', '{}'::jsonb, 'PENDING');"
    )
    .bind(delivery_id)
    .bind(event_id)
    .bind(tenant_id)
    .bind(endpoint_id)
    .execute(&pool)
    .await
    .unwrap();

    // Duplicate (event_id, endpoint_id) fails
    let res_dup_deliv = sqlx::query(
        "INSERT INTO webhook_deliveries (id, event_id, tenant_id, endpoint_id, event_type, payload, status) VALUES ($1, $2, $3, $4, 'session.created', '{}'::jsonb, 'PENDING');"
    )
    .bind(Uuid::now_v7())
    .bind(event_id)
    .bind(tenant_id)
    .bind(endpoint_id)
    .execute(&pool)
    .await;
    assert!(
        res_dup_deliv.is_err(),
        "duplicate (event_id, endpoint_id) delivery must fail"
    );
}
