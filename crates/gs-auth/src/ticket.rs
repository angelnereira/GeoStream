//! Stream ticket format, generation and hashing (docs/06 §5, docs/08 §3, FR-AUTH-006).
//!
//! A ticket is the single-use credential that authenticates a WebSocket upgrade:
//! `gst_` followed by 32 CSPRNG bytes encoded in base64url without padding. Only the
//! SHA-256 digest of a ticket is used as a storage key, so the ephemeral store never
//! holds a value that could be replayed (docs/06 §5, docs/11 §3).
#![forbid(unsafe_code)]

use base64::Engine;
use rand::Rng;
use sha2::{Digest, Sha256};
use thiserror::Error;

use gs_domain::error::{AsErrorCode, ErrorCode};

/// Prefix of a stream ticket (docs/06 §5).
pub const TICKET_PREFIX: &str = "gst";

/// Entropy of a ticket in bytes (docs/06 §5).
pub const TICKET_SECRET_BYTES: usize = 32;

/// Length of the base64url payload of a ticket, without padding (docs/06 §5).
pub const TICKET_SECRET_LEN: usize = 43;

/// Total length of a ticket: `gst_` plus [`TICKET_SECRET_LEN`] characters (docs/06 §5).
pub const TICKET_LEN: usize = TICKET_PREFIX.len() + 1 + TICKET_SECRET_LEN;

/// Errors produced while validating a stream ticket (docs/06 §5).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TicketError {
    /// The ticket does not match `^gst_[A-Za-z0-9_-]{43}$`.
    #[error("invalid stream ticket format")]
    InvalidFormat,
}

impl AsErrorCode for TicketError {
    fn error_code(&self) -> ErrorCode {
        // A malformed ticket is indistinguishable from an unknown one (docs/08 §3).
        ErrorCode::Unauthenticated
    }
}

/// Generates a new single-use ticket (docs/06 §5).
///
/// The result is exactly [`TICKET_LEN`] characters and matches
/// `^gst_[A-Za-z0-9_-]{43}$`. The value is a secret: never log it.
pub fn generate_ticket() -> String {
    let mut bytes = [0u8; TICKET_SECRET_BYTES];
    rand::rng().fill_bytes(&mut bytes);
    format!(
        "{TICKET_PREFIX}_{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    )
}

/// Validates the shape of a ticket without touching any store (docs/06 §5).
pub fn validate_ticket(ticket: &str) -> Result<(), TicketError> {
    let Some(payload) = ticket.strip_prefix(TICKET_PREFIX) else {
        return Err(TicketError::InvalidFormat);
    };
    let Some(payload) = payload.strip_prefix('_') else {
        return Err(TicketError::InvalidFormat);
    };
    if payload.len() != TICKET_SECRET_LEN {
        return Err(TicketError::InvalidFormat);
    }
    if !payload
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(TicketError::InvalidFormat);
    }
    Ok(())
}

/// SHA-256 digest of a ticket, used as the storage key suffix (docs/06 §5, docs/11 §3).
pub fn hash_ticket(ticket: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(ticket.as_bytes());
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// Lowercase hexadecimal of [`hash_ticket`]: the `{sha256}` of `{p}:ticket:{sha256}` (docs/11 §3).
pub fn hash_ticket_hex(ticket: &str) -> String {
    let digest = hash_ticket(ticket);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generated_tickets_match_the_documented_shape() {
        // covers: FR-AUTH-006, docs/06 §5
        for _ in 0..256 {
            let ticket = generate_ticket();
            assert_eq!(ticket.len(), TICKET_LEN);
            assert!(validate_ticket(&ticket).is_ok(), "ticket: {ticket}");
        }
    }

    #[test]
    fn test_tickets_are_unique() {
        // covers: FR-AUTH-006
        let first = generate_ticket();
        let second = generate_ticket();
        assert_ne!(first, second, "tickets must not repeat");
    }

    #[test]
    fn test_malformed_tickets_are_rejected() {
        // covers: FR-AUTH-006, docs/06 §5
        let cases = [
            "",
            "gst_",
            "gs_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "gst_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "gst_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "gst_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa!",
            "gst aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "gstaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ];
        for case in cases {
            assert_eq!(
                validate_ticket(case),
                Err(TicketError::InvalidFormat),
                "case: {case}"
            );
        }
    }

    #[test]
    fn test_invalid_ticket_maps_to_unauthorized() {
        // covers: docs/08 §3, docs/09 §6
        assert_eq!(
            TicketError::InvalidFormat.error_code(),
            ErrorCode::Unauthenticated
        );
    }

    #[test]
    fn test_hash_is_stable_and_hex_encoded() {
        // covers: docs/11 §3
        let ticket = "gst_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert_eq!(hash_ticket_hex(ticket), hash_ticket_hex(ticket));
        assert_eq!(hash_ticket_hex(ticket).len(), 64);
        assert!(
            hash_ticket_hex(ticket)
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        );
        assert_ne!(hash_ticket_hex(ticket), hash_ticket_hex(&generate_ticket()));
    }
}
