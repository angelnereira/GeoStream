//! Integration tests for sensitive action auditing (T-F3-09, FR-PRV-005, FR-SHR-005, docs/10 §8.2, §8.3).
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::str::FromStr;
use time::OffsetDateTime;

use gs_domain::ids::{DeviceId, MemberId, SubjectId, TenantId};
use gs_domain::membership::{Capabilities, MemberRole};
use gs_domain::session::{Actor, ClosedReason, SessionMode};
use gs_service::admin::{create_application, create_tenant};
use gs_service::audit::{ActorKind, AuditOutcome, AuditRepository};
use gs_service::invitations::{
    CreateInvitationCaller, CreateInvitationRequest, RedeemInvitationParams,
    RevokeInvitationCaller, create_invitation, redeem_invitation, revoke_invitation,
};
use gs_service::members::{
    AddMemberRequest, SetSharingCaller, SetSharingContext, UpdateCaller, UpdateMemberRequest,
    add_member, leave_session, revoke_member, set_sharing, update_member,
};
use gs_service::sessions::{
    CreateSessionRequest, SessionPolicy, close_session_with_actor, create_session,
    expire_due_sessions, update_session_with_actor,
};
use gs_service::subjects::{DeactivateDeviceParams, deactivate_device, revoke_subject_tokens};
use gs_service::{InMemoryBus, InMemoryEphemeralStore};
use gs_store::models::{Device, DevicePlatform, DeviceStatus};
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

fn session_policy() -> SessionPolicy {
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

/// Recursively verifies that a JSON Value does NOT contain any forbidden fields
/// per docs/10 §8.2 (coordinates, tokens, tickets, secrets, invitation codes, metadata, external_subject_id).
fn assert_no_forbidden_fields(val: &serde_json::Value, path: &str) {
    const FORBIDDEN_KEYS: &[&str] = &[
        "lat",
        "lon",
        "latitude",
        "longitude",
        "coordinates",
        "position",
        "accuracy",
        "altitude",
        "speed",
        "heading",
        "access_token",
        "token",
        "bearer",
        "ticket",
        "secret",
        "key_secret",
        "api_key",
        "invitation_code",
        "code",
        "metadata",
        "external_subject_id",
    ];

    match val {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                let current_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                let lower_key = key.to_ascii_lowercase();
                for forbidden in FORBIDDEN_KEYS {
                    assert_ne!(
                        &lower_key, forbidden,
                        "Forbidden key '{forbidden}' found in audit details at path '{current_path}' (docs/10 §8.2)"
                    );
                }
                assert_no_forbidden_fields(child, &current_path);
            }
        }
        serde_json::Value::Array(arr) => {
            for (i, child) in arr.iter().enumerate() {
                let current_path = format!("{path}[{i}]");
                assert_no_forbidden_fields(child, &current_path);
            }
        }
        _ => {}
    }
}

/// Helper to create a test device and subject in the database.
async fn setup_test_device(
    pool: &PgPool,
    tenant_id: TenantId,
    app_id: gs_domain::ids::ApplicationId,
    suffix: &str,
) -> (DeviceId, SubjectId) {
    let now = OffsetDateTime::now_utc();
    let ext_id = format!("usr_{suffix}");
    let subject = SubjectRepository::get_or_create_active(pool, tenant_id, &ext_id, now)
        .await
        .expect("create subject");
    let dev_id = DeviceId::new();
    let device = Device {
        id: dev_id,
        tenant_id,
        subject_id: subject.id,
        application_id: app_id,
        installation_id: gs_domain::ids::InstallationId::new(),
        platform: DevicePlatform::Android,
        app_version: Some("1.0.0".to_string()),
        sdk_version: Some("0.1.0".to_string()),
        status: DeviceStatus::Active,
        created_at: now,
        last_seen_at: now,
        deactivated_at: None,
    };
    DeviceRepository::create(pool, &device)
        .await
        .expect("create device");

    (dev_id, subject.id)
}

#[tokio::test]
async fn test_fr_prv_005_and_fr_shr_005_sensitive_actions_audited() {
    let pool = setup_db().await;
    let ephemeral = InMemoryEphemeralStore::new();
    let bus = InMemoryBus::new();

    let tenant = create_tenant(&pool, "Audit Test Tenant")
        .await
        .expect("create tenant");

    let app = create_application(&pool, &tenant.id.to_string(), "audit-app", None, None)
        .await
        .expect("create app");

    let (dev1_id, sub1_id) = setup_test_device(&pool, tenant.id, app.id, "alice").await;
    let (dev2_id, sub2_id) = setup_test_device(&pool, tenant.id, app.id, "bob").await;

    // 1. Session created
    let mut metadata = BTreeMap::new();
    metadata.insert("environment".to_string(), "staging".to_string());
    let create_req = CreateSessionRequest {
        mode: SessionMode::ManyToMany,
        expires_in_seconds: Some(3600),
        expires_at: None,
        max_members: Some(10),
        snapshot_ttl_seconds: Some(30),
        stale_after_seconds: Some(30),
        metadata: Some(metadata),
        owner_external_subject_id: None,
        initial_members: None,
    };

    let session = create_session(
        &pool,
        tenant.id,
        Actor::Device(dev1_id),
        &create_req,
        session_policy(),
    )
    .await
    .expect("create session");

    // Verify session.created audit event
    let created_events = AuditRepository::list_by_action(&pool, tenant.id, "session.created", 10)
        .await
        .expect("list session.created");
    assert_eq!(created_events.len(), 1);
    let s_created = &created_events[0];
    assert_eq!(s_created.actor_kind, ActorKind::Device);
    assert_eq!(s_created.actor_id, Some(dev1_id.into_uuid()));
    assert_eq!(s_created.subject_id, Some(sub1_id));
    assert_eq!(s_created.target_kind.as_deref(), Some("session"));
    assert_eq!(s_created.target_id, Some(session.id.into_uuid()));
    assert_eq!(s_created.outcome, AuditOutcome::Success);
    assert_eq!(s_created.details["mode"], "MANY_TO_MANY");
    assert_eq!(s_created.details["max_members"], 10);
    assert_no_forbidden_fields(&s_created.details, "");

    // Also verify owner member.added audit event
    let member_added_events = AuditRepository::list_by_action(&pool, tenant.id, "member.added", 10)
        .await
        .expect("list member.added");
    assert_eq!(member_added_events.len(), 1);
    assert_eq!(member_added_events[0].details["role"], "OWNER");
    assert_no_forbidden_fields(&member_added_events[0].details, "");

    // 2. Session updated
    let update_req = gs_service::sessions::UpdateSessionRequest {
        expires_at: None,
        max_members: Some(15),
        snapshot_ttl_seconds: None,
        stale_after_seconds: None,
        metadata: None,
    };
    let updated = update_session_with_actor(
        &pool,
        tenant.id,
        session.id,
        Some(session.version),
        &update_req,
        session_policy(),
        Some(Actor::Device(dev1_id)),
    )
    .await
    .expect("update session");
    assert_eq!(updated.max_members, 15);

    let updated_events = AuditRepository::list_by_action(&pool, tenant.id, "session.updated", 10)
        .await
        .expect("list session.updated");
    assert_eq!(updated_events.len(), 1);
    assert_eq!(updated_events[0].actor_kind, ActorKind::Device);
    assert_eq!(updated_events[0].details["max_members"], 15);
    assert_no_forbidden_fields(&updated_events[0].details, "");

    // 3. Add member directly (FR-MEM-001)
    let add_req = AddMemberRequest {
        external_subject_id: "usr_bob".to_string(),
        role: Some(MemberRole::Member),
        capabilities: Some(Capabilities {
            publish: true,
            read: true,
            invite: true,
            manage: false,
        }),
    };
    let bob_member = add_member(&pool, tenant.id, session.id, &add_req)
        .await
        .expect("add member bob");
    let bob_member_id = MemberId::from_str(&bob_member.id).expect("parse member id");

    let member_added_events = AuditRepository::list_by_action(&pool, tenant.id, "member.added", 10)
        .await
        .expect("list member.added");
    assert_eq!(member_added_events.len(), 2); // Alice owner + Bob member
    assert_eq!(member_added_events[0].subject_id, Some(sub2_id));
    assert_eq!(member_added_events[0].actor_kind, ActorKind::Application);
    assert_eq!(member_added_events[0].details["role"], "MEMBER");
    assert_eq!(member_added_events[0].details["can_publish"], true);
    assert_no_forbidden_fields(&member_added_events[0].details, "");

    // 4. Update member capabilities (FR-MEM-003)
    let update_m_req = UpdateMemberRequest {
        role: None,
        capabilities: Some(Capabilities {
            publish: true,
            read: true,
            invite: false,
            manage: false,
        }),
    };
    update_member(
        &pool,
        tenant.id,
        session.id,
        bob_member_id,
        None,
        &update_m_req,
        UpdateCaller::Application,
    )
    .await
    .expect("update member capabilities");

    let member_updated_events =
        AuditRepository::list_by_action(&pool, tenant.id, "member.updated", 10)
            .await
            .expect("list member.updated");
    assert_eq!(member_updated_events.len(), 1);
    assert_eq!(member_updated_events[0].actor_kind, ActorKind::Application);
    assert_eq!(member_updated_events[0].details["can_invite"], false);
    assert_no_forbidden_fields(&member_updated_events[0].details, "");

    // 5. FR-SHR-005: Location sharing started and stopped (PUT .../sharing)
    let ctx = SetSharingContext {
        caller: SetSharingCaller {
            subject_id: Some(sub2_id),
            device_id: Some(dev2_id),
            is_app: false,
            has_manage: false,
        },
        ephemeral: &ephemeral,
        bus: &bus,
        key_prefix: "gs",
    };
    set_sharing(&pool, tenant.id, session.id, bob_member_id, true, ctx)
        .await
        .expect("start sharing");

    let sharing_started = AuditRepository::list_by_action(&pool, tenant.id, "sharing.started", 10)
        .await
        .expect("list sharing.started");
    assert_eq!(sharing_started.len(), 1);
    assert_eq!(sharing_started[0].actor_kind, ActorKind::Device);
    assert_eq!(sharing_started[0].actor_id, Some(dev2_id.into_uuid()));
    assert_eq!(sharing_started[0].subject_id, Some(sub2_id));
    assert_eq!(
        sharing_started[0].target_id,
        Some(bob_member_id.into_uuid())
    );
    assert_eq!(sharing_started[0].details["sharing"], true);
    assert_eq!(
        sharing_started[0].details["publisher_device_id"],
        dev2_id.to_string()
    );
    assert_no_forbidden_fields(&sharing_started[0].details, "");

    // Stop sharing
    let ctx_stop = SetSharingContext {
        caller: SetSharingCaller {
            subject_id: Some(sub2_id),
            device_id: Some(dev2_id),
            is_app: false,
            has_manage: false,
        },
        ephemeral: &ephemeral,
        bus: &bus,
        key_prefix: "gs",
    };
    set_sharing(&pool, tenant.id, session.id, bob_member_id, false, ctx_stop)
        .await
        .expect("stop sharing");

    let sharing_stopped = AuditRepository::list_by_action(&pool, tenant.id, "sharing.stopped", 10)
        .await
        .expect("list sharing.stopped");
    assert_eq!(sharing_stopped.len(), 1);
    assert_eq!(sharing_stopped[0].actor_kind, ActorKind::Device);
    assert_eq!(sharing_stopped[0].details["sharing"], false);
    assert_no_forbidden_fields(&sharing_stopped[0].details, "");

    // 6. Invitations: create, revoke, redeem (FR-INV-001..003)
    let now = OffsetDateTime::now_utc();
    let effective_policy = gs_service::admin::get_effective_policy(&pool, &tenant.id.to_string())
        .await
        .expect("policy");

    let inv_created = create_invitation(
        &pool,
        tenant.id,
        session.id,
        &CreateInvitationRequest {
            grants: Some(Capabilities {
                publish: true,
                read: true,
                invite: false,
                manage: false,
            }),
            max_uses: Some(2),
            expires_in_seconds: Some(1800),
        },
        CreateInvitationCaller::Application,
        &effective_policy,
        now,
    )
    .await
    .expect("create invitation");

    let inv_id =
        gs_domain::ids::InvitationId::from_str(&inv_created.invitation.id).expect("inv id");

    let inv_created_events =
        AuditRepository::list_by_action(&pool, tenant.id, "invitation.created", 10)
            .await
            .expect("list invitation.created");
    assert_eq!(inv_created_events.len(), 1);
    assert_eq!(inv_created_events[0].actor_kind, ActorKind::Application);
    assert_eq!(inv_created_events[0].details["max_uses"], 2);
    assert_no_forbidden_fields(&inv_created_events[0].details, "");

    // Redeem invitation with third subject (Charlie)
    let (dev3_id, sub3_id) = setup_test_device(&pool, tenant.id, app.id, "charlie").await;
    let redeemed = redeem_invitation(RedeemInvitationParams {
        pool: &pool,
        ephemeral: &ephemeral,
        key_prefix: "gs",
        tenant_id: tenant.id,
        code: &inv_created.code,
        device_id: dev3_id,
        subject_id: sub3_id,
        rate_limit_rpm: 60,
        now: OffsetDateTime::now_utc(),
    })
    .await
    .expect("redeem invitation");

    let inv_redeemed_events =
        AuditRepository::list_by_action(&pool, tenant.id, "invitation.redeemed", 10)
            .await
            .expect("list invitation.redeemed");
    assert_eq!(inv_redeemed_events.len(), 1);
    assert_eq!(inv_redeemed_events[0].actor_kind, ActorKind::Device);
    assert_eq!(inv_redeemed_events[0].actor_id, Some(dev3_id.into_uuid()));
    assert_eq!(inv_redeemed_events[0].subject_id, Some(sub3_id));
    assert_eq!(inv_redeemed_events[0].target_id, Some(inv_id.into_uuid()));
    assert_no_forbidden_fields(&inv_redeemed_events[0].details, "");

    // Revoke invitation
    revoke_invitation(
        &pool,
        tenant.id,
        session.id,
        inv_id,
        RevokeInvitationCaller::Application,
        OffsetDateTime::now_utc(),
    )
    .await
    .expect("revoke invitation");

    let inv_revoked_events =
        AuditRepository::list_by_action(&pool, tenant.id, "invitation.revoked", 10)
            .await
            .expect("list invitation.revoked");
    assert_eq!(inv_revoked_events.len(), 1);
    assert_eq!(inv_revoked_events[0].actor_kind, ActorKind::Application);
    assert_eq!(inv_revoked_events[0].target_id, Some(inv_id.into_uuid()));
    assert_no_forbidden_fields(&inv_revoked_events[0].details, "");

    // 7. Member left and Member revoked
    let charlie_member_id = MemberId::from_str(&redeemed.member.id).expect("charlie member id");
    leave_session(&pool, tenant.id, session.id, sub3_id)
        .await
        .expect("charlie leaves");

    let member_left_events = AuditRepository::list_by_action(&pool, tenant.id, "member.left", 10)
        .await
        .expect("list member.left");
    assert_eq!(member_left_events.len(), 1);
    assert_eq!(member_left_events[0].subject_id, Some(sub3_id));
    assert_eq!(
        member_left_events[0].target_id,
        Some(charlie_member_id.into_uuid())
    );
    assert_no_forbidden_fields(&member_left_events[0].details, "");

    // Revoke Bob
    revoke_member(&pool, tenant.id, session.id, bob_member_id)
        .await
        .expect("revoke bob");

    let member_revoked_events =
        AuditRepository::list_by_action(&pool, tenant.id, "member.revoked", 10)
            .await
            .expect("list member.revoked");
    assert_eq!(member_revoked_events.len(), 1);
    assert_eq!(member_revoked_events[0].subject_id, Some(sub2_id));
    assert_eq!(
        member_revoked_events[0].target_id,
        Some(bob_member_id.into_uuid())
    );
    assert_no_forbidden_fields(&member_revoked_events[0].details, "");

    // 8. Subject tokens revoked and Device deactivated
    let revoked_sub = revoke_subject_tokens(&pool, tenant.id, sub1_id, OffsetDateTime::now_utc())
        .await
        .expect("revoke tokens");

    let tokens_revoked_events =
        AuditRepository::list_by_action(&pool, tenant.id, "subject.tokens_revoked", 10)
            .await
            .expect("list subject.tokens_revoked");
    assert_eq!(tokens_revoked_events.len(), 1);
    assert_eq!(tokens_revoked_events[0].subject_id, Some(sub1_id));
    assert_eq!(
        tokens_revoked_events[0].details["token_epoch"],
        revoked_sub.token_epoch
    );
    assert_no_forbidden_fields(&tokens_revoked_events[0].details, "");

    // Deactivate device
    deactivate_device(DeactivateDeviceParams {
        pool: &pool,
        ephemeral: &ephemeral,
        bus: &bus,
        tenant_id: tenant.id,
        device_id: dev1_id,
        caller_device_id: None,
        key_prefix: "gs",
        now: OffsetDateTime::now_utc(),
    })
    .await
    .expect("deactivate device");

    let dev_deactivated_events =
        AuditRepository::list_by_action(&pool, tenant.id, "device.deactivated", 10)
            .await
            .expect("list device.deactivated");
    assert_eq!(dev_deactivated_events.len(), 1);
    assert_eq!(dev_deactivated_events[0].subject_id, Some(sub1_id));
    assert_eq!(
        dev_deactivated_events[0].target_id,
        Some(dev1_id.into_uuid())
    );
    assert_no_forbidden_fields(&dev_deactivated_events[0].details, "");

    // 9. Session closed
    close_session_with_actor(
        &pool,
        tenant.id,
        session.id,
        ClosedReason::ClosedByApplication,
        Some(Actor::Application(app.id)),
    )
    .await
    .expect("close session");

    let session_closed_events =
        AuditRepository::list_by_action(&pool, tenant.id, "session.closed", 10)
            .await
            .expect("list session.closed");
    assert_eq!(session_closed_events.len(), 1);
    assert_eq!(session_closed_events[0].actor_kind, ActorKind::Application);
    assert_eq!(
        session_closed_events[0].target_id,
        Some(session.id.into_uuid())
    );
    assert_eq!(
        session_closed_events[0].details["reason"],
        "CLOSED_BY_APPLICATION"
    );
    assert_no_forbidden_fields(&session_closed_events[0].details, "");

    // 9b. Session expired (session.expired)
    let expiring_session_req = CreateSessionRequest {
        mode: SessionMode::OneToMany,
        expires_in_seconds: Some(60),
        expires_at: None,
        max_members: Some(5),
        snapshot_ttl_seconds: Some(30),
        stale_after_seconds: Some(30),
        metadata: None,
        owner_external_subject_id: None,
        initial_members: None,
    };
    let exp_session = create_session(
        &pool,
        tenant.id,
        Actor::Application(app.id),
        &expiring_session_req,
        session_policy(),
    )
    .await
    .expect("create expiring session");

    // Backdate expires_at and trigger worker expiry
    sqlx::query("UPDATE sessions SET expires_at = $1 WHERE id = $2")
        .bind(OffsetDateTime::now_utc() - time::Duration::seconds(60))
        .bind(exp_session.id.into_uuid())
        .execute(&pool)
        .await
        .expect("backdate session expires_at");

    let expired_sessions = expire_due_sessions(&pool, 10)
        .await
        .expect("expire due sessions");
    assert!(expired_sessions.contains(&exp_session.id));

    let session_expired_events =
        AuditRepository::list_by_action(&pool, tenant.id, "session.expired", 10)
            .await
            .expect("list session.expired");
    assert!(!session_expired_events.is_empty());
    let exp_event = session_expired_events
        .iter()
        .find(|e| e.target_id == Some(exp_session.id.into_uuid()))
        .expect("found exp session event");
    assert_eq!(exp_event.actor_kind, ActorKind::System);
    assert_eq!(exp_event.details["reason"], "expired");
    assert_no_forbidden_fields(&exp_event.details, "");

    // 10. Every audit event recorded for tenant must pass strict forbidden-field inspection
    let all_events = AuditRepository::list_by_tenant(&pool, tenant.id, 100)
        .await
        .expect("list all tenant events");
    assert!(
        all_events.len() >= 14,
        "Expected at least 14 audited sensitive actions, got {}",
        all_events.len()
    );
    for evt in &all_events {
        assert_no_forbidden_fields(&evt.details, &format!("{}[{}]", evt.action, evt.id));
    }
}

#[tokio::test]
async fn test_inv_ten_1_audit_strict_tenant_isolation() {
    let pool = setup_db().await;

    let t1 = create_tenant(&pool, "Tenant 1").await.expect("t1");
    let t2 = create_tenant(&pool, "Tenant 2").await.expect("t2");

    let app1 = create_application(&pool, &t1.id.to_string(), "app-1", None, None)
        .await
        .expect("app1");
    let app2 = create_application(&pool, &t2.id.to_string(), "app-2", None, None)
        .await
        .expect("app2");

    let (dev1, sub1) = setup_test_device(&pool, t1.id, app1.id, "t1_user").await;
    let (dev2, sub2) = setup_test_device(&pool, t2.id, app2.id, "t2_user").await;
    assert_ne!(sub1, sub2, "Tenants have distinct subjects");

    // Create session in T1
    let create_req = CreateSessionRequest {
        mode: SessionMode::OneToMany,
        expires_in_seconds: Some(3600),
        expires_at: None,
        max_members: Some(10),
        snapshot_ttl_seconds: Some(30),
        stale_after_seconds: Some(30),
        metadata: None,
        owner_external_subject_id: None,
        initial_members: None,
    };
    let s1 = create_session(
        &pool,
        t1.id,
        Actor::Device(dev1),
        &create_req,
        session_policy(),
    )
    .await
    .expect("s1");

    // Create session in T2
    let s2 = create_session(
        &pool,
        t2.id,
        Actor::Device(dev2),
        &create_req,
        session_policy(),
    )
    .await
    .expect("s2");

    // Verify T1 cannot see T2 audit events and vice-versa
    let t1_events = AuditRepository::list_by_tenant(&pool, t1.id, 50)
        .await
        .expect("t1 events");
    let t2_events = AuditRepository::list_by_tenant(&pool, t2.id, 50)
        .await
        .expect("t2 events");

    assert!(!t1_events.is_empty());
    assert!(!t2_events.is_empty());

    for evt in &t1_events {
        assert_eq!(evt.tenant_id, t1.id, "INV-TEN-1: T1 event tenant mismatch");
        assert_ne!(
            evt.target_id,
            Some(s2.id.into_uuid()),
            "INV-TEN-1: T1 leak of T2 session"
        );
    }
    for evt in &t2_events {
        assert_eq!(evt.tenant_id, t2.id, "INV-TEN-1: T2 event tenant mismatch");
        assert_ne!(
            evt.target_id,
            Some(s1.id.into_uuid()),
            "INV-TEN-1: T2 leak of T1 session"
        );
    }

    // Verify subject audit listing respects tenant isolation
    let sub1_events = AuditRepository::list_by_subject(&pool, t1.id, sub1, 50)
        .await
        .expect("sub1");
    assert!(!sub1_events.is_empty());
    for evt in &sub1_events {
        assert_eq!(evt.tenant_id, t1.id);
        assert_eq!(evt.subject_id, Some(sub1));
    }

    // T2 querying sub1 returns empty list
    let t2_sub1 = AuditRepository::list_by_subject(&pool, t2.id, sub1, 50)
        .await
        .expect("t2 sub1");
    assert!(
        t2_sub1.is_empty(),
        "INV-TEN-1: cross-tenant subject query must return empty"
    );

    // Action listing respects tenant isolation
    let t1_sess_created = AuditRepository::list_by_action(&pool, t1.id, "session.created", 50)
        .await
        .expect("t1 sess created");
    assert_eq!(t1_sess_created.len(), 1);
    assert_eq!(t1_sess_created[0].target_id, Some(s1.id.into_uuid()));

    let t2_sess_created = AuditRepository::list_by_action(&pool, t2.id, "session.created", 50)
        .await
        .expect("t2 sess created");
    assert_eq!(t2_sess_created.len(), 1);
    assert_eq!(t2_sess_created[0].target_id, Some(s2.id.into_uuid()));

    // Target listing respects tenant isolation
    let t1_by_s1 = AuditRepository::list_by_target(&pool, t1.id, "session", s1.id.into_uuid(), 10)
        .await
        .expect("t1 target");
    assert!(!t1_by_s1.is_empty());

    let t2_by_s1 = AuditRepository::list_by_target(&pool, t2.id, "session", s1.id.into_uuid(), 10)
        .await
        .expect("t2 target s1");
    assert!(
        t2_by_s1.is_empty(),
        "INV-TEN-1: cross-tenant target query must return empty"
    );
}
