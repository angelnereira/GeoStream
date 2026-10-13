//! AES-256-GCM encryption at rest for idempotent responses that contain secrets
//! (docs/10 §5.6, docs/07 §6).
//!
//! Envelope format: `v1:<key_id>:<nonce>:<ciphertext>` with all values in base64url except the
//! version and the key id. The nonce is 96 random bits from the operating system CSPRNG and is
//! never reused. Decryption accepts a key id so that old keys can still decrypt during rotation
//! (`GS_DATA_ENCRYPTION_KEYS_OLD`, docs/17 §2.4).
#![forbid(unsafe_code)]

use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{Aead, KeyInit};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64URL;
use rand::Rng;
use zeroize::Zeroizing;

/// Version prefix of the envelope (docs/10 §5.6).
pub const ENVELOPE_VERSION: &str = "v1";

/// Length of the AES-256-GCM nonce in bytes (96 bits).
pub const NONCE_LEN: usize = 12;

/// Errors produced while encrypting or decrypting at rest.
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    /// The key does not have the expected length.
    #[error("invalid encryption key length: {0}")]
    InvalidKey(String),

    /// The envelope is malformed.
    #[error("malformed encrypted envelope: {0}")]
    MalformedEnvelope(String),

    /// No key with the envelope's id is available.
    #[error("unknown encryption key id '{0}'")]
    UnknownKey(String),

    /// Authentication of the ciphertext failed.
    #[error("ciphertext authentication failed")]
    AuthenticationFailed,
}

/// AES-256-GCM cipher bound to one key.
pub struct DataCipher {
    key_id: String,
    cipher: Aes256Gcm,
}

impl DataCipher {
    /// Builds a cipher from raw 32 bytes and the key id that identifies them.
    pub fn new(key_id: impl Into<String>, key_bytes: &[u8; 32]) -> Result<Self, CryptoError> {
        let cipher = Aes256Gcm::new_from_slice(key_bytes)
            .map_err(|e| CryptoError::InvalidKey(e.to_string()))?;
        Ok(Self {
            key_id: key_id.into(),
            cipher,
        })
    }

    /// Key id of this cipher (first 8 hex of SHA-256 of the key, docs/17 §2.4).
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    /// Encrypts a plaintext into the `v1:<key_id>:<nonce>:<ciphertext>` envelope.
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<String, CryptoError> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = aes_gcm::aead::Nonce::<Aes256Gcm>::from(nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext)
            .map_err(|_| CryptoError::AuthenticationFailed)?;

        Ok(format!(
            "{ENVELOPE_VERSION}:{}:{}:{}",
            self.key_id,
            BASE64URL.encode(nonce_bytes),
            BASE64URL.encode(ciphertext)
        ))
    }

    /// Decrypts an envelope produced by [`DataCipher::encrypt`] with this key.
    pub fn decrypt(&self, envelope: &str) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
        let (_, nonce, ciphertext) = parse_envelope(envelope)?;
        self.decrypt_parts(nonce.as_slice(), ciphertext.as_slice())
    }

    /// Decrypts the nonce and ciphertext parts of an envelope with this key.
    pub fn decrypt_parts(
        &self,
        nonce: &[u8],
        ciphertext: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
        let nonce =
            aes_gcm::aead::Nonce::<Aes256Gcm>::from(<[u8; NONCE_LEN]>::try_from(nonce).map_err(
                |_| CryptoError::MalformedEnvelope("nonce must be 12 bytes".to_string()),
            )?);
        let plaintext = self
            .cipher
            .decrypt(&nonce, ciphertext)
            .map_err(|_| CryptoError::AuthenticationFailed)?;
        Ok(Zeroizing::new(plaintext))
    }
}

impl std::fmt::Debug for DataCipher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataCipher")
            .field("key_id", &self.key_id)
            .finish()
    }
}

/// Splits an envelope into its version, key id, nonce and ciphertext.
pub fn parse_envelope(envelope: &str) -> Result<(String, Vec<u8>, Vec<u8>), CryptoError> {
    let mut parts = envelope.split(':');
    let version = parts.next().unwrap_or_default();
    let key_id = parts.next().unwrap_or_default();
    let nonce = parts.next().unwrap_or_default();
    let ciphertext = parts.next().unwrap_or_default();
    if parts.next().is_some() {
        return Err(CryptoError::MalformedEnvelope(
            "too many segments".to_string(),
        ));
    }
    if version != ENVELOPE_VERSION {
        return Err(CryptoError::MalformedEnvelope(format!(
            "unsupported version '{version}'"
        )));
    }
    if key_id.is_empty() {
        return Err(CryptoError::MalformedEnvelope("missing key id".to_string()));
    }
    let nonce = BASE64URL
        .decode(nonce)
        .map_err(|e| CryptoError::MalformedEnvelope(format!("nonce: {e}")))?;
    let ciphertext = BASE64URL
        .decode(ciphertext)
        .map_err(|e| CryptoError::MalformedEnvelope(format!("ciphertext: {e}")))?;
    Ok((key_id.to_string(), nonce, ciphertext))
}

/// Builds a cipher from a `gs-config` key (docs/17 §2.4).
pub fn cipher_from_config_key(
    key: &gs_config::DataEncryptionKey,
) -> Result<DataCipher, CryptoError> {
    DataCipher::new(key.key_id(), key.raw_bytes())
}

/// Encrypts with the active key and decrypts with the active key or any of the old ones.
#[derive(Default)]
pub struct CryptoRotator {
    active: Option<DataCipher>,
    old: Vec<DataCipher>,
}

impl CryptoRotator {
    /// Builds a rotator with no keys: encryption fails, decryption fails.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Sets the active key.
    pub fn with_active(mut self, cipher: DataCipher) -> Self {
        self.active = Some(cipher);
        self
    }

    /// Adds an old key that can still decrypt.
    pub fn with_old(mut self, cipher: DataCipher) -> Self {
        self.old.push(cipher);
        self
    }

    /// Whether an active key is configured.
    pub fn can_encrypt(&self) -> bool {
        self.active.is_some()
    }

    /// Encrypts with the active key.
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<String, CryptoError> {
        let active = self
            .active
            .as_ref()
            .ok_or(CryptoError::UnknownKey("no active key".to_string()))?;
        active.encrypt(plaintext)
    }

    /// Decrypts with the key the envelope names (docs/17 §2.4).
    pub fn decrypt(&self, envelope: &str) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
        let (key_id, nonce, ciphertext) = parse_envelope(envelope)?;
        if let Some(active) = &self.active
            && active.key_id() == key_id
        {
            return active.decrypt_parts(&nonce, &ciphertext);
        }
        for cipher in &self.old {
            if cipher.key_id() == key_id {
                return cipher.decrypt_parts(&nonce, &ciphertext);
            }
        }
        Err(CryptoError::UnknownKey(key_id))
    }
}

impl std::fmt::Debug for CryptoRotator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CryptoRotator")
            .field("active_key_id", &self.active.as_ref().map(|c| c.key_id()))
            .field("old_keys", &self.old.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(byte: u8) -> [u8; 32] {
        [byte; 32]
    }

    #[test]
    fn test_roundtrip_encrypt_and_decrypt() {
        // covers: FR-API-002, docs/10 §5.6
        let cipher = DataCipher::new("0123456789abcdef", &key(7)).expect("cipher");
        let envelope = cipher
            .encrypt(b"response body with a secret")
            .expect("encrypt");

        assert!(envelope.starts_with("v1:0123456789abcdef:"));
        assert!(!envelope.contains("response body"));

        let plaintext = cipher.decrypt(&envelope).expect("decrypt");
        assert_eq!(&*plaintext, b"response body with a secret");
    }

    #[test]
    fn test_nonce_is_random_and_twelve_bytes() {
        // covers: docs/10 §5.6
        let cipher = DataCipher::new("k", &key(1)).expect("cipher");
        let first = cipher.encrypt(b"payload").expect("first");
        let second = cipher.encrypt(b"payload").expect("second");
        assert_ne!(first, second, "the nonce must never repeat");

        let (nonce, _) = split_nonce(&first);
        assert_eq!(nonce.len(), NONCE_LEN);
    }

    #[test]
    fn test_tampered_ciphertext_is_rejected() {
        // covers: docs/10 §5.6
        let cipher = DataCipher::new("k", &key(2)).expect("cipher");
        let envelope = cipher.encrypt(b"payload").expect("encrypt");
        let mut tampered = envelope.clone();
        tampered.push('x');
        assert!(cipher.decrypt(&format!("{tampered}z")).is_err());

        let parts: Vec<&str> = envelope.split(':').collect();
        let mut body = BASE64URL.decode(parts[3]).expect("ciphertext");
        body[0] ^= 0xff;
        let tampered = format!(
            "{}:{}:{}:{}",
            parts[0],
            parts[1],
            parts[2],
            BASE64URL.encode(body)
        );
        assert!(matches!(
            cipher.decrypt(&tampered),
            Err(CryptoError::AuthenticationFailed)
        ));
    }

    #[test]
    fn test_malformed_envelopes_are_rejected() {
        // covers: docs/10 §5.6
        let cipher = DataCipher::new("k", &key(3)).expect("cipher");
        for malformed in [
            "",
            "v2:k:nonce:ciphertext",
            "v1::nonce:ciphertext",
            "v1:k:nonce",
            "v1:k:nonce:ciphertext:extra",
            "v1:k:not-base64:ciphertext",
        ] {
            assert!(
                cipher.decrypt(malformed).is_err(),
                "malformed envelope must be rejected: {malformed}"
            );
        }
    }

    #[test]
    fn test_rotation_decrypts_with_the_old_key() {
        // covers: docs/17 §2.4, docs/10 §5.6
        let active = DataCipher::new("newkey", &key(9)).expect("active");
        let old = DataCipher::new("oldkey", &key(4)).expect("old");

        let rotator = CryptoRotator::empty()
            .with_active(active)
            .with_old(DataCipher::new("oldkey", &key(4)).expect("old again"));

        // An envelope written with the old key still decrypts.
        let envelope = old
            .encrypt(b"legacy payload")
            .expect("encrypt with old key");
        let plaintext = rotator.decrypt(&envelope).expect("decrypt with rotator");
        assert_eq!(&*plaintext, b"legacy payload");

        // New writes use the active key.
        let fresh = rotator
            .encrypt(b"fresh payload")
            .expect("encrypt with rotator");
        assert!(fresh.starts_with("v1:newkey:"));
        assert_eq!(
            &*rotator.decrypt(&fresh).expect("decrypt"),
            b"fresh payload"
        );

        // A rotator without the old key rejects the envelope.
        let limited =
            CryptoRotator::empty().with_active(DataCipher::new("newkey", &key(9)).expect("c"));
        assert!(matches!(
            limited.decrypt(&envelope),
            Err(CryptoError::UnknownKey(_))
        ));
    }

    #[test]
    fn test_cipher_from_config_key_uses_the_documented_key_id() {
        // covers: docs/17 §2.4
        let key_bytes = [5u8; 32];
        let b64 = base64::engine::general_purpose::STANDARD.encode(key_bytes);
        let config_key = gs_config::DataEncryptionKey::from_base64(&b64, "GS_DATA_ENCRYPTION_KEY")
            .expect("config key");
        let cipher = cipher_from_config_key(&config_key).expect("cipher");
        assert_eq!(cipher.key_id(), config_key.key_id());
    }

    fn split_nonce(envelope: &str) -> (Vec<u8>, Vec<u8>) {
        let parts: Vec<&str> = envelope.split(':').collect();
        (
            BASE64URL.decode(parts[2]).expect("nonce"),
            BASE64URL.decode(parts[3]).expect("ciphertext"),
        )
    }
}
