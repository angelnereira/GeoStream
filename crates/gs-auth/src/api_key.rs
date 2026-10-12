//! API key secret generation, hashing and credential formatting (docs/06_IDENTITY_AUTH.md §3.1–§3.2).
//!
//! Verification of an incoming credential (constant-time comparison) belongs to
//! the authentication middleware (T-F2-04) and is deliberately not implemented here.
#![forbid(unsafe_code)]

use base64::Engine;
use rand::Rng;
use sha2::{Digest, Sha256};

use gs_domain::ids::ApiKeyId;

/// Entropy of the API key secret in bytes (docs/06 §3.1).
pub const SECRET_BYTES: usize = 32;

/// Length of the key id payload in lowercase hex: UUIDv7 without hyphens (docs/06 §3.1).
pub const KEY_ID_HEX_LEN: usize = 32;

/// Length of the base64url secret without padding (docs/06 §3.1).
pub const SECRET_LEN: usize = 43;

/// Credential prefix of application API keys (docs/06 §3.1).
pub const CREDENTIAL_PREFIX: &str = "gsk";

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
