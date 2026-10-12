//! Unit tests for device token issuance and verification (docs/06 §4, contracts/test-vectors/token_claims.json).
#![forbid(unsafe_code)]

use std::str::FromStr;

use gs_auth::jwt::{
    DeviceTokenClaims, JwtError, SigningKeySet, TOKEN_AUDIENCE, issue_device_token,
    verify_device_token,
};
use gs_auth::{generate_signing_key, parse_public_pem};
use gs_domain::error::AsErrorCode;
use gs_domain::error::ErrorCode;
use gs_domain::ids::{ApplicationId, DeviceId, SubjectId, TenantId};
use jsonwebtoken::{Algorithm, Header, decode_header};
use time::{Duration, OffsetDateTime};

const ISSUER: &str = "https://geostream.example.com";
const VECTOR_ISS: &str = "https://geostream.example.com";
const KID: &str = "gs-2026-01";

fn claims_at(issued_at: OffsetDateTime, ttl_seconds: u32) -> DeviceTokenClaims {
    DeviceTokenClaims::new(
        ISSUER,
        SubjectId::from_str("sub_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a").expect("subject id"),
        TenantId::from_str("ten_0192b6c0a1e47c3f9d2a7b5e8f1c0d3b").expect("tenant id"),
        ApplicationId::from_str("app_0192b6c0a1e47c3f9d2a7b5e8f1c0d3c").expect("application id"),
        DeviceId::from_str("dev_0192b6c0a1e47c3f9d2a7b5e8f1c0d3d").expect("device id"),
        vec![
            "sessions:create".to_string(),
            "invitations:redeem".to_string(),
            "stream:connect".to_string(),
            "sharing:write".to_string(),
        ],
        0,
        issued_at,
        ttl_seconds,
    )
    .expect("claims")
}

fn key_set_with(kid: &str) -> (SigningKeySet, ed25519_dalek::SigningKey) {
    let dir = std::env::temp_dir().join(format!(
        "gs-auth-jwt-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let generated = generate_signing_key(kid).expect("generate signing key");
    generated.write_pair(&dir).expect("write signing key pair");

    let keys = SigningKeySet::load_from_dir(&dir).expect("load key set");
    let signing = gs_auth::parse_private_pem(generated.private_pem()).expect("parse private key");
    let _ = std::fs::remove_dir_all(&dir);
    (keys, signing)
}

#[test]
fn test_issued_claims_match_the_vector_of_token_claims_json() {
    // covers: FR-AUTH-003, contracts/test-vectors/token_claims.json
    let vector: serde_json::Value = serde_json::from_str(include_str!(
        "../../../contracts/test-vectors/token_claims.json"
    ))
    .expect("vector must be valid json");
    let expected = &vector["claims"];

    let issued_at = OffsetDateTime::from_unix_timestamp(
        vector["claims"]["iat"]
            .as_i64()
            .expect("iat must be an integer"),
    )
    .expect("issue instant");
    let issued = issue_device_token(
        &ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]),
        KID,
        &claims_at(issued_at, 900),
    )
    .expect("issue token");

    let header = decode_header(&issued.token).expect("decode header");
    assert_eq!(header.alg, Algorithm::EdDSA);
    assert_eq!(header.kid.as_deref(), Some(KID));
    assert_eq!(header.typ.as_deref(), Some("JWT"));

    // Claims are exactly those of the vector: no extra claim, none missing.
    let decoded: serde_json::Value = {
        let mut parts = issued.token.split('.');
        let payload = parts.nth(1).expect("payload part");
        use base64::Engine;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .expect("payload is base64url");
        serde_json::from_slice(&bytes).expect("payload is json")
    };
    for key in expected.as_object().expect("claims object").keys() {
        assert!(
            decoded.get(key).is_some(),
            "missing claim {key} in {decoded}"
        );
    }
    for key in decoded.as_object().expect("claims object").keys() {
        assert!(
            expected.get(key).is_some(),
            "unexpected extra claim {key} in {decoded}"
        );
    }
    assert_eq!(decoded["iss"], VECTOR_ISS);
    assert_eq!(decoded["aud"], TOKEN_AUDIENCE);
    assert_eq!(decoded["sub"], "sub_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a");
    assert_eq!(decoded["tid"], "ten_0192b6c0a1e47c3f9d2a7b5e8f1c0d3b");
    assert_eq!(decoded["aid"], "app_0192b6c0a1e47c3f9d2a7b5e8f1c0d3c");
    assert_eq!(decoded["did"], "dev_0192b6c0a1e47c3f9d2a7b5e8f1c0d3d");
    assert_eq!(decoded["scp"].as_array().map(Vec::len), Some(4));
    assert_eq!(decoded["sep"], 0);
    assert_eq!(
        decoded["exp"].as_i64(),
        Some(issued_at.unix_timestamp() + 900)
    );

    // Every required claim of the vector is present (contracts/test-vectors/token_claims.json).
    for required in vector["required_claims"]
        .as_array()
        .expect("required claims")
    {
        assert!(
            decoded
                .get(required.as_str().expect("claim name"))
                .is_some(),
            "required claim {required} is missing"
        );
    }
}

#[test]
fn test_roundtrip_issue_and_verify() {
    // covers: FR-AUTH-003, docs/06 §4.3
    let (keys, signing) = key_set_with(KID);
    let claims = claims_at(OffsetDateTime::now_utc(), 900);
    let issued = issue_device_token(&signing, KID, &claims).expect("issue token");

    let verified = verify_device_token(&issued.token, &keys, ISSUER, 30).expect("verify token");
    assert_eq!(verified.claims, claims);
    assert_eq!(verified.claims.sep, 0);
    assert!(verified.claims.has_scope("sessions:create"));
    assert!(!verified.claims.has_scope("locations:read"));
}

#[test]
fn test_verification_rejects_expired_and_future_tokens() {
    // covers: FR-AUTH-003, docs/06 §4.3
    let (keys, signing) = key_set_with(KID);

    let expired = issue_device_token(
        &signing,
        KID,
        &claims_at(OffsetDateTime::now_utc() - Duration::minutes(10), 60),
    )
    .expect("issue expired token");
    let error = verify_device_token(&expired.token, &keys, ISSUER, 0)
        .expect_err("expired token must be rejected");
    assert_eq!(error.error_code(), ErrorCode::TokenExpired);
    assert!(matches!(error, JwtError::Expired));

    let future = issue_device_token(
        &signing,
        KID,
        &claims_at(OffsetDateTime::now_utc() + Duration::minutes(5), 900),
    )
    .expect("issue future token");
    let error = verify_device_token(&future.token, &keys, ISSUER, 0)
        .expect_err("token from the future must be rejected");
    assert!(matches!(error, JwtError::NotYetValid));
    assert_eq!(error.error_code(), ErrorCode::Unauthenticated);
}

#[test]
fn test_verification_rejects_unknown_kid_and_other_signing_key() {
    // covers: FR-AUTH-003, docs/06 §4.3
    let (keys, signing) = key_set_with(KID);
    let (other_keys, _) = key_set_with("gs-2026-02");

    let issued = issue_device_token(&signing, KID, &claims_at(OffsetDateTime::now_utc(), 900))
        .expect("issue token");

    // A token signed with a key that is not in the verification set is rejected.
    let error = verify_device_token(&issued.token, &other_keys, ISSUER, 30)
        .expect_err("unknown kid must be rejected");
    assert!(matches!(error, JwtError::UnknownKid));
    assert_eq!(error.error_code(), ErrorCode::Unauthenticated);

    // A token without kid is rejected.
    let header = Header::new(Algorithm::EdDSA);
    use ed25519_dalek::pkcs8::EncodePrivateKey;
    let private_der = signing.to_pkcs8_der().expect("private der");
    let encoding_key = jsonwebtoken::EncodingKey::from_ed_der(private_der.as_bytes());
    let tampered = jsonwebtoken::encode(
        &header,
        &claims_at(OffsetDateTime::now_utc(), 900),
        &encoding_key,
    )
    .expect("encode without kid");
    let error = verify_device_token(&tampered, &keys, ISSUER, 30)
        .expect_err("missing kid must be rejected");
    assert!(matches!(error, JwtError::UnknownKid));
}

#[test]
fn test_verification_rejects_wrong_issuer_audience_and_tampered_payload() {
    // covers: FR-AUTH-003, docs/06 §4.3
    let (keys, signing) = key_set_with(KID);
    let issued = issue_device_token(&signing, KID, &claims_at(OffsetDateTime::now_utc(), 900))
        .expect("issue token");

    assert!(verify_device_token(&issued.token, &keys, "https://other.example.com", 30).is_err());
    assert!(verify_device_token("not-a-jwt", &keys, ISSUER, 30).is_err());

    // A different audience in the signed claims is caught by validation.
    let mut wrong_audience = claims_at(OffsetDateTime::now_utc(), 900);
    wrong_audience.aud = "other".to_string();
    let issued = issue_device_token(&signing, KID, &wrong_audience).expect("issue token");
    assert!(verify_device_token(&issued.token, &keys, ISSUER, 30).is_err());
}

#[test]
fn test_rotated_key_sets_verify_tokens_signed_by_any_pair() {
    // covers: FR-AUTH-004, docs/06 §4.4
    let dir = std::env::temp_dir().join(format!(
        "gs-auth-jwt-rotation-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);

    let old = generate_signing_key("gs-2026-01").expect("generate old pair");
    old.write_pair(&dir).expect("write old pair");

    // A token signed with the old pair stays valid while the new pair is added.
    let old_token = issue_device_token(
        &gs_auth::parse_private_pem(old.private_pem()).expect("parse old key"),
        "gs-2026-01",
        &claims_at(OffsetDateTime::now_utc(), 900),
    )
    .expect("issue with old pair");

    let new = generate_signing_key("gs-2026-02").expect("generate new pair");
    new.write_pair(&dir).expect("write new pair");

    let mut keys = SigningKeySet::load_from_dir(&dir).expect("load rotated key set");
    keys.set_active_kid("gs-2026-02")
        .expect("select active kid");
    assert!(keys.active_signing_key().is_some());

    let verified = verify_device_token(&old_token.token, &keys, ISSUER, 30)
        .expect("old token must stay valid during the rotation window");
    assert_eq!(verified.claims, old_token.claims);

    let (active_kid, active_key) = keys.active_signing_key().expect("active signing key");
    let new_token = issue_device_token(
        active_key,
        active_kid,
        &claims_at(OffsetDateTime::now_utc(), 900),
    )
    .expect("issue with new pair");
    assert!(verify_device_token(&new_token.token, &keys, ISSUER, 30).is_ok());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_active_kid_without_private_key_is_rejected() {
    // covers: FR-AUTH-004, docs/06 §4.4
    let (mut keys, _) = key_set_with(KID);
    let error = keys
        .set_active_kid("gs-missing")
        .expect_err("unknown active kid must fail");
    assert!(matches!(error, JwtError::MissingSigningKey { .. }));
    assert!(keys.active_signing_key().is_none());
}

#[test]
fn test_claims_forbid_unknown_fields() {
    // covers: FR-AUTH-003, docs/06 §4.2
    let claims = claims_at(OffsetDateTime::now_utc(), 900);
    let mut value = serde_json::to_value(&claims).expect("serialize claims");
    value.as_object_mut().expect("claims object").insert(
        "custom".to_string(),
        serde_json::Value::String("nope".to_string()),
    );
    let error = serde_json::from_value::<DeviceTokenClaims>(value)
        .expect_err("unknown claims must be rejected");
    assert!(
        error.to_string().contains("unknown field"),
        "unexpected error: {error}"
    );
}

#[test]
fn test_public_pem_is_reusable_as_verification_key() {
    // covers: FR-AUTH-004, docs/06 §4.4
    let generated = generate_signing_key("gs-verify").expect("generate pair");
    let verifying = parse_public_pem(&generated.public_pem).expect("parse public pem");
    assert_eq!(
        verifying.to_bytes(),
        gs_auth::parse_private_pem(generated.private_pem())
            .expect("parse private pem")
            .verifying_key()
            .to_bytes()
    );
}
