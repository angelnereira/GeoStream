//! Publishing use case against PostgreSQL: preconditions, per-sample algorithm, sequence CAS,
//! snapshot and rate limit (T-F4-05, FR-LOC-001..008, FR-STR-004, docs/08 §6.4 and §7,
//! docs/09 §3–§4, ADR-0008).
#![forbid(unsafe_code)]

use std::time::Duration;

use gs_domain::location::LocationPolicy;
use gs_service::admin::{create_api_key, create_application, create_tenant};
use gs_service::gs_auth::{generate_signing_key, parse_private_pem};
use gs_service::gs_protocol::{
    DeliveryKind, LocationKind, LocationQuality, LocationSample, MotionActivity, Position,
};
use gs_service::publishing::{PublishCaller, PublishError, PublishParams, PublishThrottle};
use gs_service::sessions::{CreateSessionRequest, SessionPolicy, create_session};
use gs_service::subscriptions::{SubscribeParams, SubscribePrincipal, subscribe};
use gs_service::tokens::{TokenRequest, TokenTtlBounds, mint_device_token};
use gs_service::{EphemeralStore, InMemoryEphemeralStore};
use gs_store::repos::MembershipRepository;
use gs_store::{PgPool, connect_pool, run_migrations};
use pbjson_types::Timestamp;
use time::OffsetDateTime;

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

/// Tenant, application, a device that owns and shares a session, and a second device.
struct World {
    pool: PgPool,
    tenant_id: gs_domain::ids::TenantId,
    session_id: gs_domain::ids::SessionId,
    owner_subject: gs_domain::ids::SubjectId,
    owner_device: gs_domain::ids::DeviceId,
    other_device: gs_domain::ids::DeviceId,
    outsider_subject: gs_domain::ids::SubjectId,
    snapshot_ttl_seconds: u32,
}

async fn setup(suffix: &str) -> World {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");

    let tenant = create_tenant(&pool, &format!("Pub Tenant {suffix}"))
        .await
        .expect("tenant");
    let application = create_application(
        &pool,
        &tenant.id.to_string(),
        &format!("pub-app-{suffix}"),
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
        Some("pub"),
        None,
    )
    .await
    .expect("api key");

    let kid = format!("pub-{suffix}");
    let generated = generate_signing_key(&kid).expect("generate key");
    let signing = parse_private_pem(generated.private_pem()).expect("parse key");

    let mint = |external: String, installation: String| {
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
                    installation_id: installation,
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

    let owner = mint(
        format!("pub-owner-{suffix}"),
        "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d11".to_string(),
    )
    .await;
    let other = mint(
        format!("pub-other-{suffix}"),
        "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d22".to_string(),
    )
    .await;
    let outsider = mint(
        format!("pub-outsider-{suffix}"),
        "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d33".to_string(),
    )
    .await;

    let session = create_session(
        &pool,
        tenant.id,
        gs_domain::session::Actor::Device(owner.device.id),
        &serde_json::from_value::<CreateSessionRequest>(serde_json::json!({
            "mode": "MANY_TO_MANY",
            "max_members": 10,
            "snapshot_ttl_seconds": 600
        }))
        .expect("request"),
        SessionPolicy::from_policy(&gs_service::gs_auth::TenantPolicy::default()),
    )
    .await
    .expect("create session");

    // The owner starts sharing, so it may publish (FR-SHR-002).
    let membership = MembershipRepository::find_active_by_session_and_subject(
        &pool,
        tenant.id,
        session.id,
        owner.subject.id,
    )
    .await
    .expect("query")
    .expect("membership");
    gs_service::members::set_sharing(
        &pool,
        tenant.id,
        session.id,
        membership.id,
        true,
        gs_service::members::SetSharingContext {
            ephemeral: &InMemoryEphemeralStore::new(),
            bus: &gs_service::InMemoryBus::new(),
            key_prefix: &format!("gs:pub:{suffix}"),
            caller: gs_service::members::SetSharingCaller {
                is_app: false,
                subject_id: Some(owner.subject.id),
                device_id: Some(owner.device.id),
                has_manage: true,
            },
        },
    )
    .await
    .expect("start sharing");

    World {
        pool,
        tenant_id: tenant.id,
        session_id: session.id,
        owner_subject: owner.subject.id,
        owner_device: owner.device.id,
        other_device: other.device.id,
        outsider_subject: outsider.subject.id,
        snapshot_ttl_seconds: 600,
    }
}

/// A valid sample for the synthetic coordinates of the vectors.
fn sample(sequence: u64, age_seconds: i64) -> LocationSample {
    LocationSample {
        sequence,
        captured_at: Some(Timestamp {
            seconds: OffsetDateTime::now_utc().unix_timestamp() - age_seconds,
            nanos: 0,
        }),
        position: Some(Position {
            latitude: 8.9824,
            longitude: -79.5199,
            horizontal_accuracy_m: 8.5,
            altitude_m: None,
            vertical_accuracy_m: None,
            speed_mps: Some(4.2),
            speed_accuracy_mps: None,
            heading_deg: Some(91.0),
            heading_accuracy_deg: None,
        }),
        quality: Some(LocationQuality {
            source: gs_service::gs_protocol::LocationSource::Gnss as i32,
            kind: LocationKind::Measured as i32,
            is_mock: Some(false),
            motion: MotionActivity::Automotive as i32,
        }),
    }
}

/// A sample that fails a pure rule of docs/09 §3.1.
fn invalid_sample(sequence: u64) -> LocationSample {
    let mut broken = sample(sequence, 0);
    if let Some(position) = broken.position.as_mut() {
        position.latitude = 91.0;
    }
    broken
}

/// An old sample: ignored as stale, not rejected (docs/09 §3.2).
fn stale_sample(sequence: u64) -> LocationSample {
    sample(sequence, 600)
}

struct Publish<'a> {
    samples: &'a [LocationSample],
    device: gs_domain::ids::DeviceId,
    subject: gs_domain::ids::SubjectId,
    policy: LocationPolicy,
}

impl<'a> Publish<'a> {
    /// A publish with the default policy of docs/17 §4.
    fn new(
        samples: &'a [LocationSample],
        device: gs_domain::ids::DeviceId,
        subject: gs_domain::ids::SubjectId,
    ) -> Self {
        Self {
            samples,
            device,
            subject,
            policy: LocationPolicy::default(),
        }
    }
}

async fn publish(
    world: &World,
    ephemeral: &InMemoryEphemeralStore,
    prefix: &str,
    request: Publish<'_>,
) -> Result<gs_service::publishing::PublishOutcome, PublishError> {
    let mut throttle =
        PublishThrottle::new(5, Duration::from_millis(1_000), OffsetDateTime::now_utc());
    gs_service::publishing::publish_batch(PublishParams {
        pool: &world.pool,
        ephemeral,
        key_prefix: prefix,
        tenant_id: world.tenant_id,
        session_id: world.session_id,
        caller: PublishCaller {
            subject_id: request.subject,
            device_id: request.device,
        },
        samples: request.samples,
        policy: request.policy,
        throttle: &mut throttle,
        now: OffsetDateTime::now_utc(),
    })
    .await
}

#[tokio::test]
async fn test_fr_loc_003_a_batch_is_accepted_ordered_and_deduplicated() {
    // covers: FR-LOC-003, FR-STR-004, docs/09 §4, ADR-0008
    let world = setup("order").await;
    let ephemeral = InMemoryEphemeralStore::new();
    let prefix = "gs:pub:order";

    let samples = vec![sample(1, 5), sample(2, 5), sample(3, 5)];
    let outcome = publish(
        &world,
        &ephemeral,
        prefix,
        Publish::new(&samples, world.owner_device, world.owner_subject),
    )
    .await
    .expect("publish");

    assert_eq!(
        outcome.results.len(),
        3,
        "one result per sample, same order (FR-STR-004)"
    );
    assert!(
        outcome
            .results
            .iter()
            .all(|result| result.status == gs_domain::location::SampleStatus::Accepted)
    );
    assert_eq!(outcome.last_sequence, Some(3));
    assert_eq!(outcome.accepted.len(), 3);

    // FR-LOC-005: the server assigns `received_at`, never the client.
    let received = outcome.accepted[0].received_at.expect("received_at");
    let captured = outcome.accepted[0]
        .sample
        .as_ref()
        .expect("sample")
        .captured_at
        .expect("captured_at");
    assert_ne!(
        received.seconds, captured.seconds,
        "received_at is the server clock"
    );

    // FR-LOC-006: the sample is delivered untouched.
    assert_eq!(
        outcome.accepted[0]
            .sample
            .as_ref()
            .expect("sample")
            .position
            .as_ref()
            .expect("position")
            .latitude,
        8.9824
    );
    assert_eq!(outcome.accepted[0].delivery, DeliveryKind::Live as i32);

    // The CAS of docs/09 §4: a repeated sequence is a duplicate, an older one is stale.
    // Each case travels in its own publish, because a batch must be strictly increasing.
    for (sequence, expected) in [
        (3, "IGNORED_DUPLICATE"),
        (2, "IGNORED_STALE"),
        (4, "ACCEPTED"),
    ] {
        let again = publish(
            &world,
            &ephemeral,
            prefix,
            Publish::new(
                &[sample(sequence, 5)],
                world.owner_device,
                world.owner_subject,
            ),
        )
        .await
        .expect("publish");
        assert_eq!(
            again.results[0].status.to_string(),
            expected,
            "sequence {sequence}"
        );
        if expected == "ACCEPTED" {
            assert_eq!(again.last_sequence, Some(4));
        }
    }

    // FR-LOC-008: the device sequence floor is lifted for the hello of the next connection.
    let floor = ephemeral
        .get_counter(&format!(
            "{prefix}:devseq:{device}",
            device = world.owner_device
        ))
        .await
        .expect("read floor");
    assert_eq!(floor, Some(4));
}

#[tokio::test]
async fn test_fr_loc_001_an_invalid_sample_is_rejected_with_field() {
    // covers: FR-LOC-001, FR-LOC-002, docs/09 §3.1
    let world = setup("invalid").await;
    let ephemeral = InMemoryEphemeralStore::new();

    let outcome = publish(
        &world,
        &ephemeral,
        "gs:pub:invalid",
        Publish::new(
            &[invalid_sample(7)],
            world.owner_device,
            world.owner_subject,
        ),
    )
    .await
    .expect("publish");

    let result = &outcome.results[0];
    assert_eq!(
        result.status,
        gs_domain::location::SampleStatus::Rejected,
        "an out-of-range latitude is rejected, not accepted"
    );
    assert_eq!(result.error_code.as_deref(), Some("INVALID_LOCATION"));
    assert_eq!(result.field.as_deref(), Some("position.latitude"));
    assert!(outcome.accepted.is_empty());
    assert_eq!(outcome.last_sequence, None);
}

#[tokio::test]
async fn test_fr_loc_004_an_old_sample_is_ignored_as_stale() {
    // covers: FR-LOC-004, docs/09 §3.2
    let world = setup("stale").await;
    let ephemeral = InMemoryEphemeralStore::new();

    let outcome = publish(
        &world,
        &ephemeral,
        "gs:pub:stale",
        Publish::new(&[stale_sample(9)], world.owner_device, world.owner_subject),
    )
    .await
    .expect("publish");
    assert_eq!(
        outcome.results[0].status,
        gs_domain::location::SampleStatus::IgnoredStale,
        "older than max_event_age_ms is stale, not an error"
    );
    assert!(outcome.accepted.is_empty());
}

#[tokio::test]
async fn test_the_snapshot_of_the_member_is_written_on_acceptance() {
    // covers: FR-LOC-005, docs/09 §4, docs/11 §3
    let world = setup("snapshot").await;
    let ephemeral = InMemoryEphemeralStore::new();
    let prefix = "gs:pub:snapshot";

    assert_eq!(world.snapshot_ttl_seconds, 600);
    publish(
        &world,
        &ephemeral,
        prefix,
        Publish::new(&[sample(21, 0)], world.owner_device, world.owner_subject),
    )
    .await
    .expect("publish");

    // A subscription now sees the member ONLINE... well, with last_location_at from the snapshot.
    let membership = MembershipRepository::find_active_by_session_and_subject(
        &world.pool,
        world.tenant_id,
        world.session_id,
        world.owner_subject,
    )
    .await
    .expect("query")
    .expect("membership");
    let view = subscribe(SubscribeParams {
        pool: &world.pool,
        ephemeral: &ephemeral,
        key_prefix: prefix,
        tenant_id: world.tenant_id,
        session_id: world.session_id,
        principal: SubscribePrincipal::Device {
            subject_id: world.owner_subject,
            device_id: world.owner_device,
        },
        now: OffsetDateTime::now_utc(),
    })
    .await
    .expect("subscribe");

    assert_eq!(
        view.snapshot.len(),
        1,
        "the accepted sample is the live snapshot"
    );
    assert_eq!(view.snapshot[0].delivery, DeliveryKind::Snapshot as i32);
    let member = &view.members[0];
    assert!(
        member.last_location_at.is_some(),
        "the snapshot timestamp becomes last_location_at"
    );
    assert_eq!(member.member_id, membership.id);
}

#[tokio::test]
async fn test_fr_shr_preconditions_of_publication() {
    // covers: FR-SHR-002, FR-SHR-004, docs/08 §6.4
    let world = setup("preconditions").await;
    let ephemeral = InMemoryEphemeralStore::new();

    // A device that is not the publisher of the membership.
    let error = publish(
        &world,
        &ephemeral,
        "gs:pub:preconditions",
        Publish::new(&[sample(31, 0)], world.other_device, world.owner_subject),
    )
    .await
    .expect_err("another device cannot publish for that membership");
    assert!(matches!(error, PublishError::NotPublisherDevice));

    // A subject without an active membership.
    let error = publish(
        &world,
        &ephemeral,
        "gs:pub:preconditions",
        Publish::new(&[sample(32, 0)], world.other_device, world.outsider_subject),
    )
    .await
    .expect_err("a non member cannot publish");
    assert!(matches!(error, PublishError::NotFound));

    // An empty batch and non increasing sequences fail the batch preconditions.
    let error = publish(
        &world,
        &ephemeral,
        "gs:pub:preconditions",
        Publish::new(&[], world.owner_device, world.owner_subject),
    )
    .await
    .expect_err("an empty batch is refused");
    assert!(matches!(error, PublishError::ValidationFailed));

    let error = publish(
        &world,
        &ephemeral,
        "gs:pub:preconditions",
        Publish::new(
            &[sample(40, 0), sample(40, 0)],
            world.owner_device,
            world.owner_subject,
        ),
    )
    .await
    .expect_err("sequences must be strictly increasing");
    assert!(matches!(error, PublishError::ValidationFailed));

    // More samples than the policy allows.
    let many: Vec<LocationSample> = (1..=4).map(|sequence| sample(sequence, 0)).collect();
    let error = publish(
        &world,
        &ephemeral,
        "gs:pub:preconditions",
        Publish {
            samples: &many,
            device: world.owner_device,
            subject: world.owner_subject,
            // The tenant policy of the fixture allows two samples per publish.
            policy: LocationPolicy {
                max_samples_per_publish: 2,
                ..LocationPolicy::default()
            },
        },
    )
    .await
    .expect_err("a batch above the limit is refused");
    assert!(matches!(error, PublishError::PayloadTooLarge));
}

#[tokio::test]
async fn test_fr_loc_007_the_token_bucket_rate_limits_publication() {
    // covers: FR-LOC-007, docs/08 §7.4
    let world = setup("throttle").await;
    let ephemeral = InMemoryEphemeralStore::new();
    let prefix = "gs:pub:throttle";

    let mut throttle =
        PublishThrottle::new(2, Duration::from_millis(50), OffsetDateTime::now_utc());

    // The burst allows two publishes; the third is rate limited.
    for expected in 1..=2 {
        let outcome = gs_service::publishing::publish_batch(PublishParams {
            pool: &world.pool,
            ephemeral: &ephemeral,
            key_prefix: prefix,
            tenant_id: world.tenant_id,
            session_id: world.session_id,
            caller: PublishCaller {
                subject_id: world.owner_subject,
                device_id: world.owner_device,
            },
            samples: &[sample(expected * 10, 0)],
            policy: LocationPolicy::default(),
            throttle: &mut throttle,
            now: OffsetDateTime::now_utc(),
        })
        .await
        .expect("publish inside the burst");
        assert_eq!(outcome.results.len(), 1);
    }

    let error = gs_service::publishing::publish_batch(PublishParams {
        pool: &world.pool,
        ephemeral: &ephemeral,
        key_prefix: prefix,
        tenant_id: world.tenant_id,
        session_id: world.session_id,
        caller: PublishCaller {
            subject_id: world.owner_subject,
            device_id: world.owner_device,
        },
        samples: &[sample(99, 0)],
        policy: LocationPolicy::default(),
        throttle: &mut throttle,
        now: OffsetDateTime::now_utc(),
    })
    .await
    .expect_err("the burst is exhausted");
    match error {
        PublishError::RateLimited { retry_after_ms } => {
            assert!(retry_after_ms > 0, "retry_after_ms must be positive");
            assert!(retry_after_ms <= 50);
        }
        other => panic!("expected RateLimited, got {other:?}"),
    }
}

#[tokio::test]
async fn test_a_closed_session_refuses_publication() {
    // covers: INV-SES-1, docs/08 §6.4
    let world = setup("closed").await;
    let ephemeral = InMemoryEphemeralStore::new();

    gs_service::sessions::close_session(
        &world.pool,
        world.tenant_id,
        world.session_id,
        gs_domain::session::ClosedReason::ClosedByMember,
    )
    .await
    .expect("close session");

    let error = publish(
        &world,
        &ephemeral,
        "gs:pub:closed",
        Publish::new(&[sample(51, 0)], world.owner_device, world.owner_subject),
    )
    .await
    .expect_err("a closed session refuses publications");
    assert!(matches!(error, PublishError::SessionClosed));
}
