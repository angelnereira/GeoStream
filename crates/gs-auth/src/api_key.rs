//! API key secret generation, hashing and credential formatting (docs/06_IDENTITY_AUTH.md §3.1–§3.2).
//!
//! Verification of an incoming credential (constant-time comparison) belongs to
//! the authentication middleware (T-F2-04) and is deliberately not implemented here.
#![forbid(unsafe_code)]

use base64::Engine;
use rand::Rng;
use sha2::{Digest, Sha256};
use std::str::FromStr;
use subtle::ConstantTimeEq;
use thiserror::Error;
use uuid::Uuid;

use gs_domain::ids::ApiKeyId;

/// Entropy of the API key secret in bytes (docs/06 §3.1).
pub const SECRET_BYTES: usize = 32;

/// Length of the key id payload in lowercase hex: UUIDv7 without hyphens (docs/06 §3.1).
pub const KEY_ID_HEX_LEN: usize = 32;

/// Length of the base64url secret without padding (docs/06 §3.1).
pub const SECRET_LEN: usize = 43;

/// Credential prefix of application API keys (docs/06 §3.1).
pub const CREDENTIAL_PREFIX: &str = "gsk";

/// Errors produced while parsing or verifying an API key credential (docs/06 §3.1–§3.2).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ApiKeyError {
    /// The credential does not match `gsk_<32 hex>.<43 base64url>`.
    #[error("invalid api key credential format")]
    InvalidCredential,

    /// The secret does not match the stored hash.
    #[error("api key secret does not match")]
    SecretMismatch,
}

impl gs_domain::error::AsErrorCode for ApiKeyError {
    fn error_code(&self) -> gs_domain::error::ErrorCode {
        gs_domain::error::ErrorCode::Unauthenticated
    }
}

/// Generates a new API key secret: 32 CSPRNG bytes encoded as base64url without padding.
///
/// The result is exactly [`SECRET_LEN`] characters of `[A-Za-z0-9_-]` (docs/06 §3.1).
pub fn generate_secret() -> String {
    let mut bytes = [0u8; SECRET_BYTES];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Computes the SHA-256 digest of a secret, which is what the database stores (docs/06 §3.2).
///
/// SHA-256 (not a slow KDF) is sufficient because the secret carries 256 bits of entropy.
pub fn hash_secret(secret: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// Builds the complete credential shown once at creation time: `gsk_<key_id>.<secret>` (docs/06 §3.1).
pub fn format_credential(key_id: ApiKeyId, secret: &str) -> String {
    format!(
        "{CREDENTIAL_PREFIX}_{}.{secret}",
        key_id.into_uuid().simple()
    )
}

/// Splits a credential into its key id and secret (docs/06 §3.1).
///
/// The shape is `^gsk_[0-9a-f]{32}\.[A-Za-z0-9_-]{43}$`; anything else is rejected without
/// touching the database, as `docs/06` §3.1 requires.
pub fn parse_credential(credential: &str) -> Result<(ApiKeyId, &str), ApiKeyError> {
    let (key_id, secret) = credential
        .split_once('.')
        .ok_or(ApiKeyError::InvalidCredential)?;
    let key_id = key_id
        .strip_prefix(CREDENTIAL_PREFIX)
        .and_then(|rest| rest.strip_prefix('_'))
        .ok_or(ApiKeyError::InvalidCredential)?;

    if key_id.len() != KEY_ID_HEX_LEN
        || !key_id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || secret.len() != SECRET_LEN
        || !secret
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(ApiKeyError::InvalidCredential);
    }

    let key_id = Uuid::from_str(key_id)
        .map(ApiKeyId::from_uuid)
        .map_err(|_| ApiKeyError::InvalidCredential)?;
    Ok((key_id, secret))
}

/// Verifies a credential against the stored hash in constant time (docs/06 §3.2).
///
/// Returns the key id on success. The comparison uses `subtle::ConstantTimeEq` so a wrong
/// secret cannot be recovered from timing differences.
pub fn verify_credential(
    credential: &str,
    stored_secret_sha256: &[u8; 32],
) -> Result<ApiKeyId, ApiKeyError> {
    let (key_id, secret) = parse_credential(credential)?;
    let computed = hash_secret(secret);
    let matches = computed
        .as_slice()
        .ct_eq(stored_secret_sha256.as_slice())
        .unwrap_u8()
        == 1;
    if !matches {
        return Err(ApiKeyError::SecretMismatch);
    }
    Ok(key_id)
}
