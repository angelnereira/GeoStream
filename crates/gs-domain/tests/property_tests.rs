//! Property-based tests for sequence ordering, capabilities, boundaries, and IDs (docs/18 §3, NFR-MAINT-002).
#![forbid(unsafe_code)]

use proptest::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use time::OffsetDateTime;

use gs_domain::ids::*;
use gs_domain::invitation::*;
use gs_domain::location::*;
use gs_domain::membership::*;
use gs_domain::session::*;

proptest! {
    // -------------------------------------------------------------------------
    // Property 1: Sequence CAS Ordering Model (docs/18 §3, FR-LOC-003)
    // "para cualquier secuencia de publicaciones (con duplicados, permutaciones,
    // reconexiones simuladas), el conjunto aceptado es estrictamente creciente
    // y coincide con un modelo de referencia simple."
    // -------------------------------------------------------------------------
    #[test]
    fn prop_sequence_ordering_matches_reference_model(
        seqs in prop::collection::vec(1u64..10000u64, 1..50)
    ) {
        let mut store = InMemorySequenceStore::new(None);
        let mut reference_floor = 0u64;
        let mut accepted_history = Vec::new();

        for seq in seqs {
            let res = store.compare_and_set(seq);

            if seq > reference_floor {
                prop_assert_eq!(res, CasResult::Accepted);
                reference_floor = seq;
                accepted_history.push(seq);
            } else if seq == reference_floor {
                prop_assert_eq!(res, CasResult::Duplicate);
            } else {
                prop_assert_eq!(res, CasResult::Stale);
            }

            prop_assert_eq!(store.current_sequence(), Some(reference_floor));
        }

        // Verify strictly increasing accepted history
        for i in 1..accepted_history.len() {
            prop_assert!(accepted_history[i] > accepted_history[i - 1]);
        }
    }

    // -------------------------------------------------------------------------
    // Property 2: Capabilities Invariants (docs/18 §3, INV-MEM-1, INV-MEM-2, INV-MEM-3)
    // "ninguna secuencia de operaciones de miembros con manage produce un conjunto
    // de capacidades mayor que el del actor (INV-MEM-2)."
    // -------------------------------------------------------------------------
    #[test]
    fn prop_capabilities_modification_invariants(
        actor_publish in any::<bool>(),
        actor_read in any::<bool>(),
        actor_invite in any::<bool>(),
        actor_manage in any::<bool>(),
        req_publish in any::<bool>(),
        req_read in any::<bool>(),
        req_invite in any::<bool>(),
        req_manage in any::<bool>(),
        target_is_owner in any::<bool>(),
        actor_is_target in any::<bool>(),
    ) {
        let actor_caps = Capabilities {
            publish: actor_publish,
            read: actor_read,
            invite: actor_invite,
            manage: actor_manage,
        };

        let req_caps = Capabilities {
            publish: req_publish,
            read: req_read,
            invite: req_invite,
            manage: req_manage,
        };

        let session_id = SessionId::new();
        let tenant_id = TenantId::new();
        let target_subject_id = SubjectId::new();
        let target_member_id = MemberId::new();
        let actor_member_id = if actor_is_target {
            target_member_id
        } else {
            MemberId::new()
        };

        let now = OffsetDateTime::now_utc();
        let mut target_membership = if target_is_owner {
            Membership::new_owner(
                target_member_id,
                session_id,
                tenant_id,
                target_subject_id,
                now,
            )
        } else {
            Membership::new_member(
                target_member_id,
                session_id,
                tenant_id,
                target_subject_id,
                Capabilities::read_only(),
                JoinedVia::Invitation(InvitationId::new()),
                now,
            )
        };

        let result = target_membership.update_capabilities(
            false,
            Some(actor_member_id),
            Some(&actor_caps),
            req_caps,
            now,
        );

        if target_is_owner {
            // INV-MEM-1: owner capabilities cannot be modified
            prop_assert_eq!(result, Err(MembershipError::OwnerCapabilitiesImmutable));
        } else if actor_is_target {
            // INV-MEM-3: member cannot modify own capabilities
            prop_assert_eq!(result, Err(MembershipError::CannotModifyOwnCapabilities));
        } else if !actor_caps.manage {
            // Manage permission required
            prop_assert_eq!(result, Err(MembershipError::PermissionDenied));
        } else if !req_caps.is_subset_of(&actor_caps) {
            // INV-MEM-2: cannot grant more than own capabilities
            prop_assert_eq!(result, Err(MembershipError::CannotEscalateCapabilities));
        } else {
            // Must succeed
            prop_assert!(result.is_ok());
            prop_assert_eq!(target_membership.capabilities, req_caps);
            prop_assert!(target_membership.capabilities.is_subset_of(&actor_caps));
        }
    }

    // -------------------------------------------------------------------------
    // Property 3: Position Boundaries Validation (docs/18 §3, docs/09 §3.1)
    // "valores generados en los bordes ± ε producen exactamente las fronteras de 09 §3."
    // -------------------------------------------------------------------------
    #[test]
    fn prop_latitude_boundaries(lat in -180.0f64..180.0f64) {
        let is_valid = (-90.0..=90.0).contains(&lat) && lat.is_finite();
        let policy = LocationPolicy {
            max_horizontal_accuracy_m: 50000.0,
            reject_null_island: false,
            max_future_skew_ms: 5000,
            max_event_age_ms: 120000,
            max_samples_per_publish: 50,
        };
        let pos_res = Position::new(lat, 10.0, 10.0, None, None, None, None, None, None, &policy);
        prop_assert_eq!(pos_res.is_ok(), is_valid);
    }

    #[test]
    fn prop_longitude_boundaries(lon in -360.0f64..360.0f64) {
        let is_valid = (-180.0..=180.0).contains(&lon) && lon.is_finite();
        let policy = LocationPolicy {
            max_horizontal_accuracy_m: 50000.0,
            reject_null_island: false,
            max_future_skew_ms: 5000,
            max_event_age_ms: 120000,
            max_samples_per_publish: 50,
        };
        let pos_res = Position::new(10.0, lon, 10.0, None, None, None, None, None, None, &policy);
        prop_assert_eq!(pos_res.is_ok(), is_valid);
    }

    #[test]
    fn prop_null_island_rejection(
        lat in prop::sample::select(vec![0.0f64, -0.0f64, 0.000001f64]),
        lon in prop::sample::select(vec![0.0f64, -0.0f64, 0.000001f64]),
        reject_null_island in any::<bool>(),
    ) {
        let policy = LocationPolicy {
            max_horizontal_accuracy_m: 50000.0,
            reject_null_island,
            max_future_skew_ms: 5000,
            max_event_age_ms: 120000,
            max_samples_per_publish: 50,
        };

        let now = OffsetDateTime::now_utc();
        let raw = RawLocationSample {
            sequence: Some(1),
            sequence_invalid: false,
            captured_at: Some(now),
            captured_at_invalid: false,
            position: Some(RawPosition {
                latitude: Some(lat),
                longitude: Some(lon),
                horizontal_accuracy_m: Some(10.0),
                altitude_m: None,
                vertical_accuracy_m: None,
                speed_mps: None,
                speed_accuracy_mps: None,
                heading_deg: None,
                heading_accuracy_deg: None,
            }),
            quality: Some(RawQuality {
                source: Some(LocationSource::Gnss),
                kind: Some(LocationKind::Measured),
                is_mock: Some(false),
                motion: Some(MotionActivity::Stationary),
            }),
        };

        let res = validate_sample(&raw, &policy, now);

        let is_null_island = lat == 0.0 && lon == 0.0;
        if reject_null_island && is_null_island {
            prop_assert!(res.is_err());
            prop_assert_eq!(res.unwrap_err().field, "position");
        } else {
            prop_assert!(res.is_ok());
        }
    }

    // -------------------------------------------------------------------------
    // Property 4: Crockford Base32 Normalization and Code Generation (docs/05 §6)
    // -------------------------------------------------------------------------
    #[test]
    fn prop_crockford_normalization_invariance(
        raw_chars in prop::collection::vec(
            prop::sample::select(b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".to_vec()),
            16
        ),
        hyphen_positions in prop::collection::vec(0usize..16usize, 0..3),
        space_positions in prop::collection::vec(0usize..16usize, 0..3),
    ) {
        let mut input = String::new();
        let hyphens: BTreeSet<usize> = hyphen_positions.into_iter().collect();
        let spaces: BTreeSet<usize> = space_positions.into_iter().collect();

        for (idx, b) in raw_chars.iter().enumerate() {
            if hyphens.contains(&idx) {
                input.push('-');
            }
            if spaces.contains(&idx) {
                input.push(' ');
            }
            let c = *b as char;
            if idx % 2 == 0 {
                input.push(c.to_ascii_lowercase());
            } else {
                input.push(c);
            }
        }

        let normalized = normalize_invitation_code(&input).expect("normalization should succeed");
        prop_assert_eq!(normalized.len(), 16);

        // Every char in normalized must be in Crockford alphabet
        for c in normalized.chars() {
            prop_assert!(CROCKFORD_ALPHABET.contains(&(c as u8)));
        }

        // Hashing produces a 64-hex lowercase string
        let hash = hash_invitation_code(&normalized);
        prop_assert_eq!(hash.len(), 64);
        prop_assert!(hash.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    // -------------------------------------------------------------------------
    // Property 5: Prefixed Typed Entity IDs Roundtrip
    // -------------------------------------------------------------------------
    #[test]
    fn prop_all_entity_ids_roundtrip_codec(
        _dummy in any::<u8>()
    ) {
        // Test TenantId
        let id1 = TenantId::new();
        let s1 = id1.to_string();
        let parsed1 = TenantId::from_str(&s1).expect("parse TenantId");
        prop_assert_eq!(id1, parsed1);
        prop_assert!(s1.starts_with("ten_"));

        // Test SessionId
        let id2 = SessionId::new();
        let s2 = id2.to_string();
        let parsed2 = SessionId::from_str(&s2).expect("parse SessionId");
        prop_assert_eq!(id2, parsed2);
        prop_assert!(s2.starts_with("ses_"));

        // Test SubjectId
        let id3 = SubjectId::new();
        let s3 = id3.to_string();
        let parsed3 = SubjectId::from_str(&s3).expect("parse SubjectId");
        prop_assert_eq!(id3, parsed3);
        prop_assert!(s3.starts_with("sub_"));

        // Test DeviceId
        let id4 = DeviceId::new();
        let s4 = id4.to_string();
        let parsed4 = DeviceId::from_str(&s4).expect("parse DeviceId");
        prop_assert_eq!(id4, parsed4);
        prop_assert!(s4.starts_with("dev_"));

        // Test MemberId
        let id5 = MemberId::new();
        let s5 = id5.to_string();
        let parsed5 = MemberId::from_str(&s5).expect("parse MemberId");
        prop_assert_eq!(id5, parsed5);
        prop_assert!(s5.starts_with("mem_"));

        // Test InvitationId
        let id6 = InvitationId::new();
        let s6 = id6.to_string();
        let parsed6 = InvitationId::from_str(&s6).expect("parse InvitationId");
        prop_assert_eq!(id6, parsed6);
        prop_assert!(s6.starts_with("inv_"));
    }

    // -------------------------------------------------------------------------
    // Property 6: Session Lazy Expiration and Close State Machine (INV-SES-1..3)
    // -------------------------------------------------------------------------
    #[test]
    fn prop_session_lazy_expiration(
        ttl_seconds in 60u32..86400u32,
        check_offset in -3600i64..3600i64,
    ) {
        let created_at = OffsetDateTime::now_utc();
        let expires_at = created_at + time::Duration::seconds(ttl_seconds as i64);

        let mut session = Session::new(
            SessionId::new(),
            TenantId::new(),
            SessionMode::OneToMany,
            None,
            Actor::Application(ApplicationId::new()),
            created_at,
            expires_at,
            100,
            600,
            60,
            BTreeMap::new(),
            ttl_seconds,
        ).expect("session creation");

        let check_time = expires_at + time::Duration::seconds(check_offset);
        let eff = session.effective_status(check_time);

        if check_offset >= 0 {
            prop_assert_eq!(eff, SessionStatus::Expired);
            prop_assert!(!session.is_active_at(check_time));
        } else {
            prop_assert_eq!(eff, SessionStatus::Active);
            prop_assert!(session.is_active_at(check_time));
        }

        // Closing an active session transitions to Closed and returns Ok(true)
        if session.is_active_at(created_at) {
            let closed = session.close(ClosedReason::Admin, created_at).expect("close");
            prop_assert!(closed);
            prop_assert_eq!(session.status, SessionStatus::Closed);

            // Re-closing is idempotent (INV-SES-3) and returns Ok(false)
            let reclosed = session.close(ClosedReason::Admin, created_at).expect("reclose");
            prop_assert!(!reclosed);
            prop_assert_eq!(session.status, SessionStatus::Closed);
        }
    }
}
