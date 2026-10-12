//! Unit and integration tests for session, membership, sharing, and presence invariants (INV-*).
//!
//! Validates FR-SES-006, FR-SES-007, FR-MEM-003, FR-MEM-007, FR-SHR-001, FR-SHR-002, FR-SHR-004 (T-F1-04).
//! As mandated by docs/05 §11, each tested invariant has its ID in the test function name.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use time::OffsetDateTime;

use gs_domain::ids::{DeviceId, MemberId, SessionId, SubjectId, TenantId};
use gs_domain::membership::{
    Capabilities, Connectivity, MemberRole, MemberStatusKind, Membership, MembershipError,
    OwnerDepartureOutcome, SharingTransition, handle_owner_departure, validate_readmission,
};
use gs_domain::session::{Actor, ClosedReason, Session, SessionError, SessionMode, SessionStatus};

fn base_time() -> OffsetDateTime {
    OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1_700_000_000)
}

fn create_test_session(mode: SessionMode, created_by: Actor) -> Session {
    let now = base_time();
    Session::new(
        SessionId::new(),
        TenantId::new(),
        mode,
        Some(SubjectId::new()),
        created_by,
        now,
        now + time::Duration::seconds(3600),
        if mode == SessionMode::OneToOne { 2 } else { 10 },
        600,
        60,
        BTreeMap::new(),
        86400,
    )
    .expect("valid session")
}

// ============================================================================
// Session Invariants (INV-SES-*)
// ============================================================================

#[test]
fn test_inv_ses_1_inactive_session_cannot_accept_operations() {
    let now = base_time();
    let mut session = create_test_session(SessionMode::ManyToMany, Actor::Device(DeviceId::new()));

    assert!(session.is_active_at(now));

    // Close session
    session
        .close(ClosedReason::ClosedByMember, now)
        .expect("close succeeds");
    assert!(!session.is_active_at(now));

    // Attempting to extend expiration on closed session must fail
    let err = session.extend_expiration(now + time::Duration::seconds(7200), now, 86400);
    assert_eq!(err, Err(SessionError::InactiveSession));

    // Attempting to update max_members on closed session must fail
    let err = session.update_max_members(20, 2, 50, now);
    assert_eq!(err, Err(SessionError::InactiveSession));
}

#[test]
fn test_inv_ses_2_lazy_expiration_at_read_time() {
    let now = base_time();
    let session = create_test_session(SessionMode::ManyToMany, Actor::Device(DeviceId::new()));

    // Before expiration
    assert_eq!(session.effective_status(now), SessionStatus::Active);
    assert!(session.is_active_at(now));

    // Exactly at or after expires_at
    let after_expiry = session.expires_at;
    assert_eq!(
        session.effective_status(after_expiry),
        SessionStatus::Expired
    );
    assert!(!session.is_active_at(after_expiry));

    let well_after = after_expiry + time::Duration::seconds(60);
    assert_eq!(session.effective_status(well_after), SessionStatus::Expired);
    assert!(!session.is_active_at(well_after));
}

#[test]
fn test_inv_ses_3_close_idempotency_and_expired_session() {
    let now = base_time();
    let mut session = create_test_session(SessionMode::ManyToMany, Actor::Device(DeviceId::new()));

    // First close returns true (transition happened)
    let res = session
        .close(ClosedReason::ClosedByMember, now)
        .expect("first close ok");
    assert!(res);
    assert_eq!(session.status, SessionStatus::Closed);
    assert_eq!(session.closed_reason, Some(ClosedReason::ClosedByMember));
    let version = session.version;

    // Second close is idempotent: returns false, version does not change
    let res = session
        .close(ClosedReason::ClosedByMember, now)
        .expect("second close ok");
    assert!(!res);
    assert_eq!(session.version, version);

    // Closing an expired session returns false without error (INV-SES-3)
    let mut expired_session =
        create_test_session(SessionMode::ManyToMany, Actor::Device(DeviceId::new()));
    let after_expiry = expired_session.expires_at + time::Duration::seconds(10);
    let res = expired_session
        .close(ClosedReason::Admin, after_expiry)
        .expect("close on expired ok");
    assert!(!res);
    assert_eq!(expired_session.status, SessionStatus::Expired);
}

// ============================================================================
// Membership Invariants (INV-MEM-*)
// ============================================================================

#[test]
fn test_inv_mem_1_owner_capabilities_cannot_be_reduced() {
    let now = base_time();
    let mut owner = Membership::new_owner(
        MemberId::new(),
        SessionId::new(),
        TenantId::new(),
        SubjectId::new(),
        now,
    );

    assert_eq!(owner.capabilities, Capabilities::all());

    // Even an application cannot reduce owner capabilities
    let err = owner.update_capabilities(true, None, None, Capabilities::read_only(), now);
    assert_eq!(err, Err(MembershipError::OwnerCapabilitiesImmutable));

    // A manager cannot reduce owner capabilities
    let manager_caps = Capabilities::all();
    let err = owner.update_capabilities(
        false,
        Some(MemberId::new()),
        Some(&manager_caps),
        Capabilities::read_only(),
        now,
    );
    assert_eq!(err, Err(MembershipError::OwnerCapabilitiesImmutable));
}

#[test]
fn test_inv_mem_2_manage_cannot_grant_more_than_own_capabilities() {
    let now = base_time();
    let caller_member_id = MemberId::new();
    let caller_caps = Capabilities {
        publish: false,
        read: true,
        invite: true,
        manage: true,
    };

    let mut target = Membership::new_member(
        MemberId::new(),
        SessionId::new(),
        TenantId::new(),
        SubjectId::new(),
        Capabilities::read_only(),
        gs_domain::membership::JoinedVia::DirectAdd,
        now,
    );

    // Caller attempts to grant `publish` (which caller does not possess)
    let escalated_caps = Capabilities {
        publish: true, // Not in caller_caps!
        read: true,
        invite: false,
        manage: false,
    };

    let err = target.update_capabilities(
        false,
        Some(caller_member_id),
        Some(&caller_caps),
        escalated_caps,
        now,
    );
    assert_eq!(err, Err(MembershipError::CannotEscalateCapabilities));

    // Granting a valid subset succeeds
    let valid_caps = Capabilities {
        publish: false,
        read: true,
        invite: true,
        manage: false,
    };
    let res = target.update_capabilities(
        false,
        Some(caller_member_id),
        Some(&caller_caps),
        valid_caps,
        now,
    );
    assert!(res.is_ok());
    assert_eq!(target.capabilities, valid_caps);
}

#[test]
fn test_inv_mem_3_member_cannot_modify_own_capabilities() {
    let now = base_time();
    let own_member_id = MemberId::new();
    let own_caps = Capabilities::all();

    let mut member = Membership::new_member(
        own_member_id,
        SessionId::new(),
        TenantId::new(),
        SubjectId::new(),
        Capabilities::read_only(),
        gs_domain::membership::JoinedVia::DirectAdd,
        now,
    );

    // Member attempts to update their own capabilities
    let err = member.update_capabilities(
        false,
        Some(own_member_id),
        Some(&own_caps),
        Capabilities::all(),
        now,
    );
    assert_eq!(err, Err(MembershipError::CannotModifyOwnCapabilities));

    // Application can modify it
    let res = member.update_capabilities(true, None, None, Capabilities::all(), now);
    assert!(res.is_ok());
    assert_eq!(member.capabilities, Capabilities::all());
}

#[test]
fn test_inv_mem_4_revoking_publish_stops_sharing() {
    let now = base_time();
    let subject_id = SubjectId::new();
    let device_id = DeviceId::new();

    let mut member = Membership::new_member(
        MemberId::new(),
        SessionId::new(),
        TenantId::new(),
        subject_id,
        Capabilities::publish_read(),
        gs_domain::membership::JoinedVia::DirectAdd,
        now,
    );

    // Start sharing
    member
        .start_sharing(subject_id, device_id, now)
        .expect("sharing starts");
    assert!(member.sharing);
    assert_eq!(member.publisher_device_id, Some(device_id));

    // Removing publish capability must automatically stop sharing
    let stopped = member
        .update_capabilities(true, None, None, Capabilities::read_only(), now)
        .expect("capability update succeeds");

    assert!(stopped, "expected sharing stopped signal");
    assert!(!member.sharing);
    assert_eq!(member.publisher_device_id, None);
}

#[test]
fn test_inv_mem_6_at_most_one_active_membership_per_subject() {
    // Subject already active in the session cannot be readmitted
    let res = validate_readmission(Some(MemberStatusKind::Active), true);
    assert_eq!(res, Err(MembershipError::AlreadyActiveMember));

    let res = validate_readmission(Some(MemberStatusKind::Active), false);
    assert_eq!(res, Err(MembershipError::AlreadyActiveMember));
}

#[test]
fn test_inv_mem_7_active_members_capped_by_max_members() {
    let now = base_time();
    let mut session = create_test_session(SessionMode::ManyToMany, Actor::Device(DeviceId::new()));

    // Session has max_members = 10. Attempting to set max_members to 4 when active_members = 5 fails.
    let err = session.update_max_members(4, 5, 50, now);
    assert_eq!(
        err,
        Err(SessionError::InvalidMaxMembers(
            "new max_members cannot be less than active members count".to_string()
        ))
    );
}

// ============================================================================
// Sharing Invariants (INV-SHR-*)
// ============================================================================

#[test]
fn test_inv_shr_1_sharing_requires_active_publisher_device_of_same_subject() {
    let now = base_time();
    let subject_id = SubjectId::new();
    let device_id = DeviceId::new();

    let mut member = Membership::new_member(
        MemberId::new(),
        SessionId::new(),
        TenantId::new(),
        subject_id,
        Capabilities::publish_read(),
        gs_domain::membership::JoinedVia::DirectAdd,
        now,
    );

    // Default is NOT sharing (FR-SHR-001)
    assert!(!member.sharing);
    assert_eq!(member.publisher_device_id, None);

    // Started sharing sets publisher_device_id
    let transition = member
        .start_sharing(subject_id, device_id, now)
        .expect("start sharing ok");
    assert_eq!(transition, SharingTransition::Started(device_id));
    assert!(member.sharing);
    assert_eq!(member.publisher_device_id, Some(device_id));
}

#[test]
fn test_inv_shr_2_only_self_subject_activates_sharing() {
    let now = base_time();
    let subject_id = SubjectId::new();
    let other_subject = SubjectId::new();
    let device_id = DeviceId::new();

    let mut member = Membership::new_member(
        MemberId::new(),
        SessionId::new(),
        TenantId::new(),
        subject_id,
        Capabilities::publish_read(),
        gs_domain::membership::JoinedVia::DirectAdd,
        now,
    );

    // Another subject attempts to activate sharing -> rejected with permission denied
    let err = member.start_sharing(other_subject, device_id, now);
    assert_eq!(err, Err(MembershipError::OnlySelfCanActivateSharing));
    assert!(!member.sharing);
}

#[test]
fn test_inv_shr_3_same_device_put_sharing_true_is_idempotent() {
    let now = base_time();
    let subject_id = SubjectId::new();
    let device_id = DeviceId::new();

    let mut member = Membership::new_member(
        MemberId::new(),
        SessionId::new(),
        TenantId::new(),
        subject_id,
        Capabilities::publish_read(),
        gs_domain::membership::JoinedVia::DirectAdd,
        now,
    );

    member
        .start_sharing(subject_id, device_id, now)
        .expect("first start ok");
    let version = member.version;

    // Idempotent re-invocation from same device
    let transition = member
        .start_sharing(subject_id, device_id, now)
        .expect("idempotent start ok");
    assert_eq!(transition, SharingTransition::Unchanged);
    assert_eq!(member.version, version);
}

#[test]
fn test_inv_shr_4_transitions_generate_transfers_and_stops() {
    let now = base_time();
    let subject_id = SubjectId::new();
    let dev1 = DeviceId::new();
    let dev2 = DeviceId::new();

    let mut member = Membership::new_member(
        MemberId::new(),
        SessionId::new(),
        TenantId::new(),
        subject_id,
        Capabilities::publish_read(),
        gs_domain::membership::JoinedVia::DirectAdd,
        now,
    );

    // 1. Started
    let t1 = member.start_sharing(subject_id, dev1, now).unwrap();
    assert_eq!(t1, SharingTransition::Started(dev1));

    // 2. Transferred to another device of the same subject (FR-SHR-004)
    let t2 = member.start_sharing(subject_id, dev2, now).unwrap();
    assert_eq!(
        t2,
        SharingTransition::Transferred {
            previous_device: dev1,
            new_device: dev2,
        }
    );
    assert_eq!(member.publisher_device_id, Some(dev2));

    // 3. Stopped
    let t3 = member.stop_sharing(now);
    assert_eq!(t3, SharingTransition::Stopped);
    assert!(!member.sharing);
    assert_eq!(member.publisher_device_id, None);
}

// ============================================================================
// Presence Invariant (INV-PRS-1)
// ============================================================================

#[test]
fn test_inv_prs_1_non_sharing_member_never_reveals_connectivity_or_location() {
    let now = base_time();
    let subject_id = SubjectId::new();
    let dev = DeviceId::new();

    let mut member = Membership::new_member(
        MemberId::new(),
        SessionId::new(),
        TenantId::new(),
        subject_id,
        Capabilities::publish_read(),
        gs_domain::membership::JoinedVia::DirectAdd,
        now,
    );

    // Case 1: Member NOT sharing
    assert!(!member.sharing);
    let view = member.build_status_view(Some("ext_1".to_string()), true, Some(now));
    assert_eq!(view.connectivity, Connectivity::Unspecified);
    assert_eq!(view.last_location_at, None);

    // Case 2: Member SHARING
    member.start_sharing(subject_id, dev, now).unwrap();
    let view_sharing = member.build_status_view(Some("ext_1".to_string()), true, Some(now));
    assert_eq!(view_sharing.connectivity, Connectivity::Online);
    assert_eq!(view_sharing.last_location_at, Some(now));

    let view_offline = member.build_status_view(Some("ext_1".to_string()), false, Some(now));
    assert_eq!(view_offline.connectivity, Connectivity::Offline);
}

// ============================================================================
// Requirements Coverage Tests (FR-SES-*, FR-MEM-*)
// ============================================================================

#[test]
fn test_fr_ses_006_session_modes_defaults_and_limits() {
    // ONE_TO_ONE requires max_members == 2
    let now = base_time();
    let err = Session::new(
        SessionId::new(),
        TenantId::new(),
        SessionMode::OneToOne,
        Some(SubjectId::new()),
        Actor::Device(DeviceId::new()),
        now,
        now + time::Duration::seconds(3600),
        3, // Invalid for ONE_TO_ONE
        600,
        60,
        BTreeMap::new(),
        86400,
    );
    assert!(err.is_err());

    // Defaults by mode
    assert_eq!(
        Capabilities::default_for_mode(SessionMode::OneToOne, false),
        Capabilities::publish_read()
    );
    assert_eq!(
        Capabilities::default_for_mode(SessionMode::OneToMany, false),
        Capabilities::read_only()
    );
    assert_eq!(
        Capabilities::default_for_mode(SessionMode::ManyToMany, false),
        Capabilities::publish_read()
    );
    assert_eq!(
        Capabilities::default_for_mode(SessionMode::OneToMany, true),
        Capabilities::all()
    );
}

#[test]
fn test_fr_ses_007_owner_departure_transfer_to_oldest_manager() {
    let now = base_time();
    let owner_id = SubjectId::new();

    let m1_id = MemberId::new();
    let m1_subj = SubjectId::new();
    let m1 = Membership::new_member(
        m1_id,
        SessionId::new(),
        TenantId::new(),
        m1_subj,
        Capabilities::read_only(), // No manage
        gs_domain::membership::JoinedVia::DirectAdd,
        now,
    );

    let m2_id = MemberId::new();
    let m2_subj = SubjectId::new();
    let m2 = Membership::new_member(
        m2_id,
        SessionId::new(),
        TenantId::new(),
        m2_subj,
        Capabilities {
            publish: true,
            read: true,
            invite: true,
            manage: true, // Eligible manager
        },
        gs_domain::membership::JoinedVia::DirectAdd,
        now + time::Duration::seconds(10),
    );

    let mut members = vec![m1, m2];
    let outcome = handle_owner_departure(owner_id, &mut members, &Actor::Device(DeviceId::new()));

    assert_eq!(
        outcome,
        OwnerDepartureOutcome::Transferred {
            new_owner_subject_id: m2_subj,
            new_owner_member_id: m2_id,
        }
    );
    // Successor receives full owner capabilities (INV-MEM-1)
    assert_eq!(members[1].role, MemberRole::Owner);
    assert_eq!(members[1].capabilities, Capabilities::all());
}

#[test]
fn test_fr_ses_007_owner_departure_device_session_closes() {
    let now = base_time();
    let owner_id = SubjectId::new();

    // No member with manage
    let m1 = Membership::new_member(
        MemberId::new(),
        SessionId::new(),
        TenantId::new(),
        SubjectId::new(),
        Capabilities::read_only(),
        gs_domain::membership::JoinedVia::DirectAdd,
        now,
    );

    let mut members = vec![m1];
    let outcome = handle_owner_departure(owner_id, &mut members, &Actor::Device(DeviceId::new()));

    assert_eq!(
        outcome,
        OwnerDepartureOutcome::SessionClosed(ClosedReason::OwnerLeft)
    );
}

#[test]
fn test_fr_ses_007_owner_departure_application_session_stays_open() {
    let now = base_time();
    let owner_id = SubjectId::new();

    // No member with manage, but session was created by application
    let m1 = Membership::new_member(
        MemberId::new(),
        SessionId::new(),
        TenantId::new(),
        SubjectId::new(),
        Capabilities::read_only(),
        gs_domain::membership::JoinedVia::DirectAdd,
        now,
    );

    let mut members = vec![m1];
    let outcome = handle_owner_departure(
        owner_id,
        &mut members,
        &Actor::Application(gs_domain::ids::ApplicationId::new()),
    );

    assert_eq!(outcome, OwnerDepartureOutcome::NoOwnerSessionOpen);
}

#[test]
fn test_fr_mem_007_readmission_left_vs_revoked() {
    // Subject with last membership LEFT can return via invitation OR direct add
    assert!(validate_readmission(Some(MemberStatusKind::Left), false).is_ok());
    assert!(validate_readmission(Some(MemberStatusKind::Left), true).is_ok());

    // Subject with last membership REVOKED can ONLY return via direct add
    assert_eq!(
        validate_readmission(Some(MemberStatusKind::Revoked), false),
        Err(MembershipError::RevokedMemberCannotRedeemInvitation)
    );
    assert!(validate_readmission(Some(MemberStatusKind::Revoked), true).is_ok());
}
