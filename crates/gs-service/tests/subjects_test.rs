//! Integration tests for subjects and devices use cases against PostgreSQL
//! (T-F3-07, FR-AUTH-005, docs/06 §7, docs/07 §9.5).
#![forbid(unsafe_code)]

use std::time::Duration;

use gs_auth::jwt::SigningKeySet;
use gs_domain::ids::{DeviceId, SubjectId, TenantId};
use gs_domain::session::{Actor, SessionMode};
use gs_service::admin::{create_application, create_tenant};
use gs_service::auth::verify_device_token_principal;
use gs_service::members::{SetSharingCaller, SetSharingContext, set_sharing};
use gs_service::sessions::{CreateSessionRequest, SessionPolicy, create_session};
use gs_service::subjects::{
    DeactivateDeviceParams, SubjectUseCaseError, deactivate_device, find_subject_by_external_id,
    get_device, get_subject, revoke_subject_tokens,
};
use gs_service::tokens::mint_device_token;
use gs_service::{Bus, EphemeralStore, InMemoryBus, InMemoryEphemeralStore};
use gs_store::repos::MembershipRepository;
use gs_store::{PgPool, connect_pool, run_migrations};
use time::OffsetDateTime;

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

async fn setup_fixture() -> (PgPool, TenantId, String, gs_auth::SigningKey, SigningKeySet) {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");

    let tenant = create_tenant(&pool, "Subjects Test Tenant")
        .await
        .expect("tenant");
    let application = create_application(&pool, &tenant.id.to_string(), "subjects-app", None, None)
        .await
        .expect("application");
    let key = gs_service::admin::create_api_key(
        &pool,
        &application.id.to_string(),
        Some(vec![
            "tokens:mint".into(),
            "sessions:write".into(),
            "sessions:read".into(),
            "members:write".into(),
            "subjects:manage".into(),
        ]),
        None,
        None,
    )
    .await
    .expect("api key");

    let dir = std::env::temp_dir().join(format!("gs-subjects-test-{}", uuid::Uuid::new_v4()));
    let _ = std::fs::remove_dir_all(&dir);
    let generated = gs_auth::generate_signing_key("sub-test").expect("generate key");
    generated.write_pair(&dir).expect("write pair");
    let keys = SigningKeySet::load_from_dir(&dir).expect("load keys");
    let signing = gs_auth::parse_private_pem(generated.private_pem()).expect("parse key");
    let _ = std::fs::remove_dir_all(&dir);

    (pool, tenant.id, key.credential.to_string(), signing, keys)
}

fn test_session_policy() -> SessionPolicy {
    SessionPolicy {
        default_max_members_one_to_many: 100,
        default_max_members_many_to_many: 50,
        max_members_cap: 1000,
        snapshot_ttl_default_seconds: 600,
        snapshot_ttl_max_seconds: 86_400,
        stale_after_seconds_default: 60,
        max_session_ttl_seconds: 604_800,
    }
}

async fn mint_device(
    pool: &PgPool,
    api_key: &str,
    signing: &gs_auth::SigningKey,
    external_subject_id: &str,
) -> (DeviceId, SubjectId, String) {
    let installation_id = uuid::Uuid::new_v4().to_string();
    let res = mint_device_token(
        pool,
        api_key,
        &gs_service::tokens::TokenRequest {
            external_subject_id: external_subject_id.to_string(),
            installation_id,
            platform: "ANDROID".into(),
            app_version: Some("1.0.0".into()),
            sdk_version: Some("0.1.0".into()),
            scopes: Some(vec![
                "sessions:create".into(),
                "stream:connect".into(),
                "sharing:write".into(),
            ]),
            ttl_seconds: Some(3600),
        },
        gs_service::tokens::TokenTtlBounds {
            min_seconds: 60,
            default_seconds: 900,
            max_seconds: 86400,
        },
        "https://geostream.example.com",
        signing,
        "sub-test",
    )
    .await
    .expect("mint");
    (res.device.id, res.subject.id, res.response.access_token)
}

#[tokio::test]
async fn test_find_and_get_subject() {
    let (pool, tenant_id, api_key, signing, _keys) = setup_fixture().await;
    let (_dev_id, sub_id, _token) = mint_device(&pool, &api_key, &signing, "user-alice").await;

    // 1. find_subject_by_external_id - found
    let found = find_subject_by_external_id(&pool, tenant_id, "user-alice")
        .await
        .expect("find")
        .expect("must be some");
    assert_eq!(found.id, sub_id);
    assert_eq!(found.external_subject_id, "user-alice");
    assert_eq!(found.status, "ACTIVE");
    assert_eq!(found.token_epoch, 0);

    // 2. find_subject_by_external_id - not found
    let not_found = find_subject_by_external_id(&pool, tenant_id, "nonexistent-user")
        .await
        .expect("find");
    assert!(not_found.is_none());

    // 3. find_subject_by_external_id - invalid format (contains '@')
    let invalid = find_subject_by_external_id(&pool, tenant_id, "alice@domain").await;
    assert!(matches!(
        invalid,
        Err(SubjectUseCaseError::InvalidExternalSubjectId(_))
    ));

    // 4. get_subject - found
    let got = get_subject(&pool, tenant_id, sub_id)
        .await
        .expect("get_subject");
    assert_eq!(got.id, sub_id);
    assert_eq!(got.external_subject_id, "user-alice");
    assert_eq!(got.status, "ACTIVE");

    // 5. get_subject - not found (random UUID)
    let unknown_sub = SubjectId::new();
    let err = get_subject(&pool, tenant_id, unknown_sub).await;
    assert!(matches!(err, Err(SubjectUseCaseError::SubjectNotFound)));

    // 6. get_subject - other tenant isolation
    let other_tenant = create_tenant(&pool, "Other Tenant").await.expect("other");
    let err_iso = get_subject(&pool, other_tenant.id, sub_id).await;
    assert!(matches!(err_iso, Err(SubjectUseCaseError::SubjectNotFound)));
}

#[tokio::test]
async fn test_revoke_subject_tokens_lifecycle() {
    let (pool, tenant_id, api_key, signing, keys) = setup_fixture().await;
    let (_dev_id, sub_id, token) = mint_device(&pool, &api_key, &signing, "user-bob").await;

    // Token verifies successfully before revocation
    let verified =
        verify_device_token_principal(&pool, &token, &keys, "https://geostream.example.com", 30)
            .await
            .expect("verify token before revocation");
    assert_eq!(verified.subject.id, sub_id);

    // Revoke tokens
    let now = OffsetDateTime::now_utc();
    let updated = revoke_subject_tokens(&pool, tenant_id, sub_id, now)
        .await
        .expect("revoke tokens");
    assert_eq!(updated.token_epoch, 1);

    // Previously minted token now fails verification (returns 401 UNAUTHENTICATED)
    let verify_err =
        verify_device_token_principal(&pool, &token, &keys, "https://geostream.example.com", 30)
            .await;
    assert!(verify_err.is_err());
    let err = verify_err.unwrap_err();
    assert_eq!(
        gs_domain::error::AsErrorCode::error_code(&err),
        gs_domain::error::ErrorCode::Unauthenticated
    );

    // Revoke again -> epoch increments to 2
    let updated2 = revoke_subject_tokens(&pool, tenant_id, sub_id, now)
        .await
        .expect("revoke tokens again");
    assert_eq!(updated2.token_epoch, 2);

    // Other tenant cannot revoke
    let other_tenant = create_tenant(&pool, "Other Tenant 2").await.expect("other");
    let err_iso = revoke_subject_tokens(&pool, other_tenant.id, sub_id, now).await;
    assert!(matches!(err_iso, Err(SubjectUseCaseError::SubjectNotFound)));
}

#[tokio::test]
async fn test_get_and_deactivate_device() {
    let (pool, tenant_id, api_key, signing, keys) = setup_fixture().await;
    let (dev_id, sub_id, token) = mint_device(&pool, &api_key, &signing, "user-carol").await;
    let ephemeral = InMemoryEphemeralStore::new();
    let bus = InMemoryBus::new();

    // 1. get_device as application (caller_subject_id is None)
    let dev = get_device(&pool, tenant_id, dev_id, None)
        .await
        .expect("get device app");
    assert_eq!(dev.id, dev_id);
    assert_eq!(dev.subject_id, sub_id);
    assert_eq!(dev.status, "ACTIVE");
    assert_eq!(dev.platform, "ANDROID");

    // 2. get_device as device caller (same subject)
    let dev_self = get_device(&pool, tenant_id, dev_id, Some(sub_id))
        .await
        .expect("get device self");
    assert_eq!(dev_self.id, dev_id);

    // 3. get_device as device caller (different subject -> DeviceNotFound FR-API-006)
    let other_sub = SubjectId::new();
    let dev_diff = get_device(&pool, tenant_id, dev_id, Some(other_sub)).await;
    assert!(matches!(dev_diff, Err(SubjectUseCaseError::DeviceNotFound)));

    // 4. deactivate_device with caller_did of another device -> DeviceNotFound (FR-API-006)
    let other_did = DeviceId::new();
    let now = OffsetDateTime::now_utc();
    let deact_err = deactivate_device(DeactivateDeviceParams {
        pool: &pool,
        ephemeral: &ephemeral,
        bus: &bus,
        tenant_id,
        device_id: dev_id,
        caller_device_id: Some(other_did),
        key_prefix: "gs",
        now,
    })
    .await;
    assert!(matches!(
        deact_err,
        Err(SubjectUseCaseError::DeviceNotFound)
    ));

    // 5. deactivate_device with own caller_did -> Success
    deactivate_device(DeactivateDeviceParams {
        pool: &pool,
        ephemeral: &ephemeral,
        bus: &bus,
        tenant_id,
        device_id: dev_id,
        caller_device_id: Some(dev_id),
        key_prefix: "gs",
        now,
    })
    .await
    .expect("deactivate own device");

    // Device status is now INACTIVE
    let dev_after = get_device(&pool, tenant_id, dev_id, None)
        .await
        .expect("get device after deactivation");
    assert_eq!(dev_after.status, "INACTIVE");

    // Token from deactivated device fails verification (401 UNAUTHENTICATED)
    let verify_err =
        verify_device_token_principal(&pool, &token, &keys, "https://geostream.example.com", 30)
            .await;
    assert!(verify_err.is_err());
    let err = verify_err.unwrap_err();
    assert_eq!(
        gs_domain::error::AsErrorCode::error_code(&err),
        gs_domain::error::ErrorCode::Unauthenticated
    );

    // 6. Idempotency: deactivate already INACTIVE device succeeds
    deactivate_device(DeactivateDeviceParams {
        pool: &pool,
        ephemeral: &ephemeral,
        bus: &bus,
        tenant_id,
        device_id: dev_id,
        caller_device_id: Some(dev_id),
        key_prefix: "gs",
        now,
    })
    .await
    .expect("idempotent deactivate");
}

#[tokio::test]
async fn test_deactivate_device_stops_active_publisher() {
    let (pool, tenant_id, api_key, signing, _keys) = setup_fixture().await;
    let (dev_id, sub_id, _token) = mint_device(&pool, &api_key, &signing, "user-david").await;
    let ephemeral = InMemoryEphemeralStore::new();
    let bus = InMemoryBus::new();

    // Create session created by device dev_id
    let session = create_session(
        &pool,
        tenant_id,
        Actor::Device(dev_id),
        &CreateSessionRequest {
            mode: SessionMode::OneToMany,
            expires_in_seconds: Some(3600),
            expires_at: None,
            max_members: Some(10),
            snapshot_ttl_seconds: Some(60),
            stale_after_seconds: Some(30),
            metadata: None,
            owner_external_subject_id: None,
            initial_members: None,
        },
        test_session_policy(),
    )
    .await
    .expect("create session");

    // Find the owner member id for user-david
    let member_page =
        MembershipRepository::list_active_by_session(&pool, tenant_id, session.id, 10, None)
            .await
            .expect("list members");
    let owner_member = member_page
        .members
        .into_iter()
        .find(|m| m.membership.subject_id == sub_id)
        .expect("owner member exists");
    let member_id = owner_member.membership.id;

    // Activate sharing for user-david with dev_id as publisher
    let sharing_res = set_sharing(
        &pool,
        tenant_id,
        session.id,
        member_id,
        true,
        SetSharingContext {
            ephemeral: &ephemeral,
            bus: &bus,
            key_prefix: "gs",
            caller: SetSharingCaller {
                subject_id: Some(sub_id),
                device_id: Some(dev_id),
                is_app: false,
                has_manage: true,
            },
        },
    )
    .await
    .expect("activate sharing");
    assert!(sharing_res.sharing);

    // Seed snapshot in ephemeral store
    let snap_key = format!("gs:snap:{}:{member_id}", session.id);
    ephemeral
        .set_snapshot(
            &snap_key,
            b"location_snapshot_payload",
            Duration::from_secs(60),
        )
        .await
        .expect("set snapshot");
    assert!(
        ephemeral
            .get_snapshot(&snap_key)
            .await
            .expect("get")
            .is_some()
    );

    // Subscribe to bus control channel
    let ctl_topic = format!("gs:bus:ctl:{}", session.id);
    let sub_id_bus = bus.subscribe(&ctl_topic).await.expect("subscribe");

    // Deactivate device
    deactivate_device(DeactivateDeviceParams {
        pool: &pool,
        ephemeral: &ephemeral,
        bus: &bus,
        tenant_id,
        device_id: dev_id,
        caller_device_id: None, // application caller
        key_prefix: "gs",
        now: OffsetDateTime::now_utc(),
    })
    .await
    .expect("deactivate device");

    // Verify membership in DB has sharing = false and publisher_device_id = NULL
    let member_row = MembershipRepository::get_by_id(&pool, tenant_id, session.id, member_id)
        .await
        .expect("get member")
        .expect("exists");
    assert!(!member_row.membership.sharing);
    assert_eq!(member_row.membership.publisher_device_id, None);

    // Verify ephemeral snapshot was deleted (NFR-PRIV-002)
    assert!(
        ephemeral
            .get_snapshot(&snap_key)
            .await
            .expect("get")
            .is_none()
    );

    // Verify bus received SharingStopped event
    let msg = bus
        .next_message(sub_id_bus)
        .await
        .expect("next")
        .expect("message");
    let payload_str = String::from_utf8_lossy(&msg.payload);
    assert!(payload_str.contains("SharingStopped"));
    assert!(payload_str.contains(&dev_id.to_string()));
}
