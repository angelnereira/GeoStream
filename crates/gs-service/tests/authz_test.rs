//! Authorization cache against PostgreSQL: TTL revalidation through `mver`, bus invalidation
//! and principal revalidation (T-F4-03, NFR-SEC-002, ADR-0009, docs/04 §6.2, docs/06 §7).
#![forbid(unsafe_code)]

use std::time::Duration;

use gs_domain::session::Actor;
use gs_service::admin::{create_api_key, create_application, create_tenant};
use gs_service::authz::{AuthzCache, AuthzError, PrincipalIdentity, verify_principal_state};
use gs_service::gs_auth::{generate_signing_key, parse_private_pem};
use gs_service::migrate::{connect_pool, run_migrations};
use gs_service::sessions::{CreateSessionRequest, SessionPolicy, create_session};
use gs_service::tokens::{TokenRequest, TokenTtlBounds, mint_device_token};
use gs_service::{Bus, EphemeralStore, InMemoryBus, InMemoryEphemeralStore};
use gs_store::PgPool;
use gs_store::repos::SubjectRepository;
use time::OffsetDateTime;

const ISSUER: &str = "https://geostream.example.com";
const INSTALLATION: &str = "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d3a";

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

/// Tenant, application, device token and the session the device owns.
struct World {
    pool: PgPool,
    tenant_id: gs_domain::ids::TenantId,
    application_id: gs_domain::ids::ApplicationId,
    subject_id: gs_domain::ids::SubjectId,
    device_id: gs_domain::ids::DeviceId,
    token_epoch: u32,
    session_id: gs_domain::ids::SessionId,
}

async fn setup(suffix: &str) -> World {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");

    let tenant = create_tenant(&pool, &format!("Authz Tenant {suffix}"))
        .await
        .expect("tenant");
    let application = create_application(
        &pool,
        &tenant.id.to_string(),
        &format!("authz-app-{suffix}"),
        Some(vec![
            "sessions:create".to_string(),
            "stream:connect".to_string(),
        ]),
        None,
    )
    .await
    .expect("application");
    let key = create_api_key(
        &pool,
        &application.id.to_string(),
        Some(vec![
            "tokens:mint".to_string(),
            "stream:connect".to_string(),
        ]),
        Some("authz"),
        None,
    )
    .await
    .expect("api key");

    let kid = format!("authz-{suffix}");
    let generated = generate_signing_key(&kid).expect("generate key");
    let signing = parse_private_pem(generated.private_pem()).expect("parse key");

    let minted = mint_device_token(
        &pool,
        key.credential.as_str(),
        &TokenRequest {
            external_subject_id: format!("authz-user-{suffix}"),
            installation_id: INSTALLATION.to_string(),
            platform: "ANDROID".to_string(),
            app_version: None,
            sdk_version: None,
            scopes: Some(vec!["stream:connect".to_string()]),
            ttl_seconds: Some(900),
        },
        TokenTtlBounds {
            min_seconds: 300,
            default_seconds: 900,
            max_seconds: 3600,
        },
        ISSUER,
        &signing,
        &kid,
    )
    .await
    .expect("mint token");

    // The device creates the session, so the owner membership exists (FR-SES-001).
    let session = create_session(
        &pool,
        tenant.id,
        Actor::Device(minted.device.id),
        &serde_json::from_value::<CreateSessionRequest>(serde_json::json!({
            "mode": "MANY_TO_MANY",
            "max_members": 10
        }))
        .expect("request"),
        SessionPolicy::from_policy(&gs_service::gs_auth::TenantPolicy::default()),
    )
    .await
    .expect("create session");

    World {
        pool,
        tenant_id: tenant.id,
        application_id: application.id,
        subject_id: minted.subject.id,
        device_id: minted.device.id,
        token_epoch: minted.subject.token_epoch,
        session_id: session.id,
    }
}

impl World {
    fn principal(&self) -> PrincipalIdentity {
        PrincipalIdentity {
            tenant_id: self.tenant_id,
            application_id: self.application_id,
            subject_id: Some(self.subject_id),
            device_id: Some(self.device_id),
            token_epoch: Some(self.token_epoch),
        }
    }
}

#[tokio::test]
async fn test_device_decision_is_cached_and_invalidated_by_the_bus() {
    // covers: NFR-SEC-002, ADR-0009, docs/04 §6.2
    let world = setup("cache").await;
    let ephemeral = InMemoryEphemeralStore::new();
    let cache = AuthzCache::new(Duration::from_secs(60));
    let prefix = "gs:authz:cache";

    // First question loads from PostgreSQL and caches.
    let first = cache
        .decision_for_device(
            &world.pool,
            ephemeral.as_ref(),
            prefix,
            world.tenant_id,
            world.session_id,
            world.subject_id,
        )
        .await
        .expect("decision")
        .expect("membership");
    assert_eq!(cache.len(), 1);
    assert!(
        first.capabilities.manage,
        "the owner keeps its capabilities"
    );

    // Second question hits the cache: the entry is not reloaded.
    let second = cache
        .decision_for_device(
            &world.pool,
            ephemeral.as_ref(),
            prefix,
            world.tenant_id,
            world.session_id,
            world.subject_id,
        )
        .await
        .expect("decision")
        .expect("membership");
    assert_eq!(first.member_id, second.member_id);
    assert_eq!(cache.len(), 1);

    // A control event of the session invalidates the entry immediately (ADR-0009).
    assert_eq!(cache.invalidate_session(world.session_id), 1);
    assert!(cache.is_empty());
    let third = cache
        .decision_for_device(
            &world.pool,
            ephemeral.as_ref(),
            prefix,
            world.tenant_id,
            world.session_id,
            world.subject_id,
        )
        .await
        .expect("decision")
        .expect("membership");
    assert_eq!(
        third.member_id, first.member_id,
        "the question is answered again from PostgreSQL"
    );
    assert_eq!(cache.len(), 1);
}

#[tokio::test]
async fn test_mver_change_forces_a_reload_of_the_membership() {
    // covers: ADR-0009, docs/04 §6.2, docs/11 §3
    let world = setup("mver").await;
    let ephemeral = InMemoryEphemeralStore::new();
    // A TTL of zero forces revalidation on every question.
    let cache = AuthzCache::new(Duration::from_millis(0));
    let prefix = "gs:authz:mver";

    let first = cache
        .decision_for_device(
            &world.pool,
            ephemeral.as_ref(),
            prefix,
            world.tenant_id,
            world.session_id,
            world.subject_id,
        )
        .await
        .expect("decision")
        .expect("membership");

    // Bumping the membership version, as every membership mutation does after the commit
    // (docs/11 §2.1), makes the cached entry stale: the next question reloads.
    ephemeral
        .increment_membership_version(&format!("{prefix}:mver:{}", world.session_id))
        .await
        .expect("increment mver");

    let second = cache
        .decision_for_device(
            &world.pool,
            ephemeral.as_ref(),
            prefix,
            world.tenant_id,
            world.session_id,
            world.subject_id,
        )
        .await
        .expect("decision")
        .expect("membership");
    assert_eq!(first.mver, 0);
    assert_eq!(
        second.mver, 1,
        "the reload reads the new membership version"
    );
    assert_eq!(first.member_id, second.member_id);
    assert_eq!(cache.len(), 1);
}

#[tokio::test]
async fn test_unchanged_mver_renews_the_entry_without_touching_postgresql() {
    // covers: ADR-0009, docs/04 §6.2
    let world = setup("renew").await;
    let ephemeral = InMemoryEphemeralStore::new();
    let cache = AuthzCache::new(Duration::from_millis(0));
    let prefix = "gs:authz:renew";

    let first = cache
        .decision_for_device(
            &world.pool,
            ephemeral.as_ref(),
            prefix,
            world.tenant_id,
            world.session_id,
            world.subject_id,
        )
        .await
        .expect("decision")
        .expect("membership");

    // The membership version never changes: the entry is renewed, not reloaded.
    let second = cache
        .decision_for_device(
            &world.pool,
            ephemeral.as_ref(),
            prefix,
            world.tenant_id,
            world.session_id,
            world.subject_id,
        )
        .await
        .expect("decision")
        .expect("membership");
    assert_eq!(first.mver, second.mver);
    assert_eq!(first.member_id, second.member_id);
    assert_eq!(cache.len(), 1);
}

#[tokio::test]
async fn test_invalidated_entry_is_reloaded_from_postgresql() {
    // covers: NFR-SEC-002, docs/06 §7 (revocación de membresía)
    let world = setup("revoked").await;
    let ephemeral = InMemoryEphemeralStore::new();
    let cache = AuthzCache::new(Duration::from_secs(60));
    let prefix = "gs:authz:revoked";

    let first = cache
        .decision_for_device(
            &world.pool,
            ephemeral.as_ref(),
            prefix,
            world.tenant_id,
            world.session_id,
            world.subject_id,
        )
        .await
        .expect("decision")
        .expect("membership");
    assert!(!first.sharing, "sharing is off by default (FR-SHR-001)");

    // Any mutation of the session publishes its control event; the cache drops the entry and
    // the next question reloads from PostgreSQL (docs/04 §6.2).
    assert_eq!(cache.invalidate_session(world.session_id), 1);
    let second = cache
        .decision_for_device(
            &world.pool,
            ephemeral.as_ref(),
            prefix,
            world.tenant_id,
            world.session_id,
            world.subject_id,
        )
        .await
        .expect("decision")
        .expect("membership");
    assert_eq!(first.member_id, second.member_id);
}

#[tokio::test]
async fn test_principal_revalidation_detects_a_revoked_token() {
    // covers: NFR-SEC-002, FR-AUTH-003, docs/06 §4.3 step 4, §7, ADR-0009
    let world = setup("principal").await;
    let cache = AuthzCache::new(Duration::from_secs(60));
    let identity = world.principal();

    // The first check reaches PostgreSQL and caches the result.
    cache
        .revalidate_principal(&world.pool, &identity)
        .await
        .expect("principal is valid");
    assert_eq!(cache.len(), 1);
    cache
        .revalidate_principal(&world.pool, &identity)
        .await
        .expect("still valid inside the TTL");
    assert_eq!(
        cache.len(),
        1,
        "the principal is not re-queried inside the TTL"
    );

    // Revoking the tokens of the subject bumps its epoch (docs/06 §7).
    SubjectRepository::increment_token_epoch(
        &world.pool,
        world.tenant_id,
        world.subject_id,
        OffsetDateTime::now_utc(),
    )
    .await
    .expect("bump epoch");

    // Worst case of ADR-0009: inside the TTL the cached decision still holds.
    cache
        .revalidate_principal(&world.pool, &identity)
        .await
        .expect("the decision is bounded by the TTL");

    // The bus event drops the entry immediately, which is the normal case.
    cache.invalidate_principal(&identity);
    let error = cache
        .revalidate_principal(&world.pool, &identity)
        .await
        .expect_err("a revoked token must stop working");
    assert!(matches!(error, AuthzError::Unauthenticated(_)));

    // With a TTL of zero every question reaches PostgreSQL, so the revocation is detected
    // on the next one (the worst case bound of ADR-0009).
    let unbounded = AuthzCache::new(Duration::from_millis(0));
    unbounded
        .revalidate_principal(&world.pool, &identity)
        .await
        .expect_err("without a TTL the revocation is immediate");
}

#[tokio::test]
async fn test_principal_revalidation_detects_a_disabled_application() {
    // covers: NFR-SEC-002, FR-ADM-003, docs/06 §4.3 step 4a
    let world = setup("disabled").await;
    let identity = PrincipalIdentity {
        tenant_id: world.tenant_id,
        application_id: world.application_id,
        subject_id: None,
        device_id: None,
        token_epoch: None,
    };

    verify_principal_state(&world.pool, &identity)
        .await
        .expect("an enabled application passes");

    gs_service::admin::set_application_enabled(
        &world.pool,
        &world.application_id.to_string(),
        false,
    )
    .await
    .expect("disable application");
    let error = verify_principal_state(&world.pool, &identity)
        .await
        .expect_err("a disabled application must be refused");
    assert!(matches!(error, AuthzError::AccountDisabled(_)));

    gs_service::admin::set_application_enabled(
        &world.pool,
        &world.application_id.to_string(),
        true,
    )
    .await
    .expect("enable application");
    verify_principal_state(&world.pool, &identity)
        .await
        .expect("re-enabling restores access");
}

#[tokio::test]
async fn test_bus_global_events_reach_the_cache() {
    // covers: ADR-0009, docs/04 §6.1
    let world = setup("bus").await;
    let ephemeral = InMemoryEphemeralStore::new();
    let bus = InMemoryBus::new();
    let cache = AuthzCache::new(Duration::from_secs(60));
    let prefix = "gs:authz:bus";
    let identity = world.principal();

    cache
        .decision_for_device(
            &world.pool,
            ephemeral.as_ref(),
            prefix,
            world.tenant_id,
            world.session_id,
            world.subject_id,
        )
        .await
        .expect("decision");
    cache
        .revalidate_principal(&world.pool, &identity)
        .await
        .expect("principal");
    assert_eq!(cache.len(), 2);

    // The control plane publishes on `{prefix}:bus:global`; the cache consumes the message.
    let topic = format!("{prefix}:bus:global");
    let subscription = bus.subscribe(&topic).await.expect("subscribe");
    let payload = format!(
        r#"{{"event":"SubjectTokensRevoked","tenant_id":"{}","subject_id":"{}","token_epoch":2}}"#,
        world.tenant_id, world.subject_id
    );
    bus.publish(&topic, payload.as_bytes())
        .await
        .expect("publish");

    let message = bus.next_message(subscription).await.expect("message");
    cache.note_global_bus_event(&message.expect("payload").payload);
    assert!(
        cache.is_empty(),
        "the principal and its decisions of the revoked subject are dropped"
    );
}
