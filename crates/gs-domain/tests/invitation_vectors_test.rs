//! Tests for invitation code normalization, generation and hashing against contracts/test-vectors/invitation_codes.json.
//!
//! Validates FR-INV-001 (base, T-F1-05).
#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;
use time::OffsetDateTime;

use gs_domain::ids::{InvitationId, SessionId, TenantId};
use gs_domain::invitation::{
    Invitation, InvitationCodeSecret, generate_invitation_code, hash_invitation_code,
    normalize_invitation_code,
};
use gs_domain::membership::Capabilities;
use gs_domain::session::Actor;

fn find_repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("Failed to locate repo root from CARGO_MANIFEST_DIR")
        .to_path_buf()
}

#[test]
fn test_all_invitation_code_vectors() {
    let repo_root = find_repo_root();
    let vector_path = repo_root.join("contracts/test-vectors/invitation_codes.json");
    let content = fs::read_to_string(&vector_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", vector_path.display()));
    let root: serde_json::Value =
        serde_json::from_str(&content).expect("Failed to parse invitation_codes.json");

    let cases = root
        .get("cases")
        .and_then(|v| v.as_array())
        .expect("cases array missing in invitation_codes.json");

    for case in cases {
        let input = case
            .get("input")
            .and_then(|v| v.as_str())
            .expect("input missing in case");

        let expected_normalized = case.get("normalized").and_then(|v| v.as_str());

        let result = normalize_invitation_code(input);

        match expected_normalized {
            Some(expected) => {
                assert_eq!(
                    result.as_deref(),
                    Ok(expected),
                    "Failed for valid input: '{input}'"
                );
            }
            None => {
                assert!(
                    result.is_err(),
                    "Expected invalid result for input '{input}', but got: {result:?}"
                );
            }
        }
    }

    // Hash vector
    let hash_obj = root
        .get("hash")
        .expect("hash object missing in invitation_codes.json");
    let norm_input = hash_obj
        .get("normalized")
        .and_then(|v| v.as_str())
        .expect("hash.normalized missing");
    let expected_hash = hash_obj
        .get("sha256_hex")
        .and_then(|v| v.as_str())
        .expect("hash.sha256_hex missing");

    let actual_hash = hash_invitation_code(norm_input);
    assert_eq!(actual_hash, expected_hash, "Hash vector mismatch");
}

#[test]
fn test_invitation_code_generation_produces_normalizable_codes() {
    for _ in 0..100 {
        let code = generate_invitation_code();
        assert_eq!(
            code.len(),
            19,
            "Formatted code must be 19 chars (XXXX-XXXX-XXXX-XXXX)"
        );
        assert_eq!(&code[4..5], "-");
        assert_eq!(&code[9..10], "-");
        assert_eq!(&code[14..15], "-");

        let normalized = normalize_invitation_code(&code)
            .expect("Generated code must always normalize successfully");
        assert_eq!(normalized.len(), 16);
    }
}

#[test]
fn test_invitation_secret_redaction() {
    let secret = InvitationCodeSecret::new("ABCD-EFGH-JKMN-PQRS".to_string());
    let debug_str = format!("{secret:?}");
    let display_str = format!("{secret}");

    assert_eq!(debug_str, "InvitationCodeSecret(<redacted>)");
    assert_eq!(display_str, "<redacted>");
    assert!(!debug_str.contains("ABCD"));
    assert!(!display_str.contains("ABCD"));
}

#[test]
fn test_invitation_entity_lifecycle() {
    let now = OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1_700_000_000);
    let session_expiry = now + time::Duration::seconds(86400);

    let mut invitation = Invitation::new(
        InvitationId::new(),
        SessionId::new(),
        TenantId::new(),
        hash_invitation_code("ABCDEFGHJKMNPQRS"),
        Capabilities::read_only(),
        2, // max_uses
        now + time::Duration::seconds(3600),
        Actor::Device(gs_domain::ids::DeviceId::new()),
        now,
        session_expiry,
        10,
        86400,
    )
    .expect("valid invitation");

    assert!(invitation.is_redeemable_at(now));

    // First redemption
    invitation.redeem(now).expect("first redemption succeeds");
    assert_eq!(invitation.uses, 1);
    assert!(invitation.is_redeemable_at(now));

    // Second redemption reaches max_uses
    invitation.redeem(now).expect("second redemption succeeds");
    assert_eq!(invitation.uses, 2);
    assert!(!invitation.is_redeemable_at(now));

    // Third redemption fails
    assert!(invitation.redeem(now).is_err());

    // Revocation
    invitation.revoke(now).expect("revocation succeeds");
    assert!(!invitation.is_redeemable_at(now));
}
