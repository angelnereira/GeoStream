//! Integration tests for invitations use cases against PostgreSQL
//! (T-F3-06, FR-INV-001..003, docs/05 §6, docs/07 §9.4).
#![forbid(unsafe_code)]

use std::str::FromStr;
use std::sync::Arc;

use gs_auth::TenantPolicy;
use gs_domain::ids::{DeviceId, MemberId, SubjectId, TenantId};
use gs_domain::membership::{Capabilities, MemberRole};
use gs_domain::session::{Actor, SessionMode};
use gs_service::admin::{create_application, create_tenant};
use gs_service::invitations::{
    CreateInvitationCaller, CreateInvitationRequest, InvitationStatus, InvitationUseCaseError,
    ListInvitationsCaller, RedeemInvitationParams, RevokeInvitationCaller, create_invitation,
    list_invitations, redeem_invitation, revoke_invitation,
};
use gs_service::members::{AddMemberRequest, add_member};
use gs_service::sessions::{CreateSessionRequest, SessionPolicy, create_session};
use gs_service::tokens::mint_device_token;
use gs_service::{EphemeralStore, InMemoryEphemeralStore};
use gs_store::{PgPool, connect_pool, run_migrations};
use time::OffsetDateTime;

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

async fn setup_fixture() -> (PgPool, TenantId, String, gs_auth::SigningKey) {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");

    let tenant = create_tenant(&pool, "Invitation Test Tenant")
        .await
        .expect("tenant");
    let application =
        create_application(&pool, &tenant.id.to_string(), "invitations-app", None, None)
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
            "invitations:write".into(),
        ]),
        None,
        None,
    )
    .await
    .expect("api key");

    let generated = gs_auth::generate_signing_key("invitation-test").expect("generate key");
    let signing = gs_auth::parse_private_pem(generated.private_pem()).expect("parse key");

    (pool, tenant.id, key.credential.to_string(), signing)
}

async fn mint_device(
    pool: &PgPool,
    api_key: &str,
    signing: &gs_auth::SigningKey,
    external_subject_id: &str,
) -> (DeviceId, SubjectId) {
    let installation_id = uuid::Uuid::new_v4().to_string();
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
        "invitation-test",
    )
    .await
    .expect("mint token");

    let dev_id = DeviceId::from_str(&minted.device.id.to_string()).expect("device id");
    let sub_id = SubjectId::from_str(&minted.subject.id.to_string()).expect("subject id");
    (dev_id, sub_id)
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
async fn test_create_invitation_app_and_device_permissions() {
    let (pool, tenant_id, api_key, signing) = setup_fixture().await;
    let (dev_owner, _sub_owner) = mint_device(&pool, &api_key, &signing, "user-owner").await;
    let (dev_member, sub_member) = mint_device(&pool, &api_key, &signing, "user-member").await;
    let (dev_viewer, sub_viewer) = mint_device(&pool, &api_key, &signing, "user-viewer").await;

    let tenant_policy = TenantPolicy::default();
    let now = OffsetDateTime::now_utc();

    // 1. Create a session with owner device
    let session = create_session(
        &pool,
        tenant_id,
        Actor::Device(dev_owner),
        &create_req(SessionMode::OneToMany, Some(10)),
        test_session_policy(),
    )
    .await
    .expect("create session");

    let session_id = session.id;

    // Add member with invite capability
    let member_with_invite = add_member(
        &pool,
        tenant_id,
        session_id,
        &AddMemberRequest {
            external_subject_id: "user-member".into(),
            role: Some(MemberRole::Member),
            capabilities: Some(Capabilities {
                publish: true,
                read: true,
                invite: true,
                manage: false,
            }),
        },
    )
    .await
    .expect("add member with invite");

    // Add viewer without invite capability
    let member_no_invite = add_member(
        &pool,
        tenant_id,
        session_id,
        &AddMemberRequest {
            external_subject_id: "user-viewer".into(),
            role: Some(MemberRole::Member),
            capabilities: Some(Capabilities {
                publish: false,
                read: true,
                invite: false,
                manage: false,
            }),
        },
    )
    .await
    .expect("add viewer");

    let member_no_invite_id = MemberId::from_str(&member_no_invite.id).expect("id");
    let member_with_invite_id = MemberId::from_str(&member_with_invite.id).expect("id");

    // 2. Caller without invite capability is rejected
    let res = create_invitation(
        &pool,
        tenant_id,
        session_id,
        &CreateInvitationRequest {
            max_uses: Some(1),
            expires_in_seconds: Some(300),
            grants: None,
        },
        CreateInvitationCaller::Device {
            device_id: dev_viewer,
            subject_id: sub_viewer,
            member_id: member_no_invite_id,
            role: MemberRole::Member,
            capabilities: Capabilities {
                publish: false,
                read: true,
                invite: false,
                manage: false,
            },
        },
        &tenant_policy,
        now,
    )
    .await;
    assert!(matches!(
        res,
        Err(InvitationUseCaseError::PermissionDenied(_))
    ));

    // 3. Member with invite cannot grant manage
    let res = create_invitation(
        &pool,
        tenant_id,
        session_id,
        &CreateInvitationRequest {
            max_uses: Some(1),
            expires_in_seconds: Some(300),
            grants: Some(Capabilities {
                publish: true,
                read: true,
                invite: true,
                manage: true,
            }),
        },
        CreateInvitationCaller::Device {
            device_id: dev_member,
            subject_id: sub_member,
            member_id: member_with_invite_id,
            role: MemberRole::Member,
            capabilities: Capabilities {
                publish: true,
                read: true,
                invite: true,
                manage: false,
            },
        },
        &tenant_policy,
        now,
    )
    .await;
    assert!(matches!(
        res,
        Err(InvitationUseCaseError::PermissionDenied(_))
    ));

    // 4. Member with invite cannot grant capabilities they don't hold
    let res = create_invitation(
        &pool,
        tenant_id,
        session_id,
        &CreateInvitationRequest {
            max_uses: Some(1),
            expires_in_seconds: Some(300),
            grants: Some(Capabilities {
                publish: true,
                read: true,
                invite: true,
                manage: false,
            }),
        },
        CreateInvitationCaller::Device {
            device_id: dev_member,
            subject_id: sub_member,
            member_id: member_with_invite_id,
            role: MemberRole::Member,
            capabilities: Capabilities {
                publish: false, // does not have publish!
                read: true,
                invite: true,
                manage: false,
            },
        },
        &tenant_policy,
        now,
    )
    .await;
    assert!(matches!(
        res,
        Err(InvitationUseCaseError::PermissionDenied(_))
    ));

    // 5. Validation on max_uses (0 is invalid, > policy is invalid)
    let res_zero = create_invitation(
        &pool,
        tenant_id,
        session_id,
        &CreateInvitationRequest {
            max_uses: Some(0),
            expires_in_seconds: Some(300),
            grants: None,
        },
        CreateInvitationCaller::Application,
        &tenant_policy,
        now,
    )
    .await;
    assert!(matches!(
        res_zero,
        Err(InvitationUseCaseError::Validation(_))
    ));

    let res_exceed = create_invitation(
        &pool,
        tenant_id,
        session_id,
        &CreateInvitationRequest {
            max_uses: Some(tenant_policy.invitation_max_uses + 1),
            expires_in_seconds: Some(300),
            grants: None,
        },
        CreateInvitationCaller::Application,
        &tenant_policy,
        now,
    )
    .await;
    assert!(matches!(
        res_exceed,
        Err(InvitationUseCaseError::Validation(_))
    ));

    // 6. Validation on expires_in_seconds (< 60s is invalid)
    let res_ttl_low = create_invitation(
        &pool,
        tenant_id,
        session_id,
        &CreateInvitationRequest {
            max_uses: Some(5),
            expires_in_seconds: Some(30),
            grants: None,
        },
        CreateInvitationCaller::Application,
        &tenant_policy,
        now,
    )
    .await;
    assert!(matches!(
        res_ttl_low,
        Err(InvitationUseCaseError::Validation(_))
    ));

    // 7. Successful creation by Application
    let created_by_app = create_invitation(
        &pool,
        tenant_id,
        session_id,
        &CreateInvitationRequest {
            max_uses: Some(5),
            expires_in_seconds: Some(1800),
            grants: Some(Capabilities {
                publish: true,
                read: true,
                invite: false,
                manage: false,
            }),
        },
        CreateInvitationCaller::Application,
        &tenant_policy,
        now,
    )
    .await
    .expect("create invitation by app");

    assert_eq!(created_by_app.invitation.max_uses, 5);
    assert_eq!(created_by_app.invitation.uses, 0);
    assert_eq!(created_by_app.invitation.status, InvitationStatus::Active);
    assert!(!created_by_app.code.is_empty());
}

#[tokio::test]
async fn test_list_and_revoke_invitations() {
    let (pool, tenant_id, api_key, signing) = setup_fixture().await;
    let (dev_owner, _sub_owner) = mint_device(&pool, &api_key, &signing, "user-list-owner").await;
    let (dev_member, sub_member) = mint_device(&pool, &api_key, &signing, "user-list-member").await;

    let tenant_policy = TenantPolicy::default();
    let now = OffsetDateTime::now_utc();

    let session = create_session(
        &pool,
        tenant_id,
        Actor::Device(dev_owner),
        &create_req(SessionMode::ManyToMany, Some(10)),
        test_session_policy(),
    )
    .await
    .expect("create session");

    let session_id = session.id;

    let member_with_invite = add_member(
        &pool,
        tenant_id,
        session_id,
        &AddMemberRequest {
            external_subject_id: "user-list-member".into(),
            role: Some(MemberRole::Member),
            capabilities: Some(Capabilities {
                publish: true,
                read: true,
                invite: true,
                manage: false,
            }),
        },
    )
    .await
    .expect("add member");

    let member_id = MemberId::from_str(&member_with_invite.id).expect("member_id");

    // 1. App creates invitation
    let inv_app = create_invitation(
        &pool,
        tenant_id,
        session_id,
        &CreateInvitationRequest {
            max_uses: Some(1),
            expires_in_seconds: Some(600),
            grants: None,
        },
        CreateInvitationCaller::Application,
        &tenant_policy,
        now,
    )
    .await
    .expect("app inv");

    // 2. Member creates invitation
    let inv_member = create_invitation(
        &pool,
        tenant_id,
        session_id,
        &CreateInvitationRequest {
            max_uses: Some(2),
            expires_in_seconds: Some(600),
            grants: None,
        },
        CreateInvitationCaller::Device {
            device_id: dev_member,
            subject_id: sub_member,
            member_id,
            role: MemberRole::Member,
            capabilities: Capabilities {
                publish: true,
                read: true,
                invite: true,
                manage: false,
            },
        },
        &tenant_policy,
        now,
    )
    .await
    .expect("member inv");

    // 3. App lists all -> sees both
    let list_app = list_invitations(
        &pool,
        tenant_id,
        session_id,
        50,
        None,
        ListInvitationsCaller::Application,
        now,
    )
    .await
    .expect("list app");
    assert_eq!(list_app.data.len(), 2);

    // 4. Member (invite_own) lists -> sees ONLY inv_member
    let list_member = list_invitations(
        &pool,
        tenant_id,
        session_id,
        50,
        None,
        ListInvitationsCaller::Device {
            member_id,
            capabilities: Capabilities {
                publish: true,
                read: true,
                invite: true,
                manage: false,
            },
        },
        now,
    )
    .await
    .expect("list member");
    assert_eq!(list_member.data.len(), 1);
    assert_eq!(list_member.data[0].id, inv_member.invitation.id);

    // 5. Member attempts to revoke app's invitation -> PermissionDenied
    let inv_app_id =
        gs_domain::ids::InvitationId::from_str(&inv_app.invitation.id).expect("inv_app_id");
    let res_revoke_forbidden = revoke_invitation(
        &pool,
        tenant_id,
        session_id,
        inv_app_id,
        RevokeInvitationCaller::Device {
            device_id: dev_member,
            member_id,
            capabilities: Capabilities {
                publish: true,
                read: true,
                invite: true,
                manage: false,
            },
        },
        now,
    )
    .await;
    assert!(matches!(
        res_revoke_forbidden,
        Err(InvitationUseCaseError::PermissionDenied(_))
    ));

    // 6. Member revokes own invitation -> Ok
    let inv_member_id =
        gs_domain::ids::InvitationId::from_str(&inv_member.invitation.id).expect("inv_member_id");
    revoke_invitation(
        &pool,
        tenant_id,
        session_id,
        inv_member_id,
        RevokeInvitationCaller::Device {
            device_id: dev_member,
            member_id,
            capabilities: Capabilities {
                publish: true,
                read: true,
                invite: true,
                manage: false,
            },
        },
        now,
    )
    .await
    .expect("revoke own");

    // Revocation is idempotent
    revoke_invitation(
        &pool,
        tenant_id,
        session_id,
        inv_member_id,
        RevokeInvitationCaller::Device {
            device_id: dev_member,
            member_id,
            capabilities: Capabilities {
                publish: true,
                read: true,
                invite: true,
                manage: false,
            },
        },
        now,
    )
    .await
    .expect("revoke own again");

    // Status is now REVOKED
    let list_member_after = list_invitations(
        &pool,
        tenant_id,
        session_id,
        50,
        None,
        ListInvitationsCaller::Device {
            member_id,
            capabilities: Capabilities {
                publish: true,
                read: true,
                invite: true,
                manage: false,
            },
        },
        now,
    )
    .await
    .expect("list member after revoke");
    assert_eq!(list_member_after.data[0].status, InvitationStatus::Revoked);
}

#[tokio::test]
async fn test_redeem_invitation_lifecycle_and_invariants() {
    let (pool, tenant_id, api_key, signing) = setup_fixture().await;
    let (dev_owner, _sub_owner) = mint_device(&pool, &api_key, &signing, "user-rdm-owner").await;
    let (dev_redeemer1, sub_redeemer1) = mint_device(&pool, &api_key, &signing, "user-rdm-1").await;
    let (dev_redeemer2, sub_redeemer2) = mint_device(&pool, &api_key, &signing, "user-rdm-2").await;

    let ephemeral = Arc::new(InMemoryEphemeralStore::new());
    let tenant_policy = TenantPolicy::default();
    let now = OffsetDateTime::now_utc();

    let session = create_session(
        &pool,
        tenant_id,
        Actor::Device(dev_owner),
        &create_req(SessionMode::OneToMany, Some(3)), // owner + 2 max
        test_session_policy(),
    )
    .await
    .expect("create session");

    let session_id = session.id;

    // Create invitation with max_uses = 1
    let created = create_invitation(
        &pool,
        tenant_id,
        session_id,
        &CreateInvitationRequest {
            max_uses: Some(1),
            expires_in_seconds: Some(300),
            grants: Some(Capabilities {
                publish: false,
                read: true,
                invite: false,
                manage: false,
            }),
        },
        CreateInvitationCaller::Application,
        &tenant_policy,
        now,
    )
    .await
    .expect("create invitation");

    let code = created.code;

    // 1. First redemption succeeds
    let redeemed1 = redeem_invitation(RedeemInvitationParams {
        pool: &pool,
        ephemeral: ephemeral.as_ref(),
        key_prefix: "gs",
        tenant_id,
        code: &code,
        device_id: dev_redeemer1,
        subject_id: sub_redeemer1,
        rate_limit_rpm: 10,
        now,
    })
    .await
    .expect("first redeem");

    assert_eq!(redeemed1.member.session_id, session_id.to_string());
    assert_eq!(redeemed1.member.subject_id, sub_redeemer1.to_string());
    assert_eq!(redeemed1.member.joined_via, "INVITATION");
    assert!(!redeemed1.member.capabilities.publish);
    assert!(redeemed1.member.capabilities.read);

    // Verify mver was initialized or incremented in ephemeral store
    let mver = ephemeral
        .get_membership_version(&format!("gs:mver:{session_id}"))
        .await
        .expect("mver");
    assert!(mver >= Some(1));

    // 2. Same subject attempts to redeem again -> Conflict (409)
    // Create another invitation for the same session to test already-member
    let created_second = create_invitation(
        &pool,
        tenant_id,
        session_id,
        &CreateInvitationRequest {
            max_uses: Some(5),
            expires_in_seconds: Some(300),
            grants: None,
        },
        CreateInvitationCaller::Application,
        &tenant_policy,
        now,
    )
    .await
    .expect("create second invitation");

    let res_conflict = redeem_invitation(RedeemInvitationParams {
        pool: &pool,
        ephemeral: ephemeral.as_ref(),
        key_prefix: "gs",
        tenant_id,
        code: &created_second.code,
        device_id: dev_redeemer1,
        subject_id: sub_redeemer1,
        rate_limit_rpm: 10,
        now,
    })
    .await;
    assert!(matches!(
        res_conflict,
        Err(InvitationUseCaseError::Conflict)
    ));

    // 3. Second redemption of the FIRST code (which had max_uses = 1) -> INVITATION_INVALID (exhausted)
    let res_exhausted = redeem_invitation(RedeemInvitationParams {
        pool: &pool,
        ephemeral: ephemeral.as_ref(),
        key_prefix: "gs",
        tenant_id,
        code: &code,
        device_id: dev_redeemer2,
        subject_id: sub_redeemer2,
        rate_limit_rpm: 10,
        now,
    })
    .await;
    assert!(matches!(
        res_exhausted,
        Err(InvitationUseCaseError::InvitationInvalid)
    ));

    // 4. Second subject redeems created_second code -> succeeds
    let redeemed2 = redeem_invitation(RedeemInvitationParams {
        pool: &pool,
        ephemeral: ephemeral.as_ref(),
        key_prefix: "gs",
        tenant_id,
        code: &created_second.code,
        device_id: dev_redeemer2,
        subject_id: sub_redeemer2,
        rate_limit_rpm: 10,
        now,
    })
    .await
    .expect("second subject redeem");
    assert_eq!(redeemed2.member.subject_id, sub_redeemer2.to_string());

    // 5. Session capacity: session max_members was 3 (owner + 2 redeemers = 3).
    // Now create a 3rd redeemer and test SessionFull
    let (dev_redeemer3, sub_redeemer3) = mint_device(&pool, &api_key, &signing, "user-rdm-3").await;
    let res_full = redeem_invitation(RedeemInvitationParams {
        pool: &pool,
        ephemeral: ephemeral.as_ref(),
        key_prefix: "gs",
        tenant_id,
        code: &created_second.code,
        device_id: dev_redeemer3,
        subject_id: sub_redeemer3,
        rate_limit_rpm: 10,
        now,
    })
    .await;
    assert!(matches!(res_full, Err(InvitationUseCaseError::SessionFull)));

    // 6. Rate limit check: exceeding GS_INVITE_REDEEM_RPM
    let (dev_rl, sub_rl) = mint_device(&pool, &api_key, &signing, "user-rdm-rl").await;
    let rl_rpm = 3u32;
    for _ in 0..rl_rpm {
        let _ = redeem_invitation(RedeemInvitationParams {
            pool: &pool,
            ephemeral: ephemeral.as_ref(),
            key_prefix: "gs",
            tenant_id,
            code: "INVALID-CODE-XYZ",
            device_id: dev_rl,
            subject_id: sub_rl,
            rate_limit_rpm: rl_rpm,
            now,
        })
        .await;
    }
    // (rl_rpm + 1)-th call triggers RateLimited
    let res_rl = redeem_invitation(RedeemInvitationParams {
        pool: &pool,
        ephemeral: ephemeral.as_ref(),
        key_prefix: "gs",
        tenant_id,
        code: "INVALID-CODE-XYZ",
        device_id: dev_rl,
        subject_id: sub_rl,
        rate_limit_rpm: rl_rpm,
        now,
    })
    .await;
    assert!(matches!(
        res_rl,
        Err(InvitationUseCaseError::RateLimited { .. })
    ));
}
