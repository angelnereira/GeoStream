//! Console passwords: PBKDF2-HMAC-SHA256 (RFC 8018) and the password policy (ADR-0037).
//!
//! No password-hashing crate is on the approved list (`docs/19` §6), so the function is built on the
//! approved `hmac` and `sha2` and checked against independent vectors (RFC 7914 §11 and Python's
//! `hashlib.pbkdf2_hmac`). 600 000 iterations is the OWASP figure for PBKDF2-HMAC-SHA256.
//!
//! A stored password is `pbkdf2-sha256$<iterations>$<salt b64url>$<hash b64url>`: the iteration count
//! travels with the hash, so raising the default never locks anybody out (`needs_rehash`).
//!
//! The password itself is never logged, never returned and is wiped from memory after use. Hashing
//! is expensive by design: callers run it outside the async executor (`spawn_blocking`).
#![forbid(unsafe_code)]

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use rand::Rng;
use sha2::Sha256;
use subtle::ConstantTimeEq;
use thiserror::Error;

type HmacSha256 = Hmac<Sha256>;

/// Default iterations (OWASP, PBKDF2-HMAC-SHA256).
pub const DEFAULT_ITERATIONS: u32 = 600_000;
/// Lowest count accepted from configuration.
pub const MIN_ITERATIONS: u32 = 100_000;
/// Shortest password (the policy asks for length, not for character classes).
pub const MIN_LENGTH: usize = 10;
/// Longest password: bounds the work an attacker can force with one request.
pub const MAX_LENGTH: usize = 128;

const SALT_LEN: usize = 16;
const HASH_LEN: usize = 32;
const SCHEME: &str = "pbkdf2-sha256";

/// Why a password was refused. Never carries the password.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PasswordPolicyError {
    /// Shorter than [`MIN_LENGTH`] or longer than [`MAX_LENGTH`] characters.
    #[error("the password must have between {MIN_LENGTH} and {MAX_LENGTH} characters")]
    Length,
    /// One character repeated, or the e-mail itself, or a very common password.
    #[error("the password is too easy to guess")]
    TooWeak,
}

/// Passwords that pass the length rule and are still among the first an attacker tries.
const COMMON: &[&str] = &[
    "password",
    "password1",
    "password12",
    "password123",
    "passw0rd123",
    "1234567890",
    "12345678910",
    "123456789012",
    "qwertyuiop",
    "qwerty12345",
    "qwertyuiop123",
    "iloveyou123",
    "letmein1234",
    "welcome1234",
    "admin12345",
    "administrator",
    "contrasena",
    "contrasena123",
    "contraseña123",
    "geostream123",
    "gravital123",
];

/// Checks the policy: length 10 to 128, not a single repeated character, not the e-mail address (or
/// its local part) and not one of a short list of the most common passwords. Length matters more than
/// composition rules (NIST SP 800-63B), so there are no required character classes.
pub fn validate_policy(password: &str, email: &str) -> Result<(), PasswordPolicyError> {
    let length = password.chars().count();
    if !(MIN_LENGTH..=MAX_LENGTH).contains(&length) {
        return Err(PasswordPolicyError::Length);
    }
    let lowered = password.to_lowercase();
    let first = password.chars().next();
    let local = email.split('@').next().unwrap_or("").to_lowercase();
    let weak = password.chars().all(|c| Some(c) == first)
        || lowered == email.to_lowercase()
        || (!local.is_empty() && lowered == local)
        || COMMON.contains(&lowered.as_str());
    if weak {
        return Err(PasswordPolicyError::TooWeak);
    }
    Ok(())
}

/// PBKDF2-HMAC-SHA256 with a 32-byte output (one block: `T1`).
fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32) -> [u8; HASH_LEN] {
    // `HmacSha256` accepts keys of any length, so `new_from_slice` cannot fail; the fallback keeps the
    // function total without a panic path.
    let Ok(base) = HmacSha256::new_from_slice(password) else {
        return [0u8; HASH_LEN];
    };
    let mut mac = base.clone();
    mac.update(salt);
    mac.update(&1u32.to_be_bytes());
    let mut u: [u8; HASH_LEN] = mac.finalize().into_bytes().into();
    let mut t = u;
    for _ in 1..iterations {
        let mut mac = base.clone();
        mac.update(&u);
        u = mac.finalize().into_bytes().into();
        for (acc, byte) in t.iter_mut().zip(u.iter()) {
            *acc ^= byte;
        }
    }
    t
}

/// Hashes and verifies passwords with a fixed iteration count for new hashes.
#[derive(Debug, Clone, Copy)]
pub struct PasswordHasher {
    iterations: u32,
}

impl PasswordHasher {
    /// A hasher that produces hashes of `iterations` rounds. Verification honours the count stored in
    /// each hash, whatever this one is.
    pub const fn new(iterations: u32) -> Self {
        Self { iterations }
    }

    /// The configured iteration count.
    pub const fn iterations(&self) -> u32 {
        self.iterations
    }

    /// Hashes a password with a fresh random salt.
    pub fn hash(&self, password: &str) -> String {
        let mut salt = [0u8; SALT_LEN];
        rand::rng().fill_bytes(&mut salt);
        let digest = pbkdf2_sha256(password.as_bytes(), &salt, self.iterations);
        format!(
            "{SCHEME}${}${}${}",
            self.iterations,
            URL_SAFE_NO_PAD.encode(salt),
            URL_SAFE_NO_PAD.encode(digest)
        )
    }

    /// Whether `password` matches `stored`. A malformed `stored` is a mismatch, never an error that
    /// distinguishes it. The comparison is constant time.
    pub fn verify(&self, stored: &str, password: &str) -> bool {
        let Some((iterations, salt, expected)) = parse(stored) else {
            return false;
        };
        let digest = pbkdf2_sha256(password.as_bytes(), &salt, iterations);
        digest.ct_eq(&expected).into()
    }

    /// Spends the work of a verification without a real hash, so that answering for an account that
    /// does not exist takes as long as answering for one that does (no enumeration by timing).
    pub fn verify_dummy(&self, password: &str) {
        let salt = [0x5au8; SALT_LEN];
        let _ = pbkdf2_sha256(password.as_bytes(), &salt, self.iterations);
    }

    /// Whether `stored` was made with fewer rounds than this hasher uses now.
    pub fn needs_rehash(&self, stored: &str) -> bool {
        parse(stored).is_none_or(|(iterations, _, _)| iterations < self.iterations)
    }
}

fn parse(stored: &str) -> Option<(u32, Vec<u8>, [u8; HASH_LEN])> {
    let mut parts = stored.split('$');
    if parts.next()? != SCHEME {
        return None;
    }
    let iterations: u32 = parts.next()?.parse().ok()?;
    if iterations == 0 {
        return None;
    }
    let salt = URL_SAFE_NO_PAD.decode(parts.next()?).ok()?;
    let hash: [u8; HASH_LEN] = URL_SAFE_NO_PAD
        .decode(parts.next()?)
        .ok()?
        .try_into()
        .ok()?;
    if parts.next().is_some() || salt.len() < 8 {
        return None;
    }
    Some((iterations, salt, hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn test_pbkdf2_matches_the_rfc_7914_vectors() {
        // RFC 7914 §11 (first 32 bytes of the 64-byte vectors).
        assert_eq!(
            hex(&pbkdf2_sha256(b"passwd", b"salt", 1)),
            "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc"
        );
        assert_eq!(
            hex(&pbkdf2_sha256(b"Password", b"NaCl", 80_000)),
            "4ddcd8f60b98be21830cee5ef22701f9641a4418d04c0414aeff08876b34ab56"
        );
    }

    #[test]
    fn test_pbkdf2_matches_an_independent_implementation() {
        // Generated with Python's hashlib.pbkdf2_hmac('sha256', …, 32).
        let salt: Vec<u8> = (0u8..16).collect();
        assert_eq!(
            hex(&pbkdf2_sha256(b"correct horse battery staple", &salt, 1000)),
            "a69b179e3add3c1e0aaf227a0eb3aa2aa8645ab86fecf6ca00c17512697c719e"
        );
    }

    #[test]
    fn test_a_hash_verifies_only_its_own_password() {
        let hasher = PasswordHasher::new(1000);
        let stored = hasher.hash("a long enough secret");
        assert!(hasher.verify(&stored, "a long enough secret"));
        assert!(!hasher.verify(&stored, "a long enough secreT"));
        assert!(!hasher.verify(&stored, ""));
    }

    #[test]
    fn test_two_hashes_of_the_same_password_differ_by_salt() {
        let hasher = PasswordHasher::new(1000);
        let a = hasher.hash("the same password");
        let b = hasher.hash("the same password");
        assert_ne!(a, b);
        assert!(hasher.verify(&a, "the same password") && hasher.verify(&b, "the same password"));
    }

    #[test]
    fn test_the_stored_form_never_contains_the_password() {
        let stored = PasswordHasher::new(1000).hash("super-secret-value");
        assert!(!stored.contains("super-secret-value"));
        assert!(stored.starts_with("pbkdf2-sha256$1000$"));
    }

    #[test]
    fn test_verification_uses_the_count_stored_in_the_hash() {
        let old = PasswordHasher::new(1000).hash("rotating password");
        let newer = PasswordHasher::new(2000);
        assert!(newer.verify(&old, "rotating password"));
        assert!(newer.needs_rehash(&old));
        assert!(!newer.needs_rehash(&newer.hash("x")));
    }

    #[test]
    fn test_malformed_hashes_are_a_plain_mismatch() {
        let hasher = PasswordHasher::new(1000);
        for stored in [
            "",
            "plaintext",
            "pbkdf2-sha256$0$AAAA$AAAA",
            "pbkdf2-sha256$abc$AAAAAAAAAAAAAAAA$AAAA",
            "md5$1000$AAAAAAAAAAAAAAAA$AAAA",
            "pbkdf2-sha256$1000$AAAAAAAAAAAAAAAA",
        ] {
            assert!(!hasher.verify(stored, "anything at all"), "{stored}");
        }
    }

    #[test]
    fn test_the_policy_asks_for_length_and_refuses_the_obvious() {
        let email = "dev@example.test";
        assert_eq!(
            validate_policy("short", email),
            Err(PasswordPolicyError::Length)
        );
        assert_eq!(
            validate_policy(&"a".repeat(MAX_LENGTH + 1), email),
            Err(PasswordPolicyError::Length)
        );
        assert_eq!(
            validate_policy("aaaaaaaaaaaa", email),
            Err(PasswordPolicyError::TooWeak)
        );
        assert_eq!(
            validate_policy("Password123", email),
            Err(PasswordPolicyError::TooWeak)
        );
        assert_eq!(
            validate_policy("DEV@example.test", email),
            Err(PasswordPolicyError::TooWeak)
        );
        assert!(validate_policy("a sentence works fine", email).is_ok());
        assert!(validate_policy(&"é".repeat(MIN_LENGTH), "x@y.test").is_err());
        assert!(validate_policy("contraseña-segura-2026", email).is_ok());
    }
}
