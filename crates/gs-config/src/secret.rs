//! Secret string types and encryption key models with strict redaction (docs/10 §8, docs/17 §2.4).
#![forbid(unsafe_code)]

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use sha2::{Digest, Sha256};
use std::fmt;
use zeroize::Zeroize;

use crate::error::ConfigError;

/// Redacted string wrapper for secret values (passwords, URLs with credentials, keys).
#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(secret: impl Into<String>) -> Self {
        Self(secret.into())
    }

    /// Exposes the underlying secret string for verified usage.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    /// Returns true if the secret string is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Zeroize for SecretString {
    fn zeroize(&mut self) {
        self.0.zeroize();
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(<redacted>)")
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl From<String> for SecretString {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<&str> for SecretString {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

/// AES-256-GCM data encryption key with derived 8-hex key ID (docs/17 §2.4).
#[derive(Clone)]
pub struct DataEncryptionKey {
    raw_bytes: [u8; 32],
    key_id: String,
}

impl Zeroize for DataEncryptionKey {
    fn zeroize(&mut self) {
        self.raw_bytes.zeroize();
    }
}

impl Drop for DataEncryptionKey {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl DataEncryptionKey {
    /// Parses a base64 encoded string into a 32-byte AES key and computes its key_id.
    pub fn from_base64(b64: &str, key_name: &str) -> Result<Self, ConfigError> {
        let bytes = BASE64_STANDARD
            .decode(b64.trim())
            .map_err(|e| ConfigError::InvalidValue {
                key: key_name.to_string(),
                message: format!("invalid base64 encoding: {e}"),
            })?;

        if bytes.len() != 32 {
            return Err(ConfigError::InvalidValue {
                key: key_name.to_string(),
                message: format!(
                    "expected exactly 32 bytes for AES-256 key, got {}",
                    bytes.len()
                ),
            });
        }

        let mut raw_bytes = [0u8; 32];
        raw_bytes.copy_from_slice(&bytes);

        // Compute SHA-256 and take first 8 lowercase hex characters as key_id
        let hash = Sha256::digest(raw_bytes);
        use std::fmt::Write;
        let mut hex_hash = String::with_capacity(64);
        for byte in &hash {
            let _ = write!(&mut hex_hash, "{byte:02x}");
        }
        let key_id = hex_hash[..8].to_string();

        Ok(Self { raw_bytes, key_id })
    }

    /// Key identifier (first 8 hex chars of SHA-256).
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    /// Raw 32 bytes for AES-256-GCM.
    pub fn raw_bytes(&self) -> &[u8; 32] {
        &self.raw_bytes
    }
}

impl fmt::Debug for DataEncryptionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DataEncryptionKey")
            .field("key_id", &self.key_id)
            .field("raw_bytes", &"<redacted>")
            .finish()
    }
}
