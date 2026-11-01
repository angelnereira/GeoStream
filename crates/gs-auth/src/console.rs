//! Primitives of the console accounts: e-mail normalization, one-time codes and session tokens
//! (ADR-0036, docs/06 §8.6).
//!
//! Everything here is pure and has no I/O. Secrets are handled the way `10` §5.6 asks: a code
//! is stored only as an HMAC bound to the e-mail, a session only as a SHA-256, and every
//! comparison is constant time. The e-mail is personal data (`10` §6.2): its `Debug` is redacted
//! and its only loggable form is the keyed hash.
#![forbid(unsafe_code)]

use std::fmt;

use base64::Engine;
use hmac::digest::Update;
use hmac::{KeyInit, Mac};
use rand::Rng;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use thiserror::Error;

type HmacSha256 = hmac::SimpleHmac<Sha256>;

/// Prefix of a console session token (`gcs_<43 base64url>`, OpenAPI `consoleSession`).
pub const SESSION_TOKEN_PREFIX: &str = "gcs";

/// Entropy of a session token in bytes.
pub const SESSION_TOKEN_BYTES: usize = 32;

/// Length of the base64url payload of a session token, without padding.
pub const SESSION_TOKEN_PAYLOAD_LEN: usize = 43;

/// Shortest and longest one-time code (`17` §2.12: `GS_CONSOLE_OTP_LENGTH`).
pub const OTP_MIN_LENGTH: usize = 6;
pub const OTP_MAX_LENGTH: usize = 10;

/// Longest e-mail address (RFC 5321 §4.5.3.1.3).
const EMAIL_MAX_LEN: usize = 254;
const EMAIL_LOCAL_MAX_LEN: usize = 64;

/// Errors of this module. None of them carries the offending value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ConsoleAuthError {
    /// The text is not an acceptable e-mail address.
    #[error("invalid e-mail address")]
    InvalidEmail,
    /// The HMAC key is shorter than 32 bytes.
    #[error("the HMAC key must be at least 32 bytes")]
    WeakKey,
}

/// A normalized e-mail address: trimmed, lowercase, ASCII, one `@`.
///
/// Aliases such as `+tag` are **not** folded: a provider may treat them as different mailboxes
/// and folding them would merge accounts that belong to different people.
#[derive(Clone, PartialEq, Eq)]
pub struct EmailAddress(String);

impl EmailAddress {
    /// Parses and normalizes an address.
    ///
    /// Deliberately stricter than RFC 5322: no quoted local parts, no comments, no IP-literal
    /// domains and no non-ASCII characters (an internationalized domain must be given in
    /// punycode). A stricter parser is safer here because the address ends up in a mail header.
    pub fn parse(raw: &str) -> Result<Self, ConsoleAuthError> {
        let candidate = raw.trim().to_ascii_lowercase();
        if candidate.is_empty() || candidate.len() > EMAIL_MAX_LEN || !candidate.is_ascii() {
            return Err(ConsoleAuthError::InvalidEmail);
        }
        let Some((local, domain)) = candidate.split_once('@') else {
            return Err(ConsoleAuthError::InvalidEmail);
        };
        if local.is_empty() || local.len() > EMAIL_LOCAL_MAX_LEN || domain.contains('@') {
            return Err(ConsoleAuthError::InvalidEmail);
        }
        let local_ok = local
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-/=?^_`{|}~.".contains(&b))
            && !local.starts_with('.')
            && !local.ends_with('.')
            && !local.contains("..");
        let labels: Vec<&str> = domain.split('.').collect();
        let domain_ok = labels.len() >= 2
            && labels.iter().all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
            && labels
                .last()
                .is_some_and(|tld| tld.len() >= 2 && tld.bytes().any(|b| b.is_ascii_alphabetic()));
        if local_ok && domain_ok {
            Ok(Self(candidate))
        } else {
            Err(ConsoleAuthError::InvalidEmail)
        }
    }

    /// The normalized address. Personal data: never log it (`10` §8.2).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for EmailAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EmailAddress(<redacted>)")
    }
}

/// The server-side key that binds e-mails and codes: keeps the stored digests useless without it.
#[derive(Clone)]
pub struct ConsoleKeys {
    // The HMAC state is built once, when the key is validated, and cloned for each use: no
    // fallible construction (and no panic path) remains in the hot path.
    base: HmacSha256,
}

impl fmt::Debug for ConsoleKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ConsoleKeys(<redacted>)")
    }
}

impl ConsoleKeys {
    /// Wraps an HMAC key of at least 32 bytes.
    pub fn new(key: &[u8]) -> Result<Self, ConsoleAuthError> {
        if key.len() < 32 {
            return Err(ConsoleAuthError::WeakKey);
        }
        let base = HmacSha256::new_from_slice(key).map_err(|_| ConsoleAuthError::WeakKey)?;
        Ok(Self { base })
    }

    fn mac(&self, domain: &[u8], parts: &[&[u8]]) -> [u8; 32] {
        let mut mac = self.base.clone();
        Update::update(&mut mac, domain);
        for part in parts {
            // Length-prefix each part so ("ab","c") and ("a","bc") never collide.
            Update::update(&mut mac, &(part.len() as u64).to_be_bytes());
            Update::update(&mut mac, part);
        }
        mac.finalize().into_bytes().into()
    }

    /// Keyed hash of an e-mail: what logs and the OTP table carry instead of the address.
    pub fn email_hash(&self, email: &EmailAddress) -> [u8; 32] {
        self.mac(b"gs-console:email:v1", &[email.as_str().as_bytes()])
    }

    /// HMAC of a code, bound to the e-mail it was issued for.
    pub fn code_hmac(&self, email: &EmailAddress, code: &str) -> [u8; 32] {
        self.mac(
            b"gs-console:otp:v1",
            &[email.as_str().as_bytes(), code.as_bytes()],
        )
    }

    /// Whether `submitted` is the code that produced `stored`, in constant time.
    pub fn verify_code(&self, email: &EmailAddress, submitted: &str, stored: &[u8]) -> bool {
        let candidate = self.code_hmac(email, submitted);
        candidate.as_slice().ct_eq(stored).into()
    }
}

/// Generates a one-time code of `length` decimal digits from the system CSPRNG.
///
/// Uses rejection sampling, so every digit is uniform: a plain `byte % 10` would favour the six
/// lowest digits. Out-of-range lengths are clamped to the documented bounds.
pub fn generate_code(length: usize) -> String {
    let length = length.clamp(OTP_MIN_LENGTH, OTP_MAX_LENGTH);
    let mut code = String::with_capacity(length);
    let mut buffer = [0u8; 32];
    while code.len() < length {
        rand::rng().fill_bytes(&mut buffer);
        for byte in buffer {
            // 250 = 25 * 10: bytes above it would bias the result.
            if byte < 250 && code.len() < length {
                code.push(char::from(b'0' + byte % 10));
            }
        }
    }
    code
}

/// Whether `text` could be a code of the documented shape (6 to 10 digits).
pub fn is_code_shaped(text: &str) -> bool {
    (OTP_MIN_LENGTH..=OTP_MAX_LENGTH).contains(&text.len())
        && text.bytes().all(|b| b.is_ascii_digit())
}

/// Generates a session token `gcs_<43 base64url>` (a secret: never log it).
pub fn generate_session_token() -> String {
    let mut bytes = [0u8; SESSION_TOKEN_BYTES];
    rand::rng().fill_bytes(&mut bytes);
    format!(
        "{SESSION_TOKEN_PREFIX}_{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    )
}

/// Whether `token` has the shape of a session token. Checked before touching the database so
/// garbage never costs a query.
pub fn is_session_token_shaped(token: &str) -> bool {
    token
        .strip_prefix(SESSION_TOKEN_PREFIX)
        .and_then(|rest| rest.strip_prefix('_'))
        .is_some_and(|payload| {
            payload.len() == SESSION_TOKEN_PAYLOAD_LEN
                && payload
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
}

/// SHA-256 of a session token: the only form in which it is stored.
pub fn hash_session_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn email(raw: &str) -> EmailAddress {
        EmailAddress::parse(raw).expect("valid e-mail")
    }

    fn keys() -> ConsoleKeys {
        ConsoleKeys::new(&[7u8; 32]).expect("key")
    }

    #[test]
    fn test_fr_con_001_emails_are_trimmed_and_lowercased() {
        // covers: FR-CON-001
        assert_eq!(
            email("  Ana.Perez@Example.COM ").as_str(),
            "ana.perez@example.com"
        );
        // `+tag` is kept: folding it would merge accounts of different mailboxes.
        assert_eq!(email("ana+geo@example.com").as_str(), "ana+geo@example.com");
    }

    #[test]
    fn test_fr_con_001_malformed_emails_are_rejected() {
        // covers: FR-CON-001 (the address ends up in a mail header: strict on purpose)
        for bad in [
            "",
            "   ",
            "sin-arroba",
            "@example.com",
            "ana@",
            "ana@@example.com",
            "ana@example",
            "ana@.com",
            "ana@example..com",
            "ana@-example.com",
            "ana@example.c",
            "ana@exam ple.com",
            "ana perez@example.com",
            ".ana@example.com",
            "ana.@example.com",
            "a..na@example.com",
            "ana@example.com\r\nBcc: victim@example.com",
            "ana@example.com\nSubject: x",
            "\"ana\"@example.com",
            "ana@[127.0.0.1]",
            "ana@ejemplo.cóm",
            "añá@example.com",
            "ana@123.456",
        ] {
            assert_eq!(
                EmailAddress::parse(bad),
                Err(ConsoleAuthError::InvalidEmail),
                "{bad:?}"
            );
        }
        let too_long = format!("{}@example.com", "a".repeat(65));
        assert!(EmailAddress::parse(&too_long).is_err());
        let huge = format!("a@{}.com", "b".repeat(300));
        assert!(EmailAddress::parse(&huge).is_err());
    }

    #[test]
    fn test_fr_con_011_the_email_is_redacted_in_debug() {
        // covers: FR-CON-011, docs/10 §8.2
        let rendered = format!("{:?}", email("secret.person@example.com"));
        assert!(!rendered.contains("secret"), "{rendered}");
        assert!(!rendered.contains("example"), "{rendered}");
        assert!(!format!("{:?}", keys()).contains('7'));
    }

    #[test]
    fn test_fr_con_001_a_code_verifies_only_with_the_right_email_key_and_digits() {
        // covers: FR-CON-001, 10 T-41
        let alice = email("alice@example.com");
        let bob = email("bob@example.com");
        let stored = keys().code_hmac(&alice, "482913");
        assert!(keys().verify_code(&alice, "482913", &stored));
        assert!(
            !keys().verify_code(&alice, "482914", &stored),
            "wrong digit"
        );
        assert!(
            !keys().verify_code(&bob, "482913", &stored),
            "the code is bound to its e-mail"
        );
        let other_key = ConsoleKeys::new(&[9u8; 32]).unwrap();
        assert!(
            !other_key.verify_code(&alice, "482913", &stored),
            "wrong server key"
        );
        assert!(
            !keys().verify_code(&alice, "482913", &stored[..31]),
            "truncated digest"
        );
        assert!(!keys().verify_code(&alice, "482913", &[]), "empty digest");
    }

    #[test]
    fn test_hmac_parts_are_length_prefixed() {
        // The e-mail and the code are different values even if their concatenation is equal.
        let k = keys();
        assert_ne!(
            k.mac(b"d", &[b"ab", b"c"]),
            k.mac(b"d", &[b"a", b"bc"]),
            "length prefixes avoid concatenation collisions"
        );
        // The domain separates the e-mail hash from a code HMAC over the same bytes.
        assert_ne!(
            k.mac(b"gs-console:email:v1", &[b"x"]),
            k.mac(b"gs-console:otp:v1", &[b"x"])
        );
    }

    #[test]
    fn test_weak_keys_are_refused() {
        assert_eq!(
            ConsoleKeys::new(&[1u8; 31]).unwrap_err(),
            ConsoleAuthError::WeakKey
        );
        assert_eq!(
            ConsoleKeys::new(&[]).unwrap_err(),
            ConsoleAuthError::WeakKey
        );
        assert!(ConsoleKeys::new(&[1u8; 32]).is_ok());
    }

    #[test]
    fn test_fr_con_001_codes_have_the_requested_length_and_only_digits() {
        // covers: FR-CON-001
        for length in OTP_MIN_LENGTH..=OTP_MAX_LENGTH {
            let code = generate_code(length);
            assert_eq!(code.len(), length);
            assert!(is_code_shaped(&code), "{code}");
        }
        assert_eq!(generate_code(0).len(), OTP_MIN_LENGTH, "clamped up");
        assert_eq!(generate_code(500).len(), OTP_MAX_LENGTH, "clamped down");
    }

    #[test]
    fn test_codes_are_not_biased_toward_low_digits() {
        // A naive `byte % 10` over 0..=255 gives digits 0-5 a 10 % advantage (26 vs 25 values).
        // 60 000 digits: each digit should be near 6 000; the biased version puts 0-5 near 6 154.
        let mut counts = [0usize; 10];
        for _ in 0..6_000 {
            for digit in generate_code(10).bytes() {
                counts[usize::from(digit - b'0')] += 1;
            }
        }
        for (digit, count) in counts.iter().enumerate() {
            assert!(
                (5_700..=6_300).contains(count),
                "digit {digit} appeared {count} times of 60000: {counts:?}"
            );
        }
    }

    #[test]
    fn test_codes_do_not_repeat() {
        let codes: std::collections::BTreeSet<String> =
            (0..2_000).map(|_| generate_code(8)).collect();
        assert!(
            codes.len() > 1_990,
            "unexpected repetition: {} distinct of 2000",
            codes.len()
        );
    }

    #[test]
    fn test_code_shape_rejects_non_digits_and_bad_lengths() {
        for bad in [
            "",
            "12345",
            "12345678901",
            "12345a",
            "１２３４５６",
            " 123456",
            "123 456",
            "12.456",
        ] {
            assert!(!is_code_shaped(bad), "{bad:?}");
        }
        assert!(is_code_shaped("000000"));
    }

    #[test]
    fn test_fr_con_010_session_tokens_have_the_documented_shape_and_hash() {
        // covers: FR-CON-010
        let token = generate_session_token();
        assert!(is_session_token_shaped(&token), "{token}");
        assert_eq!(
            token.len(),
            SESSION_TOKEN_PREFIX.len() + 1 + SESSION_TOKEN_PAYLOAD_LEN
        );
        assert_ne!(token, generate_session_token());
        assert_eq!(hash_session_token(&token), hash_session_token(&token));
        assert_ne!(hash_session_token(&token), hash_session_token("gcs_other"));
        // What is stored is never the token itself.
        assert!(
            !hash_session_token(&token)
                .windows(8)
                .any(|w| token.as_bytes().windows(8).any(|t| t == w))
        );
    }

    #[test]
    fn test_session_token_shape_rejects_other_credentials() {
        for bad in [
            "",
            "gcs_",
            "gcs_short",
            "gsk_0123456789abcdef0123456789abcdef.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "gst_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "Bearer gcs_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "gcs_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA!",
            "gcs_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ] {
            assert!(!is_session_token_shaped(bad), "{bad:?}");
        }
    }
}
