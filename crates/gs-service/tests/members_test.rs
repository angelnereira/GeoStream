//! Integration tests for membership use cases against PostgreSQL
//! (T-F3-04, FR-MEM-001..007, FR-SES-007, INV-MEM-1..7).
#![forbid(unsafe_code)]

use std::str::FromStr;

use gs_domain::ids::{DeviceId, MemberId, SubjectId, TenantId};
use gs_domain::membership::{Capabilities, MemberRole, MemberStatusKind};
use gs_domain::session::{Actor, ClosedReason, SessionMode, SessionStatus};
use gs_service::admin::{create_application, create_tenant};
use gs_service::members::{
    AddMemberRequest, MemberUseCaseError, UpdateCaller, UpdateMemberRequest, add_member,
    get_member_for_device_me, leave_session, list_members, revoke_member, update_member,
};
use gs_service::sessions::{CreateSessionRequest, SessionPolicy, create_session, get_session};
use gs_service::tokens::mint_device_token;
use gs_store::repos::MembershipRepository;
use gs_store::{PgPool, connect_pool, run_migrations};

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

async fn setup_tenant_and_app() -> (PgPool, TenantId, String, gs_auth::SigningKey) {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");

    let tenant = create_tenant(&pool, "Member Test Tenant")
        .await
        .expect("tenant");
    let application = create_application(&pool, &tenant.id.to_string(), "members-app", None, None)
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
        ]),
        None,
        None,
    )
    .await
    .expect("api key");

    let generated = gs_auth::generate_signing_key("member-test").expect("generate key");
    let signing = gs_auth::parse_private_pem(generated.private_pem()).expect("parse key");

    (pool, tenant.id, key.credential.to_string(), signing)
}

async fn mint_device(
    pool: &PgPool,
    api_key: &str,
    signing: &gs_auth::SigningKey,
    external_subject_id: &str,
    installation_id: &str,
) -> (DeviceId, SubjectId) {
    let request = serde_json::from_value(serde_json::json!({
        "external_subject_id": external_subject_id,
        "installation_id": installation_id,
        "platform": "ANDROID"
    }))
    .expect("token request");

    let minted = mint_device_token(
        pool,
        api_key,
        &request,
        gs_service::tokens::TokenTtlBounds {
            min_seconds: 300,
            default_seconds: 900,
            max_seconds: 3600,
        },
        "https://geostream.example.com",
        signing,
        "member-test",
    )
    .await
    .expect("mint token");

    let dev_id = DeviceId::from_str(&minted.device.id.to_string()).expect("device id");
    let sub_id = SubjectId::from_str(&minted.subject.id.to_string()).expect("subject id");
    (dev_id, sub_id)
}

fn test_policy() -> SessionPolicy {
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

fn create_req(mode: SessionMode, max_members: Option<u32>) -> CreateSessionRequest {
    CreateSessionRequest {
        mode,
        expires_in_seconds: Some(7200),
        expires_at: None,
        max_members,
        snapshot_ttl_seconds: None,
        stale_after_seconds: None,
        metadata: None,
        owner_external_subject_id: None,
        initial_members: None,
    }
}

#[tokio::test]
async fn test_add_member_direct_and_limits() {
    // covers: FR-MEM-001, INV-MEM-6, INV-MEM-7
    let (pool, tenant_id, api_key, signing) = setup_tenant_and_app().await;
    let (dev_owner, _sub_owner) = mint_device(
        &pool,
        &api_key,
        &signing,
        "owner-sub",
        "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d01",
    )
    .await;

    // Create session with max_members = 2
    let session = create_session(
        &pool,
        tenant_id,
        Actor::Device(dev_owner),
        &create_req(SessionMode::OneToOne, Some(2)),
        test_policy(),
    )
    .await
    .expect("create session");

    // Add first member (direct add) -> success
    let add_req_1 = AddMemberRequest {
        external_subject_id: "user-alpha".to_string(),
        role: None,
        capabilities: None,
    };
    let member_1 = add_member(&pool, tenant_id, session.id, &add_req_1)
        .await
        .expect("add member 1");
    assert_eq!(member_1.external_subject_id, "user-alpha");
    assert_eq!(member_1.role, MemberRole::Member);
    assert_eq!(member_1.status, MemberStatusKind::Active);
    assert_eq!(member_1.capabilities, Capabilities::publish_read());

    // Try to add the same member again -> INV-MEM-6 conflict
    let err_dup = add_member(&pool, tenant_id, session.id, &add_req_1).await;
    match err_dup {
        Err(MemberUseCaseError::AlreadyActiveMember) => {}
        other => panic!("expected AlreadyActiveMember, got {other:?}"),
    }

    // Try to add a 3rd member -> INV-MEM-7 session full (max_members = 2 reached: owner + user-alpha)
    let add_req_2 = AddMemberRequest {
        external_subject_id: "user-beta".to_string(),
        role: None,
        capabilities: None,
    };
    let err_full = add_member(&pool, tenant_id, session.id, &add_req_2).await;
    match err_full {
        Err(MemberUseCaseError::SessionFull) => {}
        other => panic!("expected SessionFull, got {other:?}"),
    }
}

#[tokio::test]
async fn test_readmission_left_and_revoked() {
    // covers: FR-MEM-007 (readmission after LEFT and REVOKED)
    let (pool, tenant_id, api_key, signing) = setup_tenant_and_app().await;
    let (dev_owner, _sub_owner) = mint_device(
        &pool,
        &api_key,
        &signing,
        "owner-readmit",
        "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d02",
    )
    .await;
    let (_dev_user, sub_user) = mint_device(
        &pool,
        &api_key,
        &signing,
        "user-readmit",
        "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d03",
    )
    .await;

    let session = create_session(
        &pool,
        tenant_id,
        Actor::Device(dev_owner),
        &create_req(SessionMode::ManyToMany, Some(10)),
        test_policy(),
    )
    .await
    .expect("create session");

    // 1. Add user
    let m1 = add_member(
        &pool,
        tenant_id,
        session.id,
        &AddMemberRequest {
            external_subject_id: "user-readmit".to_string(),
            role: None,
            capabilities: None,
        },
    )
    .await
    .expect("add member");

    // User leaves voluntarily
    leave_session(&pool, tenant_id, session.id, sub_user)
        .await
        .expect("leave session");

    // Readmission after LEFT via direct add -> allowed!
    let m2 = add_member(
        &pool,
        tenant_id,
        session.id,
        &AddMemberRequest {
            external_subject_id: "user-readmit".to_string(),
            role: None,
            capabilities: None,
        },
    )
    .await
    .expect("readmit member after left");
    assert_ne!(m1.id, m2.id); // new membership ID

    // Manager/App revokes user
    let m2_id = MemberId::from_str(&m2.id).expect("member id");
    revoke_member(&pool, tenant_id, session.id, m2_id)
        .await
        .expect("revoke member");

    // Revoking again is idempotent
    revoke_member(&pool, tenant_id, session.id, m2_id)
        .await
        .expect("idempotent revoke");

    // FR-MEM-007: Readmission after REVOKED via direct add -> allowed!
    let m3 = add_member(
        &pool,
        tenant_id,
        session.id,
        &AddMemberRequest {
            external_subject_id: "user-readmit".to_string(),
            role: None,
            capabilities: None,
        },
    )
    .await
    .expect("readmit member after revoked");
    assert_ne!(m2.id, m3.id);
    assert_eq!(m3.status, MemberStatusKind::Active);
}

#[tokio::test]
async fn test_list_members_and_get_me() {
    // covers: FR-MEM-002, FR-MEM-003
    let (pool, tenant_id, api_key, signing) = setup_tenant_and_app().await;
    let (dev_owner, sub_owner) = mint_device(
        &pool,
        &api_key,
        &signing,
        "owner-list",
        "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d04",
    )
    .await;

    let session = create_session(
        &pool,
        tenant_id,
        Actor::Device(dev_owner),
        &create_req(SessionMode::ManyToMany, Some(10)),
        test_policy(),
    )
    .await
    .expect("create session");

    // Add 3 members
    for i in 1..=3 {
        add_member(
            &pool,
            tenant_id,
            session.id,
            &AddMemberRequest {
                external_subject_id: format!("member-{i}"),
                role: None,
                capabilities: None,
            },
        )
        .await
        .expect("add member");
    }

    // List members with limit = 2 (page 1)
    let page1 = list_members(&pool, tenant_id, session.id, 2, None)
        .await
        .expect("page 1");
    assert_eq!(page1.members.len(), 3); // 2 + 1 to detect has_more
    assert!(page1.has_more(2));

    // Page 2 using cursor
    let cursor = Some((page1.last_joined_at.unwrap(), page1.last_id.unwrap()));
    let page2 = list_members(&pool, tenant_id, session.id, 2, cursor)
        .await
        .expect("page 2");
    assert_eq!(page2.members.len(), 1); // 4th total item

    // Test get me for owner
    let me = get_member_for_device_me(&pool, tenant_id, session.id, sub_owner)
        .await
        .expect("get me");
    assert_eq!(me.external_subject_id, "owner-list");
    assert_eq!(me.role, MemberRole::Owner);
}

#[tokio::test]
async fn test_update_member_capabilities_invariants() {
    // covers: INV-MEM-1, INV-MEM-2, INV-MEM-3, INV-MEM-4, FR-MEM-004
    let (pool, tenant_id, api_key, signing) = setup_tenant_and_app().await;
    let (dev_owner, sub_owner) = mint_device(
        &pool,
        &api_key,
        &signing,
        "owner-inv",
        "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d05",
    )
    .await;

    let session = create_session(
        &pool,
        tenant_id,
        Actor::Device(dev_owner),
        &create_req(SessionMode::ManyToMany, Some(10)),
        test_policy(),
    )
    .await
    .expect("create session");

    // Get owner membership
    let owner_mem = MembershipRepository::find_active_by_session_and_subject(
        &pool, tenant_id, session.id, sub_owner,
    )
    .await
    .expect("find owner")
    .expect("owner exists");

    // Add manager member
    let manager = add_member(
        &pool,
        tenant_id,
        session.id,
        &AddMemberRequest {
            external_subject_id: "manager-user".to_string(),
            role: None,
            capabilities: Some(Capabilities {
                publish: true,
                read: true,
                invite: true,
                manage: true,
            }),
        },
    )
    .await
    .expect("add manager");
    let manager_id = MemberId::from_str(&manager.id).expect("manager id");

    // Add target member
    let target = add_member(
        &pool,
        tenant_id,
        session.id,
        &AddMemberRequest {
            external_subject_id: "target-user".to_string(),
            role: None,
            capabilities: Some(Capabilities::publish_read()),
        },
    )
    .await
    .expect("add target");
    let target_id = MemberId::from_str(&target.id).expect("target id");

    // INV-MEM-1: Owner capabilities cannot be reduced
    let err_owner = update_member(
        &pool,
        tenant_id,
        session.id,
        owner_mem.id,
        None,
        &UpdateMemberRequest {
            capabilities: Some(Capabilities::read_only()),
            role: None,
        },
        UpdateCaller::Application,
    )
    .await;
    assert!(err_owner.is_err());

    // INV-MEM-3: Manager cannot modify their own capabilities
    let manager_mem = MembershipRepository::find_active_by_session_and_subject(
        &pool,
        tenant_id,
        session.id,
        SubjectId::from_str(&manager.subject_id).unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    let err_self = update_member(
        &pool,
        tenant_id,
        session.id,
        manager_id,
        None,
        &UpdateMemberRequest {
            capabilities: Some(Capabilities::read_only()),
            role: None,
        },
        UpdateCaller::Device(&manager_mem),
    )
    .await;
    assert!(err_self.is_err());

    // INV-MEM-2: Non-manager member cannot update capabilities
    let target_mem = MembershipRepository::find_active_by_session_and_subject(
        &pool,
        tenant_id,
        session.id,
        SubjectId::from_str(&target.subject_id).unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    let err_no_manage = update_member(
        &pool,
        tenant_id,
        session.id,
        manager_id,
        None,
        &UpdateMemberRequest {
            capabilities: Some(Capabilities::read_only()),
            role: None,
        },
        UpdateCaller::Device(&target_mem),
    )
    .await;
    assert!(err_no_manage.is_err());

    // Success update capabilities by application
    let updated = update_member(
        &pool,
        tenant_id,
        session.id,
        target_id,
        Some(target.version),
        &UpdateMemberRequest {
            capabilities: Some(Capabilities::read_only()),
            role: None,
        },
        UpdateCaller::Application,
    )
    .await
    .expect("update target caps");
    assert_eq!(updated.capabilities, Capabilities::read_only());
    assert_eq!(updated.version, target.version + 1);

    // Stale version If-Match fails with PreconditionFailed
    let err_stale = update_member(
        &pool,
        tenant_id,
        session.id,
        target_id,
        Some(target.version), // old version
        &UpdateMemberRequest {
            capabilities: Some(Capabilities::all()),
            role: None,
        },
        UpdateCaller::Application,
    )
    .await;
    match err_stale {
        Err(MemberUseCaseError::PreconditionFailed) => {}
        other => panic!("expected PreconditionFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_transfer_ownership_and_owner_departure_cascade() {
    // covers: FR-MEM-005, FR-SES-007
    let (pool, tenant_id, api_key, signing) = setup_tenant_and_app().await;
    let (dev_owner, sub_owner) = mint_device(
        &pool,
        &api_key,
        &signing,
        "owner-depart",
        "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d06",
    )
    .await;
    let (_dev_succ, sub_succ) = mint_device(
        &pool,
        &api_key,
        &signing,
        "successor-user",
        "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d07",
    )
    .await;

    // Create session by device
    let session = create_session(
        &pool,
        tenant_id,
        Actor::Device(dev_owner),
        &create_req(SessionMode::ManyToMany, Some(10)),
        test_policy(),
    )
    .await
    .expect("create session");

    // Add candidate with manage capability
    let candidate = add_member(
        &pool,
        tenant_id,
        session.id,
        &AddMemberRequest {
            external_subject_id: "successor-user".to_string(),
            role: None,
            capabilities: Some(Capabilities {
                publish: true,
                read: true,
                invite: true,
                manage: true,
            }),
        },
    )
    .await
    .expect("add candidate");
    let candidate_id = MemberId::from_str(&candidate.id).expect("cand id");

    // 1. Explicit ownership transfer via PATCH role: OWNER (FR-MEM-005)
    let new_owner = update_member(
        &pool,
        tenant_id,
        session.id,
        candidate_id,
        None,
        &UpdateMemberRequest {
            capabilities: None,
            role: Some(MemberRole::Owner),
        },
        UpdateCaller::Application,
    )
    .await
    .expect("transfer ownership");
    assert_eq!(new_owner.role, MemberRole::Owner);
    assert_eq!(new_owner.capabilities, Capabilities::all());

    // Former owner became regular member
    let former_owner = MembershipRepository::find_active_by_session_and_subject(
        &pool, tenant_id, session.id, sub_owner,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(former_owner.role, MemberRole::Member);

    // Session owner_subject_id updated
    let sess = get_session(&pool, tenant_id, session.id)
        .await
        .expect("get session");
    assert_eq!(sess.owner_subject_id, Some(sub_succ));

    // 2. FR-SES-007: New owner leaves.
    // Former owner does NOT have manage, but wait: former owner kept their capabilities!
    // When owner demoted to MEMBER, did former owner keep manage? Yes, former owner had all 4 caps.
    // So former owner is eligible to be promoted back!
    leave_session(&pool, tenant_id, session.id, sub_succ)
        .await
        .expect("new owner leaves");

    // Former owner was re-promoted to OWNER!
    let sess_after = get_session(&pool, tenant_id, session.id)
        .await
        .expect("get session");
    assert_eq!(sess_after.owner_subject_id, Some(sub_owner));
    assert_eq!(sess_after.status, SessionStatus::Active);

    // 3. Now former owner leaves too. No active members remain with manage!
    // Since session was created by Device, session MUST close with ClosedReason::OwnerLeft (FR-SES-007)
    leave_session(&pool, tenant_id, session.id, sub_owner)
        .await
        .expect("last owner leaves");

    let sess_closed = get_session(&pool, tenant_id, session.id)
        .await
        .expect("get session");
    assert_eq!(sess_closed.status, SessionStatus::Closed);
    assert_eq!(sess_closed.closed_reason, Some(ClosedReason::OwnerLeft));
}
