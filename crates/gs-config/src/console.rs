//! Console configuration (docs/17 §2.12, ADR-0036).
#![forbid(unsafe_code)]

use std::fmt;
use std::path::PathBuf;

use crate::secret::SecretString;

/// Where the one-time codes are delivered.
#[derive(Clone)]
pub enum EmailConfig {
    /// Transactional e-mail provider over HTTPS (the only one allowed in production).
    Resend { api_key: SecretString, from: String },
    /// Each message is written to a file in `dir`. Only in `dev` and `test`; never to the logs.
    File { dir: PathBuf, from: String },
}

impl fmt::Debug for EmailConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resend { from, .. } => f
                .debug_struct("Resend")
                .field("api_key", &"<redacted>")
                .field("from", from)
                .finish(),
            Self::File { dir, from } => f
                .debug_struct("File")
                .field("dir", dir)
                .field("from", from)
                .finish(),
        }
    }
}

/// HMAC key that binds codes and e-mails. Redacted in `Debug`.
#[derive(Clone)]
pub struct ConsoleHmacKey(Vec<u8>);

impl ConsoleHmacKey {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// The raw key bytes: a secret, never to be logged.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for ConsoleHmacKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ConsoleHmacKey(<redacted>)")
    }
}

/// Settings of the console, present only when it is enabled (`GS_CONSOLE_OTP_HMAC_KEY` set).
#[derive(Debug, Clone)]
pub struct ConsoleConfig {
    /// Origin of the portal (`GS_CONSOLE_PUBLIC_URL`).
    pub public_url: String,
    pub otp_length: u64,
    pub otp_ttl_s: u64,
    pub otp_max_attempts: u64,
    pub otp_cooldown_s: u64,
    pub otp_hmac_key: ConsoleHmacKey,
    pub session_ttl_s: u64,
    pub email: EmailConfig,
    pub email_daily_cap: u64,
    pub max_orgs_per_account: u64,
    pub signup_rpm_per_ip: u64,
}
