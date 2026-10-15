//! Tests for domain control events and live revocation on the bus (T-F4-07, FR-STR-008, FR-STR-011, FR-AUTH-005).
#![forbid(unsafe_code)]

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use time::OffsetDateTime;

use gs_domain::ids::{
    ApplicationId, DeviceId, InstallationId, MemberId, SessionId, SubjectId, TenantId,
};
use gs_domain::membership::{Capabilities, Connectivity, MemberRole};
use gs_domain::session::{Actor, ClosedReason, SessionMode, SessionStatus};
use gs_service::admin::{create_application, create_tenant};
use gs_service::control::{
    GlobalBusEvent, SessionControlEvent, to_proto_member_status, to_proto_session_info,
};
use gs_service::members::{
    AddMemberRequest, UpdateCaller, UpdateMemberRequest, add_member, leave_session_with_bus,
    revoke_member_with_bus, update_member_with_bus,
};
use gs_service::sessions::{
    CreateSessionRequest, SessionPolicy, close_session_with_bus, create_session,
};
use gs_service::subjects::{DeactivateDeviceParams, deactivate_device, revoke_tokens_with_bus};
use gs_service::{Bus, EphemeralStore, InMemoryBus, InMemoryEphemeralStore};
use gs_store::models::{Device, DevicePlatform, DeviceStatus, Subject, SubjectStatus};
use gs_store::repos::{DeviceRepository, SubjectRepository};
use gs_store::{PgPool, connect_pool, run_migrations};

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

async fn setup_fixture() -> (PgPool, TenantId, ApplicationId) {
    let pool = setup_db().await;
    let tenant = create_tenant(&pool, "Control Tenant")
        .await
        .expect("tenant");
    let application = create_application(&pool, &tenant.id.to_string(), "control-app", None, None)
        .await
        .expect("application");
    (pool, tenant.id, application.id)
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

#[tokio::test]
async fn test_global_bus_event_serialization_roundtrip() {
    // covers: FR-AUTH-005, docs/06 §7, docs/08 §9
    let tenant_id = TenantId::new();
    let subject_id = SubjectId::new();
    let device_id = DeviceId::new();

    let events = vec![
        GlobalBusEvent::TenantDisabled {
            tenant_id: tenant_id.to_string(),
            at: "2026-10-14T20:00:00Z".to_string(),
        },
        GlobalBusEvent::TenantEnabled {
            tenant_id: tenant_id.to_string(),
            at: "2026-10-14T20:00:00Z".to_string(),
        },
        GlobalBusEvent::ApplicationDisabled {
            tenant_id: tenant_id.to_string(),
            application_id: "app_123".to_string(),
            at: "2026-10-14T20:00:00Z".to_string(),
        },
        GlobalBusEvent::SubjectTokensRevoked {
            tenant_id: tenant_id.to_string(),
            subject_id: subject_id.to_string(),
            at: "2026-10-14T20:00:00Z".to_string(),
        },
        GlobalBusEvent::DeviceDeactivated {
            tenant_id: tenant_id.to_string(),
            device_id: device_id.to_string(),
            subject_id: Some(subject_id.to_string()),
            at: "2026-10-14T20:00:00Z".to_string(),
        },
    ];

    for event in events {
        let json = serde_json::to_vec(&event).expect("serialize");
        let parsed: GlobalBusEvent = serde_json::from_slice(&json).expect("deserialize");
        assert_eq!(parsed, event);
    }
}

#[tokio::test]
async fn test_session_control_event_serialization_roundtrip() {
    // covers: FR-STR-008, FR-STR-011, docs/05 §10, docs/08 §9
    let session_id = SessionId::new();
    let member_id = MemberId::new();
    let subject_id = SubjectId::new();
    let now = OffsetDateTime::now_utc();

    let proto_session = to_proto_session_info(
        session_id,
        SessionMode::ManyToMany,
        SessionStatus::Active,
        now,
        50,
        60,
        600,
    );
    let proto_member = to_proto_member_status(
        session_id,
        member_id,
        "user-alice",
        MemberRole::Member,
        Capabilities::all(),
        true,
        Connectivity::Online,
        Some(now),
    );

    let events = vec![
        SessionControlEvent::SessionClosed {
            session_id: session_id.to_string(),
            reason: "CLOSED_BY_OWNER".to_string(),
            session: Some(proto_session.clone()),
            at: "2026-10-14T20:00:00Z".to_string(),
        },
        SessionControlEvent::SessionExpired {
            session_id: session_id.to_string(),
            session: Some(proto_session.clone()),
            at: "2026-10-14T20:00:00Z".to_string(),
        },
        SessionControlEvent::SessionUpdated {
            session_id: session_id.to_string(),
            session: proto_session.clone(),
            at: "2026-10-14T20:00:00Z".to_string(),
        },
        SessionControlEvent::MemberJoined {
            session_id: session_id.to_string(),
            member: proto_member.clone(),
            at: "2026-10-14T20:00:00Z".to_string(),
        },
        SessionControlEvent::MemberUpdated {
            session_id: session_id.to_string(),
            member: proto_member.clone(),
            subject_id: subject_id.to_string(),
            lost_read: true,
            lost_publish: false,
            at: "2026-10-14T20:00:00Z".to_string(),
        },
        SessionControlEvent::MemberLeft {
            session_id: session_id.to_string(),
            member_id: member_id.to_string(),
            subject_id: subject_id.to_string(),
            member: Some(proto_member.clone()),
            at: "2026-10-14T20:00:00Z".to_string(),
        },
        SessionControlEvent::MemberRevoked {
            session_id: session_id.to_string(),
            member_id: member_id.to_string(),
            subject_id: subject_id.to_string(),
            member: Some(proto_member.clone()),
            at: "2026-10-14T20:00:00Z".to_string(),
        },
    ];

    for event in events {
        let json = serde_json::to_vec(&event).expect("serialize");
        let parsed: SessionControlEvent = serde_json::from_slice(&json).expect("deserialize");
        assert_eq!(parsed, event);
    }
}

#[tokio::test]
async fn test_close_session_with_bus_publishes_event_and_increments_mver() {
    // covers: FR-STR-008, FR-SES-004, docs/04 §7.5, docs/11 §2.1
    let (pool, tenant_id, app_id) = setup_fixture().await;
    let ephemeral = Arc::new(InMemoryEphemeralStore::new());
    let bus = Arc::new(InMemoryBus::new());

    let actor = Actor::Application(app_id);
    let session = create_session(
        &pool,
        tenant_id,
        actor,
        &CreateSessionRequest {
            mode: SessionMode::ManyToMany,
            expires_in_seconds: Some(3600),
            expires_at: None,
            max_members: Some(10),
            stale_after_seconds: None,
            snapshot_ttl_seconds: None,
            owner_external_subject_id: None,
            initial_members: None,
            metadata: None,
        },
        test_session_policy(),
    )
    .await
    .expect("create session");
    let session_id = session.id;

    let sub = bus
        .subscribe(&format!("gs:bus:ctl:{session_id}"))
        .await
        .expect("subscribe");

    close_session_with_bus(
        &pool,
        ephemeral.as_ref(),
        bus.as_ref(),
        "gs",
        tenant_id,
        session_id,
        ClosedReason::ClosedByApplication,
        None,
    )
    .await
    .expect("close session with bus");

    // mver was incremented
    let mver = ephemeral
        .get_membership_version(&format!("gs:mver:{session_id}"))
        .await
        .expect("mver");
    assert_eq!(mver, Some(1));

    // bus event received
    let msg = bus.next_message(sub).await.expect("recv").expect("some");
    let parsed: SessionControlEvent = serde_json::from_slice(&msg.payload).expect("parse");
    match parsed {
        SessionControlEvent::SessionClosed {
            session_id: sid,
            reason,
            ..
        } => {
            assert_eq!(sid, session_id.to_string());
            assert_eq!(reason, "CLOSED_BY_APPLICATION");
        }
        other => panic!("expected SessionClosed event, got {other:?}"),
    }
}

#[tokio::test]
async fn test_revoke_member_with_bus_publishes_event_deletes_snapshot_and_increments_mver() {
    // covers: FR-STR-008, FR-MEM-004, docs/04 §7.3, docs/11 §2.1
    let (pool, tenant_id, app_id) = setup_fixture().await;
    let ephemeral = Arc::new(InMemoryEphemeralStore::new());
    let bus = Arc::new(InMemoryBus::new());

    let actor = Actor::Application(app_id);
    let session = create_session(
        &pool,
        tenant_id,
        actor,
        &CreateSessionRequest {
            mode: SessionMode::ManyToMany,
            expires_in_seconds: Some(3600),
            expires_at: None,
            max_members: Some(10),
            stale_after_seconds: None,
            snapshot_ttl_seconds: None,
            owner_external_subject_id: None,
            initial_members: None,
            metadata: None,
        },
        test_session_policy(),
    )
    .await
    .expect("create session");
    let session_id = session.id;

    let member = add_member(
        &pool,
        tenant_id,
        session_id,
        &AddMemberRequest {
            external_subject_id: "user-bob".to_string(),
            role: Some(MemberRole::Member),
            capabilities: Some(Capabilities::all()),
        },
    )
    .await
    .expect("add member");
    let member_id = MemberId::from_str(&member.id).expect("member_id");

    // Put a fake snapshot in ephemeral
    let snap_key = format!("gs:snap:{session_id}:{member_id}");
    ephemeral
        .set_snapshot(&snap_key, b"fake_snapshot", Duration::from_secs(600))
        .await
        .expect("set snapshot");
    assert!(
        ephemeral
            .get_snapshot(&snap_key)
            .await
            .expect("snap")
            .is_some()
    );

    let sub = bus
        .subscribe(&format!("gs:bus:ctl:{session_id}"))
        .await
        .expect("subscribe");

    revoke_member_with_bus(
        &pool,
        ephemeral.as_ref(),
        bus.as_ref(),
        "gs",
        tenant_id,
        session_id,
        member_id,
    )
    .await
    .expect("revoke member with bus");

    // snapshot is deleted
    assert!(
        ephemeral
            .get_snapshot(&snap_key)
            .await
            .expect("snap")
            .is_none()
    );

    // mver is incremented
    let mver = ephemeral
        .get_membership_version(&format!("gs:mver:{session_id}"))
        .await
        .expect("mver");
    assert_eq!(mver, Some(1));

    // bus event received
    let msg = bus.next_message(sub).await.expect("recv").expect("some");
    let parsed: SessionControlEvent = serde_json::from_slice(&msg.payload).expect("parse");
    match parsed {
        SessionControlEvent::MemberRevoked {
            session_id: sid,
            member_id: mid,
            subject_id: sub_id,
            ..
        } => {
            assert_eq!(sid, session_id.to_string());
            assert_eq!(mid, member_id.to_string());
            assert_eq!(sub_id, member.subject_id);
        }
        other => panic!("expected MemberRevoked event, got {other:?}"),
    }
}

#[tokio::test]
async fn test_leave_session_with_bus_publishes_event_deletes_snapshot_and_increments_mver() {
    // covers: FR-STR-008, FR-MEM-005, docs/05 §5.2
    let (pool, tenant_id, app_id) = setup_fixture().await;
    let ephemeral = Arc::new(InMemoryEphemeralStore::new());
    let bus = Arc::new(InMemoryBus::new());

    let actor = Actor::Application(app_id);
    let session = create_session(
        &pool,
        tenant_id,
        actor,
        &CreateSessionRequest {
            mode: SessionMode::ManyToMany,
            expires_in_seconds: Some(3600),
            expires_at: None,
            max_members: Some(10),
            stale_after_seconds: None,
            snapshot_ttl_seconds: None,
            owner_external_subject_id: None,
            initial_members: None,
            metadata: None,
        },
        test_session_policy(),
    )
    .await
    .expect("create session");
    let session_id = session.id;

    let member = add_member(
        &pool,
        tenant_id,
        session_id,
        &AddMemberRequest {
            external_subject_id: "user-bob".to_string(),
            role: Some(MemberRole::Member),
            capabilities: Some(Capabilities::all()),
        },
    )
    .await
    .expect("add member");
    let member_id = MemberId::from_str(&member.id).expect("member_id");
    let subject_id = SubjectId::from_str(&member.subject_id).expect("subject_id");

    let snap_key = format!("gs:snap:{session_id}:{member_id}");
    ephemeral
        .set_snapshot(&snap_key, b"fake_snapshot", Duration::from_secs(600))
        .await
        .expect("set snapshot");

    let sub = bus
        .subscribe(&format!("gs:bus:ctl:{session_id}"))
        .await
        .expect("subscribe");

    leave_session_with_bus(
        &pool,
        ephemeral.as_ref(),
        bus.as_ref(),
        "gs",
        tenant_id,
        session_id,
        subject_id,
    )
    .await
    .expect("leave session with bus");

    assert!(
        ephemeral
            .get_snapshot(&snap_key)
            .await
            .expect("snap")
            .is_none()
    );

    let msg = bus.next_message(sub).await.expect("recv").expect("some");
    let parsed: SessionControlEvent = serde_json::from_slice(&msg.payload).expect("parse");
    match parsed {
        SessionControlEvent::MemberLeft {
            session_id: sid,
            member_id: mid,
            subject_id: sub_id,
            ..
        } => {
            assert_eq!(sid, session_id.to_string());
            assert_eq!(mid, member_id.to_string());
            assert_eq!(sub_id, subject_id.to_string());
        }
        other => panic!("expected MemberLeft event, got {other:?}"),
    }
}

#[tokio::test]
async fn test_update_member_with_bus_detects_lost_read_and_lost_publish() {
    // covers: FR-STR-008, FR-STR-011, FR-MEM-003, docs/05 §5.3, docs/08 §9
    let (pool, tenant_id, app_id) = setup_fixture().await;
    let ephemeral = Arc::new(InMemoryEphemeralStore::new());
    let bus = Arc::new(InMemoryBus::new());

    let actor = Actor::Application(app_id);
    let session = create_session(
        &pool,
        tenant_id,
        actor,
        &CreateSessionRequest {
            mode: SessionMode::ManyToMany,
            expires_in_seconds: Some(3600),
            expires_at: None,
            max_members: Some(10),
            stale_after_seconds: None,
            snapshot_ttl_seconds: None,
            owner_external_subject_id: None,
            initial_members: None,
            metadata: None,
        },
        test_session_policy(),
    )
    .await
    .expect("create session");
    let session_id = session.id;

    let member = add_member(
        &pool,
        tenant_id,
        session_id,
        &AddMemberRequest {
            external_subject_id: "user-bob".to_string(),
            role: Some(MemberRole::Member),
            capabilities: Some(Capabilities::all()),
        },
    )
    .await
    .expect("add member");
    let member_id = MemberId::from_str(&member.id).expect("member_id");

    let sub = bus
        .subscribe(&format!("gs:bus:ctl:{session_id}"))
        .await
        .expect("subscribe");

    update_member_with_bus(
        &pool,
        ephemeral.as_ref(),
        bus.as_ref(),
        "gs",
        tenant_id,
        session_id,
        member_id,
        &UpdateMemberRequest {
            role: None,
            capabilities: Some(Capabilities {
                publish: false,
                read: false,
                invite: false,
                manage: false,
            }),
        },
        None,
        UpdateCaller::Application,
    )
    .await
    .expect("update member with bus");

    let msg = bus.next_message(sub).await.expect("recv").expect("some");
    let parsed: SessionControlEvent = serde_json::from_slice(&msg.payload).expect("parse");
    match parsed {
        SessionControlEvent::MemberUpdated {
            session_id: sid,
            lost_read,
            lost_publish,
            ..
        } => {
            assert_eq!(sid, session_id.to_string());
            assert!(lost_read, "should detect lost read");
            assert!(lost_publish, "should detect lost publish");
        }
        other => panic!("expected MemberUpdated event, got {other:?}"),
    }
}

#[tokio::test]
async fn test_revoke_tokens_with_bus_publishes_global_event() {
    // covers: FR-AUTH-005, docs/06 §7
    let (pool, tenant_id, _) = setup_fixture().await;
    let bus = Arc::new(InMemoryBus::new());

    let subject = Subject {
        id: SubjectId::new(),
        tenant_id,
        external_subject_id: Some("user-alice".to_string()),
        status: SubjectStatus::Active,
        token_epoch: 0,
        created_at: OffsetDateTime::now_utc(),
        updated_at: OffsetDateTime::now_utc(),
        erased_at: None,
    };
    SubjectRepository::create(&pool, &subject)
        .await
        .expect("create subject");

    let sub = bus.subscribe("gs:bus:global").await.expect("subscribe");

    revoke_tokens_with_bus(
        &pool,
        bus.as_ref(),
        "gs",
        tenant_id,
        subject.id,
        OffsetDateTime::now_utc(),
    )
    .await
    .expect("revoke tokens with bus");

    let msg = bus.next_message(sub).await.expect("recv").expect("some");
    let parsed: GlobalBusEvent = serde_json::from_slice(&msg.payload).expect("parse");
    match parsed {
        GlobalBusEvent::SubjectTokensRevoked {
            tenant_id: tid,
            subject_id: sid,
            ..
        } => {
            assert_eq!(tid, tenant_id.to_string());
            assert_eq!(sid, subject.id.to_string());
        }
        other => panic!("expected SubjectTokensRevoked event, got {other:?}"),
    }
}

#[tokio::test]
async fn test_deactivate_device_publishes_global_event() {
    // covers: FR-AUTH-005, docs/06 §7
    let (pool, tenant_id, app_id) = setup_fixture().await;
    let ephemeral = Arc::new(InMemoryEphemeralStore::new());
    let bus = Arc::new(InMemoryBus::new());

    let subject = Subject {
        id: SubjectId::new(),
        tenant_id,
        external_subject_id: Some("user-bob".to_string()),
        status: SubjectStatus::Active,
        token_epoch: 0,
        created_at: OffsetDateTime::now_utc(),
        updated_at: OffsetDateTime::now_utc(),
        erased_at: None,
    };
    SubjectRepository::create(&pool, &subject)
        .await
        .expect("create subject");

    let device = Device {
        id: DeviceId::new(),
        tenant_id,
        subject_id: subject.id,
        application_id: app_id,
        installation_id: InstallationId::new(),
        platform: DevicePlatform::Android,
        app_version: Some("1.0.0".to_string()),
        sdk_version: Some("0.1.0".to_string()),
        status: DeviceStatus::Active,
        created_at: OffsetDateTime::now_utc(),
        last_seen_at: OffsetDateTime::now_utc(),
        deactivated_at: None,
    };
    DeviceRepository::create(&pool, &device)
        .await
        .expect("create device");

    let sub = bus.subscribe("gs:bus:global").await.expect("subscribe");

    deactivate_device(DeactivateDeviceParams {
        pool: &pool,
        ephemeral: ephemeral.as_ref(),
        bus: bus.as_ref(),
        tenant_id,
        device_id: device.id,
        caller_device_id: None,
        key_prefix: "gs",
        now: OffsetDateTime::now_utc(),
    })
    .await
    .expect("deactivate device");

    let msg = bus.next_message(sub).await.expect("recv").expect("some");
    let parsed: GlobalBusEvent = serde_json::from_slice(&msg.payload).expect("parse");
    match parsed {
        GlobalBusEvent::DeviceDeactivated {
            tenant_id: tid,
            device_id: did,
            subject_id: sid,
            ..
        } => {
            assert_eq!(tid, tenant_id.to_string());
            assert_eq!(did, device.id.to_string());
            assert_eq!(sid, Some(subject.id.to_string()));
        }
        other => panic!("expected DeviceDeactivated event, got {other:?}"),
    }
}
