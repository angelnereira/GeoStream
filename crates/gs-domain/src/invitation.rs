//! Invitation code generation, Crockford Base32 normalization, and entity rules (docs/05 §6).
#![forbid(unsafe_code)]

use rand::Rng;
use sha2::{Digest, Sha256};
use std::fmt;
use time::OffsetDateTime;

use crate::ids::{InvitationId, SessionId, TenantId};
use crate::membership::Capabilities;
use crate::session::Actor;

/// Crockford Base32 alphabet symbols (10 digits + 22 letters, excluding I, L, O, U).
pub const CROCKFORD_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Invitation code normalization and validation errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvitationCodeError {
    #[error("invitation code length must be between 1 and 32 characters before normalization")]
    InvalidInputLength,
    #[error("normalized invitation code must be exactly 16 characters (got {0})")]
    InvalidNormalizedLength(usize),
    #[error("forbidden character '{0}' in invitation code (U is excluded from Crockford Base32)")]
    ForbiddenCharacter(char),
    #[error("invalid character '{0}' not in Crockford Base32 alphabet or allowed separators")]
    InvalidCharacter(char),
}

/// Invitation domain errors for redemption and revocation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvitationError {
    #[error("invitation is already revoked")]
    AlreadyRevoked,
    #[error("invitation has expired")]
    Expired,
    #[error("invitation has reached maximum uses")]
    MaxUsesReached,
    #[error("invalid grant: grants must be a subset of issuer capabilities")]
    InvalidGrants,
    #[error("invalid code format: {0}")]
    CodeError(#[from] InvitationCodeError),
}

/// Normalizes an invitation code according to docs/05 §6:
/// - Input length check: 1..=32 characters before normalization.
/// - Uppercase conversion.
/// - Remove hyphens `-` and spaces `' '`.
/// - Reject any other separator or non-alphanumeric character.
/// - Map `I` and `L` to `1`.
/// - Map `O` to `0`.
/// - Reject `U` and any character not in the Crockford Base32 alphabet.
/// - Verify normalized output length is exactly 16 characters.
pub fn normalize_invitation_code(input: &str) -> Result<String, InvitationCodeError> {
    if input.is_empty() || input.len() > 32 {
        return Err(InvitationCodeError::InvalidInputLength);
    }

    let mut normalized = String::with_capacity(16);

    for c in input.chars() {
        match c {
            '-' | ' ' => {
                // Allowed separators, stripped
            }
            '0'..='9' => {
                normalized.push(c);
            }
            'a'..='z' | 'A'..='Z' => {
                let upper = c.to_ascii_uppercase();
                match upper {
                    'I' | 'L' => normalized.push('1'),
                    'O' => normalized.push('0'),
                    'U' => return Err(InvitationCodeError::ForbiddenCharacter('U')),
                    'A'..='H' | 'J' | 'K' | 'M' | 'N' | 'P'..='T' | 'V'..='Z' => {
                        normalized.push(upper)
                    }
                    _ => return Err(InvitationCodeError::InvalidCharacter(c)),
                }
            }
            _ => return Err(InvitationCodeError::InvalidCharacter(c)),
        }
    }

    if normalized.len() != 16 {
        return Err(InvitationCodeError::InvalidNormalizedLength(
            normalized.len(),
        ));
    }

    Ok(normalized)
}

/// Generates a new 16-character Crockford Base32 invitation code formatted as `XXXX-XXXX-XXXX-XXXX`.
pub fn generate_invitation_code() -> String {
    let mut rng = rand::rng();
    let mut random_bytes = [0u8; 16];
    rng.fill_bytes(&mut random_bytes);

    let mut chars = [0u8; 16];
    for (i, b) in random_bytes.iter().enumerate() {
        chars[i] = CROCKFORD_ALPHABET[(b % 32) as usize];
    }

    let raw = std::str::from_utf8(&chars).expect("valid ASCII bytes");
    format!(
        "{}-{}-{}-{}",
        &raw[0..4],
        &raw[4..8],
        &raw[8..12],
        &raw[12..16]
    )
}

/// Computes SHA-256 hash of a normalized invitation code (lowercase hex string).
pub fn hash_invitation_code(normalized_code: &str) -> String {
    use std::fmt::Write;
    let mut hasher = Sha256::new();
    hasher.update(normalized_code.as_bytes());
    let result = hasher.finalize();

    let mut hex = String::with_capacity(64);
    for b in result {
        let _ = write!(hex, "{b:02x}");
    }
    hex
}

/// Normalizes and hashes an invitation code in one step.
pub fn hash_raw_invitation_code(raw_code: &str) -> Result<String, InvitationCodeError> {
    let normalized = normalize_invitation_code(raw_code)?;
    Ok(hash_invitation_code(&normalized))
}

/// Computes SHA-256 hash of a normalized invitation code returning raw 32 bytes (docs/05 §6).
pub fn hash_invitation_code_bytes(normalized_code: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(normalized_code.as_bytes());
    let result = hasher.finalize();
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&result);
    bytes
}

/// Redacted container for invitation code secrets (docs/10 §8.2).
#[derive(Clone, PartialEq, Eq)]
pub struct InvitationCodeSecret(String);

impl InvitationCodeSecret {
    pub fn new(code: String) -> Self {
        Self(code)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for InvitationCodeSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InvitationCodeSecret(<redacted>)")
    }
}

impl fmt::Display for InvitationCodeSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Invitation entity (docs/05 §6).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Invitation {
    pub id: InvitationId,
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub code_hash: String,
    pub grants: Capabilities,
    pub max_uses: u32,
    pub uses: u32,
    pub expires_at: OffsetDateTime,
    pub created_by: Actor,
    pub created_at: OffsetDateTime,
    pub revoked_at: Option<OffsetDateTime>,
}

impl Invitation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: InvitationId,
        session_id: SessionId,
        tenant_id: TenantId,
        code_hash: String,
        grants: Capabilities,
        max_uses: u32,
        expires_at: OffsetDateTime,
        created_by: Actor,
        created_at: OffsetDateTime,
        session_expires_at: OffsetDateTime,
        invitation_max_uses: u32,
        invitation_max_ttl_seconds: u32,
    ) -> Result<Self, InvitationError> {
        if max_uses < 1 || max_uses > invitation_max_uses {
            return Err(InvitationError::MaxUsesReached);
        }

        let max_ttl = created_at + time::Duration::seconds(invitation_max_ttl_seconds as i64);
        if expires_at <= created_at || expires_at > max_ttl || expires_at > session_expires_at {
            return Err(InvitationError::Expired);
        }

        Ok(Self {
            id,
            session_id,
            tenant_id,
            code_hash,
            grants,
            max_uses,
            uses: 0,
            expires_at,
            created_by,
            created_at,
            revoked_at: None,
        })
    }

    /// Evaluates whether the invitation is currently redeemable (docs/05 §6).
    pub fn is_redeemable_at(&self, now: OffsetDateTime) -> bool {
        self.revoked_at.is_none() && self.uses < self.max_uses && self.expires_at > now
    }

    /// Records a redemption use (docs/05 §6).
    pub fn redeem(&mut self, now: OffsetDateTime) -> Result<(), InvitationError> {
        if self.revoked_at.is_some() {
            return Err(InvitationError::AlreadyRevoked);
        }
        if self.expires_at <= now {
            return Err(InvitationError::Expired);
        }
        if self.uses >= self.max_uses {
            return Err(InvitationError::MaxUsesReached);
        }

        self.uses += 1;
        Ok(())
    }

    /// Revokes the invitation.
    pub fn revoke(&mut self, now: OffsetDateTime) -> Result<(), InvitationError> {
        if self.revoked_at.is_some() {
            return Err(InvitationError::AlreadyRevoked);
        }
        self.revoked_at = Some(now);
        Ok(())
    }

    /// Computes the calculated status following the precedence of docs/07 §9.4:
    /// REVOKED > EXPIRED > EXHAUSTED > ACTIVE.
    pub fn status_at(&self, now: OffsetDateTime) -> InvitationStatus {
        if self.revoked_at.is_some() {
            InvitationStatus::Revoked
        } else if self.expires_at <= now {
            InvitationStatus::Expired
        } else if self.uses >= self.max_uses {
            InvitationStatus::Exhausted
        } else {
            InvitationStatus::Active
        }
    }
}

/// Invitation status calculated by precedence: REVOKED > EXPIRED > EXHAUSTED > ACTIVE (docs/07 §9.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InvitationStatus {
    Active,
    Exhausted,
    Expired,
    Revoked,
}
