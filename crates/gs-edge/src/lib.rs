#![forbid(unsafe_code)]

use hmac::digest::Update;
use hmac::{KeyInit, Mac};
use sha2::Sha256;

type HmacSha256 = hmac::SimpleHmac<Sha256>;

/// Verifica firma HMAC-SHA256 con tiempo constante (comparacion via Mac::verify_slice).
/// Devuelve false ante clave vacía o trama malformada — sin panico y sin filtrar.
pub fn verify_lpwam_signature(secret: &[u8], message: &[u8], signature: &[u8]) -> bool {
    if secret.is_empty() {
        return false;
    }
    let mut mac = match HmacSha256::new_from_slice(secret) {
        Ok(m) => m,
        Err(_) => return false,
    };
    Update::update(&mut mac, message);
    mac.verify_slice(signature).is_ok()
}

/// Ephemeral identifier rotation (E8 §2.3): derived from EIK + counter + window,
/// only the tag knows EIK; server stores only the ephemeral id.
pub fn ephemeral_id(eik: &[u8], counter: u64, window: u64) -> u64 {
    use sha2::Digest;
    let mut h = Sha256::new();
    Digest::update(&mut h, eik);
    Digest::update(&mut h, counter.to_le_bytes());
    Digest::update(&mut h, window.to_le_bytes());
    let digest = h.finalize();
    u64::from_le_bytes(digest[0..8].try_into().unwrap_or([0u8; 8]))
}

pub fn is_separated(last_seen_ms: u64, now_ms: u64, timeout_ms: u64) -> bool {
    now_ms.saturating_sub(last_seen_ms) > timeout_ms
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hmac_roundtrip() {
        let secret = b"truly-secret";
        let msg = b"hello lpwan";
        let mut mac = HmacSha256::new_from_slice(secret).unwrap();
        Update::update(&mut mac, msg);
        let sig = mac.finalize().into_bytes().to_vec();
        assert!(verify_lpwam_signature(secret, msg, &sig));
        assert!(!verify_lpwam_signature(secret, b"tampered", &sig));
        assert!(!verify_lpwam_signature(b"", msg, &sig));
    }
    #[test]
    fn separated_detection() {
        assert!(!is_separated(1000, 1500, 1000));
        assert!(is_separated(1000, 2500, 1000));
    }
}
