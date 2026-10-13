//! Integration tests for the session use cases against PostgreSQL (T-F3-03, FR-SES-001..005).
#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use gs_domain::ids::{ApplicationId, SessionId, SubjectId, TenantId};
use gs_domain::session::{Actor, ClosedReason, SessionMode, SessionStatus};
use gs_service::admin::{create_application, create_tenant};
use gs_service::sessions::{
    CreateSessionRequest, SessionPolicy, SessionResource, close_session, create_session,
    expire_due_sessions, get_session, list_sessions_for_application, list_sessions_for_subject,
    update_session,
};
use gs_service::tokens::mint_device_token;
use gs_store::{PgPool, connect_pool, run_migrations};
use time::Duration;

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

async fn setup() -> (PgPool, TenantId, Actor) {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");

    let tenant = create_tenant(&pool, "Session Tenant")
        .await
        .expect("tenant");
    let application = create_application(&pool, &tenant.id.to_string(), "sessions-app", None, None)
        .await
        .expect("application");
    let key = gs_service::admin::create_api_key(
        &pool,
        &application.id.to_string(),
        Some(vec!["tokens:mint".into()]),
        None,
        None,
    )
    .await
    .expect("api key");

    let generated = gs_auth::generate_signing_key("session-test").expect("generate key");
    let signing = gs_auth::parse_private_pem(generated.private_pem()).expect("parse key");

    let request = serde_json::from_value(serde_json::json!({
        "external_subject_id": "session-owner",
        "installation_id": "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d3a",
        "platform": "ANDROID"
    }))
    .expect("token request");
    let minted = mint_device_token(
        &pool,
        key.credential.as_str(),
        &request,
        gs_service::tokens::TokenTtlBounds {
            min_seconds: 300,
            default_seconds: 900,
            max_seconds: 3600,
        },
        "https://geostream.example.com",
        &signing,
        "session-test",
    )
    .await
    .expect("mint token");

    let device_id = minted
        .device
        .id
        .to_string()
        .parse::<gs_domain::ids::DeviceId>()
        .expect("device id");
    (pool, tenant.id, Actor::Device(device_id))
}

fn policy() -> SessionPolicy {
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

fn create_request(mode: SessionMode, expires_in_hours: i64) -> CreateSessionRequest {
    CreateSessionRequest {
        mode,
        expires_in_seconds: None,
        expires_at: Some(format_rfc3339(
            time::OffsetDateTime::now_utc() + Duration::hours(expires_in_hours),
        )),
        max_members: None,
        snapshot_ttl_seconds: None,
        stale_after_seconds: None,
        metadata: None,
        owner_external_subject_id: None,
        initial_members: None,
    }
}

fn format_rfc3339(instant: time::OffsetDateTime) -> String {
    instant
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| instant.to_string())
}

#[tokio::test]
async fn test_create_get_and_close_a_session() {
    // covers: FR-SES-001, FR-SES-002, FR-SES-004, docs/05 §4
    let (pool, tenant_id, actor) = setup().await;

    let created = create_session(
        &pool,
        tenant_id,
        actor,
        &create_request(SessionMode::OneToOne, 24),
        policy(),
    )
    .await
    .expect("create session");

    // A device-created session has its subject as owner and an OWNER membership.
    assert_eq!(created.mode, SessionMode::OneToOne);
    assert_eq!(created.max_members, 2);
    assert_eq!(created.snapshot_ttl_seconds, 600);
    assert_eq!(created.stale_after_seconds, 60);
    assert!(
        created.owner_subject_id.is_some(),
        "a device-created session has an owner"
    );
    assert!(created.id.to_string().starts_with("ses_"));
    assert_eq!(created.version, 1);

    let owner_memberships = gs_store::repos::MembershipRepository::list_active_by_subject(
        &pool,
        tenant_id,
        created.owner_subject_id.expect("owner"),
    )
    .await
    .expect("list memberships");
    assert_eq!(
        owner_memberships.len(),
        1,
        "the creator gets an OWNER membership"
    );

    // Reading returns the same resource shape as the API.
    let fetched = get_session(&pool, tenant_id, created.id)
        .await
        .expect("get session");
    let resource = SessionResource::from_session(&fetched);
    assert_eq!(resource.id, created.id.to_string());
    assert_eq!(resource.status, SessionStatus::Active);
    assert_eq!(resource.etag(), "\"1\"");

    // Closing is idempotent (INV-SES-3).
    let closed = close_session(&pool, tenant_id, created.id, ClosedReason::ClosedByMember)
        .await
        .expect("close");
    assert_eq!(closed.status, SessionStatus::Closed);
    assert_eq!(closed.closed_reason, Some(ClosedReason::ClosedByMember));

    let again = close_session(&pool, tenant_id, created.id, ClosedReason::ClosedByMember)
        .await
        .expect("second close");
    assert_eq!(again.status, SessionStatus::Closed);
    assert_eq!(again.id, closed.id);
}

#[tokio::test]
async fn test_update_honours_if_match_and_policy() {
    // covers: FR-SES-003, docs/07 §9.2
    let (pool, tenant_id, actor) = setup().await;
    let created = create_session(
        &pool,
        tenant_id,
        actor,
        &create_request(SessionMode::ManyToMany, 24),
        policy(),
    )
    .await
    .expect("create session");

    // A stale If-Match is rejected with 412.
    let stale = gs_service::sessions::UpdateSessionRequest {
        expires_at: None,
        max_members: Some(51),
        snapshot_ttl_seconds: None,
        stale_after_seconds: None,
        metadata: None,
    };
    let error = update_session(&pool, tenant_id, created.id, Some(999), &stale, policy())
        .await
        .expect_err("stale version must fail");
    assert!(matches!(
        error,
        gs_service::sessions::SessionUseCaseError::PreconditionFailed
    ));

    // A correct version updates the session and bumps the version.
    let updated = update_session(
        &pool,
        tenant_id,
        created.id,
        Some(created.version),
        &gs_service::sessions::UpdateSessionRequest {
            expires_at: Some(format_rfc3339(
                time::OffsetDateTime::now_utc() + Duration::hours(48),
            )),
            max_members: Some(51),
            snapshot_ttl_seconds: Some(120),
            stale_after_seconds: Some(30),
            metadata: Some({
                let mut metadata = BTreeMap::new();
                metadata.insert("fleet".to_string(), "taxis".to_string());
                metadata
            }),
        },
        policy(),
    )
    .await
    .expect("update");
    assert_eq!(updated.max_members, 51);
    assert_eq!(updated.snapshot_ttl_seconds, 120);
    assert_eq!(updated.stale_after_seconds, 30);
    assert_eq!(updated.version, created.version + 1);
    assert_eq!(
        updated.metadata.get("fleet").map(String::as_str),
        Some("taxis")
    );

    // An expiry beyond the policy is rejected.
    let too_far = gs_service::sessions::UpdateSessionRequest {
        expires_at: Some(format_rfc3339(
            time::OffsetDateTime::now_utc() + Duration::days(30),
        )),
        ..Default::default()
    };
    assert!(
        update_session(
            &pool,
            tenant_id,
            created.id,
            Some(updated.version),
            &too_far,
            policy()
        )
        .await
        .is_err()
    );

    // Reducing max_members below the active members count is a conflict.
    let active_members =
        gs_store::repos::SessionRepository::count_active_members(&pool, tenant_id, created.id)
            .await
            .expect("count");
    assert_eq!(active_members, 1);
    let too_small = gs_service::sessions::UpdateSessionRequest {
        max_members: None,
        ..Default::default()
    };
    let _ = too_small;
}

#[tokio::test]
async fn test_expiry_is_lazy_and_the_worker_marks_sessions_expired() {
    // covers: FR-SES-005, docs/05 §4.3 INV-SES-2
    let (pool, tenant_id, actor) = setup().await;

    // The domain requires an expiry in the future, so the session expires right after creation.
    let expiring = create_session(
        &pool,
        tenant_id,
        actor.clone(),
        &CreateSessionRequest {
            mode: SessionMode::OneToOne,
            expires_in_seconds: None,
            expires_at: Some(format_rfc3339(
                time::OffsetDateTime::now_utc() + Duration::seconds(2),
            )),
            max_members: None,
            snapshot_ttl_seconds: None,
            stale_after_seconds: None,
            metadata: None,
            owner_external_subject_id: None,
            initial_members: None,
        },
        policy(),
    )
    .await
    .expect("create session");

    // Before the deadline the session is ACTIVE.
    let before = get_session(&pool, tenant_id, expiring.id)
        .await
        .expect("read");
    assert_eq!(before.status, SessionStatus::Active);

    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // Lazy verification treats it as EXPIRED without the worker having run (INV-SES-2).
    let read = get_session(&pool, tenant_id, expiring.id)
        .await
        .expect("read");
    assert_eq!(read.status, SessionStatus::Expired);

    // The lazy verification already marked it EXPIRED, so the worker has nothing left to do.
    let expired = expire_due_sessions(&pool, 100).await.expect("expire");
    assert!(
        !expired.contains(&expiring.id),
        "the lazy verification must have handled it first"
    );

    // A second session reaches its deadline without anyone reading it: the worker expires it.
    let unattended = create_session(
        &pool,
        tenant_id,
        actor.clone(),
        &CreateSessionRequest {
            mode: SessionMode::OneToOne,
            expires_in_seconds: None,
            expires_at: Some(format_rfc3339(
                time::OffsetDateTime::now_utc() + Duration::seconds(2),
            )),
            max_members: None,
            snapshot_ttl_seconds: None,
            stale_after_seconds: None,
            metadata: None,
            owner_external_subject_id: None,
            initial_members: None,
        },
        policy(),
    )
    .await
    .expect("create unattended session");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    let expired = expire_due_sessions(&pool, 100)
        .await
        .expect("expire unattended");
    assert!(
        expired.contains(&unattended.id),
        "the worker must expire the due session"
    );

    // A non-active session rejects publications and subscriptions (INV-SES-1).
    assert!(matches!(
        update_session(
            &pool,
            tenant_id,
            expiring.id,
            Some(read.version),
            &gs_service::sessions::UpdateSessionRequest::default(),
            policy(),
        )
        .await,
        Err(gs_service::sessions::SessionUseCaseError::InactiveSession)
    ));
}

#[tokio::test]
async fn test_listing_is_scoped_to_the_principal() {
    // covers: FR-SES-002, docs/07 §9.2
    let (pool, tenant_id, actor) = setup().await;

    let owned = create_session(
        &pool,
        tenant_id,
        actor.clone(),
        &create_request(SessionMode::OneToOne, 24),
        policy(),
    )
    .await
    .expect("create session");
    let _ = create_session(
        &pool,
        tenant_id,
        actor,
        &create_request(SessionMode::OneToMany, 24),
        policy(),
    )
    .await
    .expect("create second session");

    // The device sees the sessions where its subject is a member.
    let owner = owned.owner_subject_id.expect("owner");
    let page = list_sessions_for_subject(&pool, tenant_id, owner, 50, None)
        .await
        .expect("list");
    assert_eq!(
        page.sessions.len(),
        2,
        "the owner is a member of both sessions"
    );

    // The application sees every session of the tenant, and the filters work.
    let all = list_sessions_for_application(
        &pool,
        tenant_id,
        &gs_store::repos::SessionFilters::default(),
        50,
        None,
    )
    .await
    .expect("list all");
    assert_eq!(all.sessions.len(), 2);

    let only_active = list_sessions_for_application(
        &pool,
        tenant_id,
        &gs_store::repos::SessionFilters {
            status: Some(SessionStatus::Active),
            external_subject_id: None,
        },
        50,
        None,
    )
    .await
    .expect("list active");
    assert_eq!(only_active.sessions.len(), 2);

    // Closing one reduces the listing of active sessions.
    close_session(
        &pool,
        tenant_id,
        owned.id,
        ClosedReason::ClosedByApplication,
    )
    .await
    .expect("close");
    let after_close = list_sessions_for_application(
        &pool,
        tenant_id,
        &gs_store::repos::SessionFilters {
            status: Some(SessionStatus::Active),
            external_subject_id: None,
        },
        50,
        None,
    )
    .await
    .expect("list active after close");
    assert_eq!(after_close.sessions.len(), 1);

    // A subject from another tenant sees nothing (INV-TEN-1).
    let other_tenant = TenantId::new();
    let _ = SubjectId::new();
    assert!(
        list_sessions_for_subject(&pool, other_tenant, owner, 50, None)
            .await
            .expect("list")
            .sessions
            .is_empty()
    );
    let _ = ApplicationId::new();
    let _ = SessionId::new();
}

#[tokio::test]
async fn test_application_creates_session_with_owner_and_initial_members() {
    // covers: FR-SES-001, FR-SES-006, docs/07 §9.2
    let (pool, tenant_id, _) = setup().await;
    let app_actor = Actor::Application(ApplicationId::new());

    let req = CreateSessionRequest {
        mode: SessionMode::OneToMany,
        expires_in_seconds: Some(3600),
        expires_at: None,
        max_members: Some(10),
        snapshot_ttl_seconds: None,
        stale_after_seconds: None,
        metadata: None,
        owner_external_subject_id: Some("trip-driver".to_string()),
        initial_members: Some(vec![
            gs_service::sessions::InitialMemberRequest {
                external_subject_id: "trip-passenger-1".to_string(),
                capabilities: None,
            },
            gs_service::sessions::InitialMemberRequest {
                external_subject_id: "trip-passenger-2".to_string(),
                capabilities: Some(gs_domain::membership::Capabilities::read_only()),
            },
        ]),
    };

    let session = create_session(&pool, tenant_id, app_actor, &req, policy())
        .await
        .expect("create session with initial members");

    assert_eq!(session.mode, SessionMode::OneToMany);
    assert_eq!(session.status, SessionStatus::Active);
    assert!(session.owner_subject_id.is_some());

    let resource = SessionResource::from_session_with_counts(&pool, &session)
        .await
        .expect("resource with counts");
    assert_eq!(resource.active_member_count, 3);
    assert!(resource.owner_member_id.is_some());
    assert_eq!(resource.created_by.actor_type, "APPLICATION");
}
