//! Integration tests for gs-store repositories against PostgreSQL (T-F2-02, INV-TEN-1, INV-DEV-1/2, INV-SUB-1).
#![forbid(unsafe_code)]

use serde_json::json;
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};

use gs_domain::ids::{ApiKeyId, ApplicationId, AuditId, DeviceId, InstallationId, TenantId};
use gs_store::{
    ActorKind, ApiKey, ApiKeyRepository, ApiKeyStatus, Application, ApplicationRepository,
    ApplicationStatus, AuditEvent, AuditOutcome, AuditRepository, DevicePlatform, DeviceRepository,
    DeviceStatus, StoreError, Subject, SubjectRepository, SubjectStatus, Tenant, TenantRepository,
    TenantStatus, UpsertDeviceParams, connect_pool, run_migrations,
};

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

async fn setup_db() -> PgPool {
    let pool = connect_pool(&test_database_url())
        .await
        .expect("connect to test database");
    run_migrations(&pool).await.expect("apply migrations");
    pool
}

fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

async fn new_tenant(pool: &PgPool, name: &str) -> Tenant {
    let t = Tenant {
        id: TenantId::new(),
        name: name.to_string(),
        status: TenantStatus::Active,
        policy: json!({}),
        created_at: now(),
        updated_at: now(),
    };
    TenantRepository::create(pool, &t)
        .await
        .expect("create tenant");
    t
}

async fn new_app(pool: &PgPool, tenant_id: TenantId, name: &str) -> Application {
    let a = Application {
        id: ApplicationId::new(),
        tenant_id,
        name: name.to_string(),
        status: ApplicationStatus::Active,
        allowed_device_scopes: vec!["sessions:create".into(), "stream:connect".into()],
        allowed_origins: vec![],
        created_at: now(),
        updated_at: now(),
    };
    ApplicationRepository::create(pool, &a)
        .await
        .expect("create app");
    a
}

async fn new_subject(pool: &PgPool, tenant_id: TenantId, ext: &str) -> Subject {
    SubjectRepository::get_or_create_active(pool, tenant_id, ext, now())
        .await
        .expect("create subject")
}

#[tokio::test]
async fn test_tenant_crud_and_status() {
    // covers: FR-AUTH-001 (tenant persistence)
    let pool = setup_db().await;
    let t = new_tenant(&pool, "Acme").await;

    let got = TenantRepository::get_by_id(&pool, t.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.name, "Acme");
    assert_eq!(got.status, TenantStatus::Active);

    assert!(
        TenantRepository::update_status(&pool, t.id, TenantStatus::Disabled, now())
            .await
            .unwrap()
    );
    let got = TenantRepository::get_by_id(&pool, t.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.status, TenantStatus::Disabled);

    let policy = json!({"max_devices_per_subject": 3});
    assert!(
        TenantRepository::update_policy(&pool, t.id, &policy, now())
            .await
            .unwrap()
    );
    let got = TenantRepository::get_by_id(&pool, t.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.policy, policy);

    assert!(
        TenantRepository::list(&pool)
            .await
            .unwrap()
            .iter()
            .any(|x| x.id == t.id)
    );
    assert!(
        TenantRepository::get_by_id(&pool, TenantId::new())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn test_inv_ten_1_application_tenant_isolation_and_crud() {
    // covers: INV-TEN-1, FR-AUTH-001 (application persistence)
    let pool = setup_db().await;
    let t1 = new_tenant(&pool, "T1").await;
    let t2 = new_tenant(&pool, "T2").await;
    let app = new_app(&pool, t1.id, "app-one").await;

    let got = ApplicationRepository::get_by_id(&pool, t1.id, app.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.name, "app-one");
    assert_eq!(got.allowed_device_scopes, app.allowed_device_scopes);

    // INV-TEN-1: another tenant cannot see or modify it.
    assert!(
        ApplicationRepository::get_by_id(&pool, t2.id, app.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !ApplicationRepository::update_status(
            &pool,
            t2.id,
            app.id,
            ApplicationStatus::Disabled,
            now()
        )
        .await
        .unwrap()
    );
    assert!(
        ApplicationRepository::list_by_tenant(&pool, t2.id)
            .await
            .unwrap()
            .is_empty()
    );

    assert!(
        ApplicationRepository::update_status(
            &pool,
            t1.id,
            app.id,
            ApplicationStatus::Disabled,
            now()
        )
        .await
        .unwrap()
    );
    let got = ApplicationRepository::get_by_id(&pool, t1.id, app.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.status, ApplicationStatus::Disabled);

    assert!(
        ApplicationRepository::update_scopes_and_origins(
            &pool,
            t1.id,
            app.id,
            &["stream:connect".to_string()],
            &["https://example.com".to_string()],
            now()
        )
        .await
        .unwrap()
    );
    let got = ApplicationRepository::get_by_id(&pool, t1.id, app.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        got.allowed_device_scopes,
        vec!["stream:connect".to_string()]
    );
    assert_eq!(got.allowed_origins, vec!["https://example.com".to_string()]);
    assert_eq!(
        ApplicationRepository::list_by_tenant(&pool, t1.id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn test_fr_auth_001_api_key_lifecycle_and_tenant_isolation() {
    // covers: FR-AUTH-001
    let pool = setup_db().await;
    let t1 = new_tenant(&pool, "K1").await;
    let t2 = new_tenant(&pool, "K2").await;
    let app = new_app(&pool, t1.id, "keys-app").await;

    let key = ApiKey {
        id: ApiKeyId::new(),
        tenant_id: t1.id,
        application_id: app.id,
        secret_sha256: [7u8; 32],
        scopes: vec!["tokens:mint".into()],
        label: Some("ci".into()),
        status: ApiKeyStatus::Active,
        expires_at: Some(now() + Duration::days(1)),
        last_used_at: None,
        created_at: now(),
        revoked_at: None,
    };
    ApiKeyRepository::create(&pool, &key).await.unwrap();

    let got = ApiKeyRepository::get_by_id(&pool, t1.id, key.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.secret_sha256, [7u8; 32]);
    assert_eq!(got.scopes, key.scopes);
    assert_eq!(got.label.as_deref(), Some("ci"));
    // Debug never leaks the hash.
    assert!(format!("{got:?}").contains("<redacted>"));

    assert!(
        ApiKeyRepository::get_by_id(&pool, t2.id, key.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        ApiKeyRepository::list_by_application(&pool, t2.id, app.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        ApiKeyRepository::list_by_application(&pool, t1.id, app.id)
            .await
            .unwrap()
            .len(),
        1
    );

    let auth = ApiKeyRepository::find_by_id(&pool, key.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(auth.tenant_id, t1.id);

    assert!(
        ApiKeyRepository::record_usage(&pool, key.id, now())
            .await
            .unwrap()
    );
    let got = ApiKeyRepository::get_by_id(&pool, t1.id, key.id)
        .await
        .unwrap()
        .unwrap();
    assert!(got.last_used_at.is_some());

    // Wrong tenant cannot revoke.
    assert!(
        !ApiKeyRepository::revoke(&pool, t2.id, key.id, now())
            .await
            .unwrap()
    );
    assert!(
        ApiKeyRepository::revoke(&pool, t1.id, key.id, now())
            .await
            .unwrap()
    );
    // Revocation is terminal and idempotent-false the second time.
    assert!(
        !ApiKeyRepository::revoke(&pool, t1.id, key.id, now())
            .await
            .unwrap()
    );
    let got = ApiKeyRepository::get_by_id(&pool, t1.id, key.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.status, ApiKeyStatus::Revoked);
    assert!(got.revoked_at.is_some());
}

#[tokio::test]
async fn test_inv_sub_1_subject_upsert_epoch_and_erase() {
    // covers: INV-SUB-1, FR-AUTH-001 (subject persistence)
    let pool = setup_db().await;
    let t1 = new_tenant(&pool, "S1").await;
    let t2 = new_tenant(&pool, "S2").await;

    let s = new_subject(&pool, t1.id, "user-1").await;
    assert_eq!(s.token_epoch, 0);
    assert_eq!(s.status, SubjectStatus::Active);

    // INV-SUB-1: same external id in same tenant returns the same subject.
    let again = new_subject(&pool, t1.id, "user-1").await;
    assert_eq!(again.id, s.id);
    // Same external id in another tenant is a different subject.
    let other = new_subject(&pool, t2.id, "user-1").await;
    assert_ne!(other.id, s.id);

    assert!(
        SubjectRepository::get_by_id(&pool, t2.id, s.id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        SubjectRepository::find_active_by_external_id(&pool, t1.id, "user-1")
            .await
            .unwrap()
            .unwrap()
            .id,
        s.id
    );
    assert!(
        SubjectRepository::find_active_by_external_id(&pool, t2.id, "nobody")
            .await
            .unwrap()
            .is_none()
    );

    assert_eq!(
        SubjectRepository::increment_token_epoch(&pool, t1.id, s.id, now())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        SubjectRepository::increment_token_epoch(&pool, t1.id, s.id, now())
            .await
            .unwrap(),
        2
    );
    // Cross-tenant epoch bump is rejected.
    assert!(matches!(
        SubjectRepository::increment_token_epoch(&pool, t2.id, s.id, now()).await,
        Err(StoreError::NotFound { .. })
    ));

    assert!(
        !SubjectRepository::erase(&pool, t2.id, s.id, now())
            .await
            .unwrap()
    );
    assert!(
        SubjectRepository::erase(&pool, t1.id, s.id, now())
            .await
            .unwrap()
    );
    let erased = SubjectRepository::get_by_id(&pool, t1.id, s.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(erased.status, SubjectStatus::Erased);
    assert!(erased.external_subject_id.is_none());
    assert!(erased.erased_at.is_some());

    // A new token with the same external id creates a new subject.
    let reborn = new_subject(&pool, t1.id, "user-1").await;
    assert_ne!(reborn.id, s.id);
    assert_eq!(reborn.token_epoch, 0);

    assert!(
        !SubjectRepository::list_by_tenant(&pool, t1.id, 10)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn test_inv_dev_1_inv_dev_2_device_upsert_identity_and_max_devices() {
    // covers: INV-DEV-1, INV-DEV-2, FR-AUTH-001 (device persistence)
    let pool = setup_db().await;
    let t1 = new_tenant(&pool, "D1").await;
    let t2 = new_tenant(&pool, "D2").await;
    let app = new_app(&pool, t1.id, "dev-app").await;
    let subj = new_subject(&pool, t1.id, "driver-1").await;

    let inst = InstallationId::new();
    let d1 = DeviceRepository::upsert_device(
        &pool,
        UpsertDeviceParams {
            tenant_id: t1.id,
            subject_id: subj.id,
            application_id: app.id,
            installation_id: inst,
            platform: DevicePlatform::Android,
            app_version: Some("1.0.0".into()),
            sdk_version: None,
            max_devices_per_subject: 2,
            now: now(),
        },
    )
    .await
    .unwrap();

    // INV-DEV-1: same identity tuple updates the same device.
    let d1b = DeviceRepository::upsert_device(
        &pool,
        UpsertDeviceParams {
            tenant_id: t1.id,
            subject_id: subj.id,
            application_id: app.id,
            installation_id: inst,
            platform: DevicePlatform::Ios,
            app_version: None,
            sdk_version: Some("0.2.0".into()),
            max_devices_per_subject: 2,
            now: now(),
        },
    )
    .await
    .unwrap();
    assert_eq!(d1b.id, d1.id);
    assert_eq!(d1b.platform, DevicePlatform::Ios);
    assert_eq!(d1b.app_version.as_deref(), Some("1.0.0"));
    assert_eq!(d1b.sdk_version.as_deref(), Some("0.2.0"));

    assert!(
        DeviceRepository::get_by_id(&pool, t2.id, d1.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        DeviceRepository::get_by_id(&pool, t1.id, d1.id)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        DeviceRepository::find_active_by_identity(&pool, app.id, inst, subj.id)
            .await
            .unwrap()
            .is_some()
    );

    // INV-DEV-2: with max=2, the third device deactivates the oldest by last_seen_at.
    let base = now();
    DeviceRepository::update_last_seen(&pool, t1.id, d1.id, None, None, base - Duration::hours(2))
        .await
        .unwrap();
    let d2 = DeviceRepository::upsert_device(
        &pool,
        UpsertDeviceParams {
            tenant_id: t1.id,
            subject_id: subj.id,
            application_id: app.id,
            installation_id: InstallationId::new(),
            platform: DevicePlatform::Web,
            app_version: None,
            sdk_version: None,
            max_devices_per_subject: 2,
            now: base - Duration::hours(1),
        },
    )
    .await
    .unwrap();
    let d3 = DeviceRepository::upsert_device(
        &pool,
        UpsertDeviceParams {
            tenant_id: t1.id,
            subject_id: subj.id,
            application_id: app.id,
            installation_id: InstallationId::new(),
            platform: DevicePlatform::Web,
            app_version: None,
            sdk_version: None,
            max_devices_per_subject: 2,
            now: base,
        },
    )
    .await
    .unwrap();

    let active = DeviceRepository::list_active_by_subject(&pool, t1.id, subj.id)
        .await
        .unwrap();
    let ids: Vec<DeviceId> = active.iter().map(|d| d.id).collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&d2.id) && ids.contains(&d3.id));
    let oldest = DeviceRepository::get_by_id(&pool, t1.id, d1.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(oldest.status, DeviceStatus::Inactive);
    assert!(oldest.deactivated_at.is_some());

    // Cross-tenant list is empty; cross-tenant deactivate is a no-op.
    assert!(
        DeviceRepository::list_active_by_subject(&pool, t2.id, subj.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !DeviceRepository::deactivate(&pool, t2.id, d3.id, now())
            .await
            .unwrap()
    );
    assert!(
        DeviceRepository::deactivate(&pool, t1.id, d3.id, now())
            .await
            .unwrap()
    );
    // INACTIVE is terminal.
    assert!(
        !DeviceRepository::deactivate(&pool, t1.id, d3.id, now())
            .await
            .unwrap()
    );
    assert!(
        !DeviceRepository::update_last_seen(&pool, t1.id, d3.id, None, None, now())
            .await
            .unwrap()
    );

    // A new token for an inactive installation creates a new device ID.
    let reborn = DeviceRepository::upsert_device(
        &pool,
        UpsertDeviceParams {
            tenant_id: t1.id,
            subject_id: subj.id,
            application_id: app.id,
            installation_id: d3.installation_id,
            platform: DevicePlatform::Web,
            app_version: None,
            sdk_version: None,
            max_devices_per_subject: 5,
            now: now(),
        },
    )
    .await
    .unwrap();
    assert_ne!(reborn.id, d3.id);
}

#[tokio::test]
async fn test_fr_prv_005_audit_record_and_tenant_filtering() {
    // covers: FR-PRV-005 (audit storage base)
    let pool = setup_db().await;
    let t1 = new_tenant(&pool, "A1").await;
    let t2 = new_tenant(&pool, "A2").await;
    let subj = new_subject(&pool, t1.id, "audited").await;

    let ev = AuditEvent {
        id: AuditId::new(),
        tenant_id: t1.id,
        occurred_at: now(),
        actor_kind: ActorKind::Operator,
        actor_id: None,
        subject_id: Some(subj.id),
        action: "subject.tokens_revoked".into(),
        target_kind: Some("subject".into()),
        target_id: Some(*subj.id.as_uuid()),
        outcome: AuditOutcome::Success,
        request_id: None,
        details: json!({"reason": "test"}),
    };
    AuditRepository::record(&pool, &ev).await.unwrap();

    let list = AuditRepository::list_by_tenant(&pool, t1.id, 10)
        .await
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].action, "subject.tokens_revoked");
    assert_eq!(list[0].outcome, AuditOutcome::Success);
    assert_eq!(list[0].subject_id, Some(subj.id));

    assert!(
        AuditRepository::list_by_tenant(&pool, t2.id, 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        AuditRepository::list_by_subject(&pool, t1.id, subj.id, 10)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        AuditRepository::list_by_subject(&pool, t2.id, subj.id, 10)
            .await
            .unwrap()
            .is_empty()
    );
}
