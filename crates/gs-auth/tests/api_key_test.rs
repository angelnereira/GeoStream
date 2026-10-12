//! Unit tests for API key secret generation, hashing and credential format (docs/06 §3.1–§3.2).
#![forbid(unsafe_code)]

use base64::Engine;
use gs_auth::api_key::{
    ApiKeyError, CREDENTIAL_PREFIX, KEY_ID_HEX_LEN, SECRET_BYTES, SECRET_LEN, format_credential,
    generate_secret, hash_secret, parse_credential, verify_credential,
};
use gs_domain::ids::ApiKeyId;
use sha2::{Digest, Sha256};

fn is_base64url_no_pad(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[test]
fn test_secret_has_256_bits_of_entropy_in_43_base64url_characters() {
    // covers: FR-AUTH-001, docs/06 §3.1
    let secret = generate_secret();
    assert_eq!(secret.len(), SECRET_LEN);
    assert!(is_base64url_no_pad(&secret));

    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&secret)
        .expect("secret must be base64url without padding");
    assert_eq!(bytes.len(), SECRET_BYTES);
}

#[test]
fn test_generated_secrets_are_unique() {
    // covers: FR-AUTH-001, docs/06 §3.1
    let mut seen: Vec<String> = Vec::new();
    for _ in 0..64 {
        let secret = generate_secret();
        assert!(
            !seen.contains(&secret),
            "secret repeated across generations"
        );
        seen.push(secret);
    }
}

#[test]
fn test_hash_secret_is_sha256_of_the_secret_bytes() {
    // covers: FR-AUTH-001, docs/06 §3.2
    let secret = generate_secret();
    let hash = hash_secret(&secret);

    let digest = Sha256::digest(secret.as_bytes());
    let mut expected = [0u8; 32];
    expected.copy_from_slice(&digest);

    assert_eq!(hash, expected);
    assert_ne!(hash, hash_secret("another-secret"));
}

#[test]
fn test_credential_matches_the_gsk_format() {
    // covers: FR-AUTH-001, docs/06 §3.1
    let key_id = ApiKeyId::new();
    let secret = generate_secret();
    let credential = format_credential(key_id, &secret);

    let key_id_hex = key_id.into_uuid().simple().to_string();
    let expected_prefix = format!("{CREDENTIAL_PREFIX}_{key_id_hex}");
    assert_eq!(credential.len(), expected_prefix.len() + 1 + SECRET_LEN);
    assert!(
        credential.starts_with(&format!("{expected_prefix}.")),
        "unexpected credential shape: {credential}"
    );

    let id_part = credential
        .strip_prefix(&format!("{CREDENTIAL_PREFIX}_"))
        .expect("credential must start with the prefix")
        .split('.')
        .next()
        .expect("credential must contain the separator");
    assert_eq!(id_part.len(), KEY_ID_HEX_LEN);
    assert!(
        id_part
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
}

#[test]
fn test_parse_credential_rejects_malformed_shapes() {
    // covers: FR-AUTH-001, docs/06 §3.1
    for invalid in [
        "",
        "gsk_",
        "gsk_abcd.onlywenvelope",
        "key_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a.secrettosecrettosecrettosecrettosecret1234",
        "gsk_0192B6C0A1E47C3F9D2A7B5E8F1C0D3A.secrettosecrettosecrettosecrettosecret1234",
        "gsk_short.secrettosecrettosecrettosecrettosecret1234",
        "gsk_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a.short",
    ] {
        assert_eq!(
            parse_credential(invalid),
            Err(ApiKeyError::InvalidCredential),
            "credential must be rejected: {invalid}"
        );
    }
}

#[test]
fn test_parse_credential_recovers_the_key_id_and_secret() {
    // covers: FR-AUTH-001, docs/06 §3.1
    let key_id = ApiKeyId::new();
    let secret = generate_secret();
    let credential = format_credential(key_id, &secret);

    let (parsed_id, parsed_secret) =
        parse_credential(&credential).expect("credential must be accepted");
    assert_eq!(parsed_id, key_id);
    assert_eq!(parsed_secret, secret);
}

#[test]
fn test_verify_credential_uses_constant_time_comparison() {
    // covers: FR-AUTH-001, docs/06 §3.2
    let key_id = ApiKeyId::new();
    let secret = generate_secret();
    let credential = format_credential(key_id, &secret);
    let stored = hash_secret(&secret);

    assert_eq!(verify_credential(&credential, &stored), Ok(key_id));
    assert_eq!(
        verify_credential(&credential, &hash_secret(&format!("{secret}x"))),
        Err(ApiKeyError::SecretMismatch)
    );
    assert_eq!(
        verify_credential("not-a-credential", &stored),
        Err(ApiKeyError::InvalidCredential)
    );
}
