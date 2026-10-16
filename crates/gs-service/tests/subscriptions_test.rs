//! Subscription use case against PostgreSQL: authorization, snapshot and the presence
//! minimization of docs/05 §8 (T-F4-04, FR-STR-002, FR-PRS-001, FR-PRS-004).
#![forbid(unsafe_code)]

use gs_domain::ids::{ApplicationId, SubjectId, TenantId};
use gs_domain::session::Actor;
use gs_service::admin::{create_api_key, create_application, create_tenant};
use gs_service::gs_auth::{generate_signing_key, parse_private_pem};
use gs_service::gs_domain::ids::DeviceId;
use gs_service::gs_domain::membership::Capabilities;
use gs_service::gs_protocol::{DeliveryKind, LocationSample};
use gs_service::pbjson_types::Timestamp;
use gs_service::sessions::{CreateSessionRequest, SessionPolicy, create_session};
use gs_service::subscriptions::{SubscribeError, SubscribeParams, SubscribePrincipal, subscribe};
use gs_service::tokens::{TokenRequest, TokenTtlBounds, mint_device_token};
use gs_service::{EphemeralStore, InMemoryBus, InMemoryEphemeralStore};
use gs_store::repos::MembershipRepository;
use gs_store::{PgPool, connect_pool, run_migrations};
use prost::Message as _;
use time::OffsetDateTime;

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

/// Tenant, application, a device that owns a session and a second subject.
struct World {
    pool: PgPool,
    tenant_id: TenantId,
    application_id: ApplicationId,
    subject_id: SubjectId,
    device_id: DeviceId,
    outsider: SubjectId,
    session_id: gs_domain::ids::SessionId,
}

async fn setup(suffix: &str) -> World {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");

    let tenant = create_tenant(&pool, &format!("Sub Tenant {suffix}"))
        .await
        .expect("tenant");
    let application = create_application(
        &pool,
        &tenant.id.to_string(),
        &format!("sub-app-{suffix}"),
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
            "locations:read".to_string(),
        ]),
        Some("sub"),
        None,
    )
    .await
    .expect("api key");

    let kid = format!("sub-{suffix}");
    let generated = generate_signing_key(&kid).expect("generate key");
    let signing = parse_private_pem(generated.private_pem()).expect("parse key");

    let mint = |external: String| {
        let pool = pool.clone();
        let signing = signing.clone();
        let kid = kid.clone();
        let credential = key.credential.to_string();
        async move {
            mint_device_token(
                &pool,
                credential.as_str(),
                &TokenRequest {
                    external_subject_id: external,
                    installation_id: "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d3a".to_string(),
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
                "https://geostream.example.com",
                &signing,
                &kid,
            )
            .await
            .expect("mint token")
        }
    };

    let owner = mint(format!("sub-owner-{suffix}")).await;
    let outsider = mint(format!("sub-outsider-{suffix}")).await;

    // The device creates the session, so the owner membership exists (FR-SES-001).
    let session = create_session(
        &pool,
        tenant.id,
        Actor::Device(owner.device.id),
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
        subject_id: owner.subject.id,
        device_id: owner.device.id,
        outsider: outsider.subject.id,
        session_id: session.id,
    }
}

impl World {
    /// Asks for a subscription as the owner device.
    async fn subscribe_as_device(
        &self,
        ephemeral: &InMemoryEphemeralStore,
        prefix: &str,
        subject_id: SubjectId,
    ) -> Result<gs_service::subscriptions::SubscribeView, SubscribeError> {
        subscribe(SubscribeParams {
            pool: &self.pool,
            ephemeral,
            key_prefix: prefix,
            tenant_id: self.tenant_id,
            session_id: self.session_id,
            principal: SubscribePrincipal::Device {
                subject_id,
                device_id: self.device_id,
            },
            now: OffsetDateTime::now_utc(),
        })
        .await
    }
}

#[tokio::test]
async fn test_the_owner_subscribes_and_sees_the_session_and_members() {
    // covers: FR-STR-002, docs/08 §6.3
    let world = setup("owner").await;
    let ephemeral = InMemoryEphemeralStore::new();
    let prefix = "gs:sub:owner";

    let view = world
        .subscribe_as_device(ephemeral.as_ref(), prefix, world.subject_id)
        .await
        .expect("subscription");

    assert_eq!(view.session.session_id, world.session_id);
    assert_eq!(view.session.max_members, 10);
    assert_eq!(view.session.stale_after_seconds, 60);
    assert_eq!(view.session.snapshot_ttl_seconds, 600);
    assert_eq!(view.members.len(), 1, "the owner is the only member");
    let member = &view.members[0];
    assert!(
        member.member_id.to_string().starts_with("mem_"),
        "typed member id"
    );
    assert!(member.capabilities.read, "the owner can read");
    assert!(member.capabilities.publish);
    assert!(!member.sharing, "sharing is off by default (FR-SHR-001)");
    // FR-PRS-004: a member that does not share never reveals connectivity.
    assert_eq!(
        member.connectivity,
        gs_service::gs_domain::membership::Connectivity::Unspecified
    );
    assert!(member.last_location_at.is_none());
    assert!(view.snapshot.is_empty(), "nothing was published yet");
}

#[tokio::test]
async fn test_a_non_member_is_not_told_the_session_exists() {
    // covers: FR-STR-002, FR-API-006
    let world = setup("outsider").await;
    let ephemeral = InMemoryEphemeralStore::new();

    let error = world
        .subscribe_as_device(ephemeral.as_ref(), "gs:sub:outsider", world.outsider)
        .await
        .expect_err("a non member must be refused");
    assert!(matches!(error, SubscribeError::NotFound));
}

#[tokio::test]
async fn test_a_member_without_read_is_denied() {
    // covers: FR-STR-002, docs/06 §8.3
    let world = setup("no-read").await;
    let ephemeral = InMemoryEphemeralStore::new();

    // A second subject joins without capabilities.
    let blind = gs_service::members::add_member(
        &world.pool,
        world.tenant_id,
        world.session_id,
        &gs_service::members::AddMemberRequest {
            external_subject_id: "sub-blind".to_string(),
            role: None,
            capabilities: Some(Capabilities::none()),
        },
    )
    .await
    .expect("add member");

    let blind_subject_id = blind.subject_id.parse().expect("subject id");
    let error = world
        .subscribe_as_device(ephemeral.as_ref(), "gs:sub:no-read", blind_subject_id)
        .await
        .expect_err("a member without read must be refused");
    assert!(matches!(error, SubscribeError::PermissionDenied(_)));
}

#[tokio::test]
async fn test_an_application_needs_locations_read() {
    // covers: FR-STR-002, docs/06 §8.3
    let world = setup("app").await;
    let ephemeral = InMemoryEphemeralStore::new();

    let allowed = subscribe(SubscribeParams {
        pool: &world.pool,
        ephemeral: ephemeral.as_ref(),
        key_prefix: "gs:sub:app",
        tenant_id: world.tenant_id,
        session_id: world.session_id,
        principal: SubscribePrincipal::Application {
            application_id: world.application_id,
            scopes: &["locations:read".to_string()],
        },
        now: OffsetDateTime::now_utc(),
    })
    .await
    .expect("an application with locations:read subscribes");
    assert_eq!(allowed.members.len(), 1);

    let denied = subscribe(SubscribeParams {
        pool: &world.pool,
        ephemeral: ephemeral.as_ref(),
        key_prefix: "gs:sub:app",
        tenant_id: world.tenant_id,
        session_id: world.session_id,
        principal: SubscribePrincipal::Application {
            application_id: world.application_id,
            scopes: &["stream:connect".to_string()],
        },
        now: OffsetDateTime::now_utc(),
    })
    .await
    .expect_err("without locations:read the application is refused");
    assert!(matches!(denied, SubscribeError::PermissionDenied(_)));
}

#[tokio::test]
async fn test_a_closed_and_an_expired_session_are_refused() {
    // covers: FR-STR-002, INV-SES-1, INV-SES-2
    let world = setup("closed").await;
    let ephemeral = InMemoryEphemeralStore::new();

    gs_service::sessions::close_session(
        &world.pool,
        world.tenant_id,
        world.session_id,
        gs_service::gs_domain::session::ClosedReason::ClosedByMember,
    )
    .await
    .expect("close session");

    let error = world
        .subscribe_as_device(ephemeral.as_ref(), "gs:sub:closed", world.subject_id)
        .await
        .expect_err("a closed session refuses subscriptions");
    assert!(matches!(error, SubscribeError::SessionClosed));
}

#[tokio::test]
async fn test_the_snapshot_only_contains_members_that_share() {
    // covers: FR-STR-002, FR-PRS-001, docs/08 §6.3, docs/11 §3
    let world = setup("snapshot").await;
    let ephemeral = InMemoryEphemeralStore::new();
    let prefix = "gs:sub:snapshot";

    // The owner starts sharing from its own device (FR-SHR-002).
    let membership = MembershipRepository::find_active_by_session_and_subject(
        &world.pool,
        world.tenant_id,
        world.session_id,
        world.subject_id,
    )
    .await
    .expect("query")
    .expect("membership");
    gs_service::members::set_sharing(
        &world.pool,
        world.tenant_id,
        world.session_id,
        membership.id,
        true,
        gs_service::members::SetSharingContext {
            ephemeral: ephemeral.as_ref(),
            bus: &InMemoryBus::new(),
            key_prefix: prefix,
            caller: gs_service::members::SetSharingCaller {
                is_app: false,
                subject_id: Some(world.subject_id),
                device_id: Some(world.device_id),
                has_manage: true,
            },
        },
    )
    .await
    .expect("start sharing");

    // Presence of the publishing device: ONLINE (docs/05 §8).
    ephemeral
        .set_presence(
            &format!("{prefix}:presence:{device}", device = world.device_id),
            &[("connection_id", "con_1"), ("instance_id", "i-1")],
            std::time::Duration::from_secs(90),
        )
        .await
        .expect("presence");

    let update = gs_service::gs_protocol::LocationUpdate {
        session_id: world.session_id.to_string(),
        member_id: membership.id.to_string(),
        sample: Some(LocationSample {
            sequence: 12,
            captured_at: Some(Timestamp {
                seconds: OffsetDateTime::now_utc().unix_timestamp() - 5,
                nanos: 0,
            }),
            ..Default::default()
        }),
        received_at: None,
        delivery: DeliveryKind::Snapshot as i32,
        estimate: None,
    };
    let snapshot_key = format!(
        "{prefix}:snap:{session}:{member}",
        session = world.session_id,
        member = membership.id
    );
    ephemeral
        .set_snapshot(
            &snapshot_key,
            &update.encode_to_vec(),
            std::time::Duration::from_secs(600),
        )
        .await
        .expect("write snapshot");

    let view = world
        .subscribe_as_device(ephemeral.as_ref(), prefix, world.subject_id)
        .await
        .expect("subscription");

    let member = &view.members[0];
    assert!(member.sharing, "the owner is sharing");
    assert_eq!(
        member.connectivity,
        gs_service::gs_domain::membership::Connectivity::Online,
        "presence of the publisher means ONLINE (docs/05 §8)"
    );
    assert!(
        member.last_location_at.is_some(),
        "the snapshot timestamp is exposed as last_location_at"
    );
    assert_eq!(view.snapshot.len(), 1, "the live snapshot is delivered");
    assert_eq!(
        view.snapshot[0].sample.as_ref().map(|s| s.sequence),
        Some(12)
    );
    assert_eq!(
        view.snapshot[0].delivery,
        DeliveryKind::Snapshot as i32,
        "a snapshot read from the store is delivered as SNAPSHOT"
    );
}

#[tokio::test]
async fn test_a_member_that_does_not_share_is_minimized() {
    // covers: FR-PRS-004, INV-PRS-1, docs/05 §8
    let world = setup("minimized").await;
    let ephemeral = InMemoryEphemeralStore::new();
    let prefix = "gs:sub:minimized";

    // A second member, connected but not sharing: nothing about it may leak.
    let other = gs_service::members::add_member(
        &world.pool,
        world.tenant_id,
        world.session_id,
        &gs_service::members::AddMemberRequest {
            external_subject_id: "sub-connected".to_string(),
            role: None,
            capabilities: Some(Capabilities::read_only()),
        },
    )
    .await
    .expect("add member");

    // Its device has presence and even a snapshot: neither may be revealed.
    ephemeral
        .set_presence(
            &format!("{prefix}:presence:{device}", device = world.device_id),
            &[("connection_id", "con_2"), ("instance_id", "i-1")],
            std::time::Duration::from_secs(90),
        )
        .await
        .expect("presence");
    let update = gs_service::gs_protocol::LocationUpdate {
        session_id: world.session_id.to_string(),
        member_id: other.id.to_string(),
        sample: Some(LocationSample {
            sequence: 3,
            ..Default::default()
        }),
        received_at: None,
        delivery: DeliveryKind::Live as i32,
        estimate: None,
    };
    ephemeral
        .set_snapshot(
            &format!(
                "{prefix}:snap:{session}:{member}",
                session = world.session_id,
                member = other.id
            ),
            &update.encode_to_vec(),
            std::time::Duration::from_secs(600),
        )
        .await
        .expect("write snapshot");

    let view = world
        .subscribe_as_device(ephemeral.as_ref(), prefix, world.subject_id)
        .await
        .expect("subscription");

    assert_eq!(view.members.len(), 2, "both members are reported");
    for member in &view.members {
        assert!(
            !member.sharing,
            "no member shares in this scenario (FR-SHR-001)"
        );
        assert_eq!(
            member.connectivity,
            gs_service::gs_domain::membership::Connectivity::Unspecified,
            "INV-PRS-1: a non sharing member never reveals connectivity"
        );
        assert!(
            member.last_location_at.is_none(),
            "INV-PRS-1: no last_location_at without sharing"
        );
    }
    assert!(
        view.snapshot.is_empty(),
        "the snapshot of a member that does not share is not delivered"
    );
}
