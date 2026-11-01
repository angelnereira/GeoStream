//! Console sign-up and sign-in with e-mail one-time codes (ADR-0036, FR-CON-001, FR-CON-009 to
//! FR-CON-011).
//!
//! Registration and sign-in are one ceremony: ask for a code, then redeem it. Everything here
//! avoids telling a caller anything about which accounts exist: asking for a code answers the
//! same whether or not the address is registered, and every redemption failure is the same
//! `Unauthenticated`. Neither the e-mail nor the code is ever written to a log (docs/10 §8.2):
//! the only loggable form of an address is the first bytes of its keyed hash.
#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use thiserror::Error;
use time::OffsetDateTime;
use tracing::{error, warn};

use gs_auth::{
    ConsoleConfig, ConsoleKeys, EmailAddress, EmailConfig, SecretString, generate_code,
    generate_session_token, hash_session_token, is_code_shaped, is_session_token_shaped,
};
use gs_domain::error::{AsErrorCode, ErrorCode};
use gs_domain::ids::AccountId;
use gs_store::{ConsoleAccount, ConsoleRepository, OtpIssue, PgPool};

/// Default endpoint of the transactional e-mail provider.
pub const RESEND_API_URL: &str = "https://api.resend.com";

/// Longest wait for the provider before giving up on one message.
const MAIL_TIMEOUT: Duration = Duration::from_secs(10);

/// Failures of the console sign-in flow, each mapped to one canonical error code.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConsoleError {
    /// The address is not acceptable. Reveals nothing about accounts.
    #[error("invalid e-mail address")]
    InvalidEmail,
    /// The code does not have the documented shape (6 to 10 digits).
    #[error("invalid code format")]
    InvalidCodeFormat,
    /// The account does not exist yet and the terms were not accepted.
    #[error("the terms must be accepted to create an account")]
    TermsRequired,
    /// Wrong, expired, used or exhausted code; unknown, expired or revoked session. Never says which.
    #[error("unauthenticated")]
    Unauthenticated,
    /// The account cannot be deleted yet (it is the only owner of an organization with active projects).
    #[error("conflict")]
    Conflict,
    /// A dependency failed. Carries no detail: it could echo connection data.
    #[error("internal error")]
    Internal,
}

impl AsErrorCode for ConsoleError {
    fn error_code(&self) -> ErrorCode {
        match self {
            Self::InvalidEmail | Self::InvalidCodeFormat | Self::TermsRequired => {
                ErrorCode::ValidationFailed
            }
            Self::Unauthenticated => ErrorCode::Unauthenticated,
            Self::Conflict => ErrorCode::Conflict,
            Self::Internal => ErrorCode::Internal,
        }
    }
}

impl From<gs_store::StoreError> for ConsoleError {
    fn from(error: gs_store::StoreError) -> Self {
        // The store error can contain SQL text or connection details: log its kind only.
        error!(kind = "store", "console store operation failed");
        let _ = error;
        Self::Internal
    }
}

/// A message that could not be delivered. Carries no address, no code and no provider body.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum MailError {
    #[error("the mail provider could not be reached")]
    Unreachable,
    #[error("the mail provider rejected the message (HTTP {0})")]
    Rejected(u16),
    #[error("the message could not be stored")]
    Storage,
}

/// A message captured by the in-memory mailer of the tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentMail {
    pub to: String,
    pub subject: String,
    pub body: String,
}

/// Where the codes go. A closed set: static dispatch keeps the futures `Send` for axum.
pub enum EmailBackend {
    /// Transactional provider over HTTPS (production).
    Resend {
        client: reqwest::Client,
        base_url: String,
        api_key: SecretString,
        from: String,
    },
    /// One file per message in `dir` (dev and test only; never the logs).
    File { dir: PathBuf, from: String },
    /// Captured in memory (tests only; the configuration cannot select it).
    Memory(Mutex<Vec<SentMail>>),
}

impl std::fmt::Debug for EmailBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Resend { .. } => "EmailBackend::Resend",
            Self::File { .. } => "EmailBackend::File",
            Self::Memory(_) => "EmailBackend::Memory",
        })
    }
}

impl EmailBackend {
    /// Builds the backend selected by the configuration.
    pub fn from_config(config: &EmailConfig) -> Result<Self, MailError> {
        match config {
            EmailConfig::Resend { api_key, from } => {
                Self::resend(RESEND_API_URL, api_key.clone(), from)
            }
            EmailConfig::File { dir, from } => Ok(Self::File {
                dir: dir.clone(),
                from: from.clone(),
            }),
        }
    }

    /// A provider backend against `base_url` (a stub in tests).
    pub fn resend(base_url: &str, api_key: SecretString, from: &str) -> Result<Self, MailError> {
        let client = reqwest::Client::builder()
            .timeout(MAIL_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| MailError::Unreachable)?;
        Ok(Self::Resend {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            from: from.to_string(),
        })
    }

    /// An in-memory backend for tests.
    pub fn memory() -> Self {
        Self::Memory(Mutex::new(Vec::new()))
    }

    /// Messages captured by a memory backend; empty for any other.
    pub fn sent(&self) -> Vec<SentMail> {
        match self {
            Self::Memory(sent) => sent.lock().map(|g| g.clone()).unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// Sends the sign-in code. The code is the message: it never goes anywhere else.
    pub async fn send_code(
        &self,
        to: &EmailAddress,
        code: &str,
        ttl: Duration,
    ) -> Result<(), MailError> {
        let subject = "Tu código de acceso a GeoStream".to_string();
        let minutes = (ttl.as_secs() / 60).max(1);
        let body = format!(
            "Tu código de acceso a GeoStream es: {code}\n\n\
             Caduca en {minutes} minutos y solo sirve una vez.\n\n\
             Si no lo pediste tú, ignora este mensaje: nadie puede entrar sin el código. \
             GeoStream nunca te pedirá este código por teléfono ni por chat.\n"
        );
        match self {
            Self::Memory(sent) => {
                sent.lock().map_err(|_| MailError::Storage)?.push(SentMail {
                    to: to.as_str().to_string(),
                    subject,
                    body,
                });
                Ok(())
            }
            Self::File { dir, from } => {
                tokio::fs::create_dir_all(dir)
                    .await
                    .map_err(|_| MailError::Storage)?;
                let message = format!(
                    "From: {from}\nTo: {}\nSubject: {subject}\n\n{body}",
                    to.as_str()
                );
                tokio::fs::write(dir.join(format!("{}.txt", uuid::Uuid::now_v7())), message)
                    .await
                    .map_err(|_| MailError::Storage)
            }
            Self::Resend {
                client,
                base_url,
                api_key,
                from,
            } => {
                let payload = serde_json::json!({
                    "from": from,
                    "to": [to.as_str()],
                    "subject": subject,
                    "text": body,
                });
                let response = client
                    .post(format!("{base_url}/emails"))
                    .bearer_auth(api_key.expose_secret())
                    .header("Content-Type", "application/json")
                    .body(payload.to_string())
                    .send()
                    .await
                    .map_err(|_| MailError::Unreachable)?;
                let status = response.status();
                if status.is_success() {
                    Ok(())
                } else {
                    // The provider body can echo the address: it is dropped, never logged.
                    Err(MailError::Rejected(status.as_u16()))
                }
            }
        }
    }
}

/// Parameters of the sign-in flow, taken from the configuration (docs/17 §2.12).
#[derive(Debug, Clone)]
pub struct ConsoleSettings {
    pub otp_length: usize,
    pub otp_ttl: Duration,
    pub otp_max_attempts: i32,
    pub otp_cooldown: Duration,
    pub session_ttl: Duration,
    pub email_daily_cap: i32,
    pub max_orgs_per_account: i64,
}

impl ConsoleSettings {
    /// Reads the settings of an enabled console.
    pub fn from_config(config: &ConsoleConfig) -> Self {
        Self {
            otp_length: usize::try_from(config.otp_length).unwrap_or(6),
            otp_ttl: Duration::from_secs(config.otp_ttl_s),
            otp_max_attempts: i32::try_from(config.otp_max_attempts).unwrap_or(5),
            otp_cooldown: Duration::from_secs(config.otp_cooldown_s),
            session_ttl: Duration::from_secs(config.session_ttl_s),
            email_daily_cap: i32::try_from(config.email_daily_cap).unwrap_or(500),
            max_orgs_per_account: i64::try_from(config.max_orgs_per_account).unwrap_or(3),
        }
    }
}

/// An opened session: the token is shown once and stored only as its SHA-256.
pub struct OpenedSession {
    pub token: zeroize::Zeroizing<String>,
    pub expires_at: OffsetDateTime,
    pub account: ConsoleAccount,
    /// `true` when this redemption created the account.
    pub created: bool,
}

impl std::fmt::Debug for OpenedSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenedSession")
            .field("token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("account", &self.account)
            .field("created", &self.created)
            .finish()
    }
}

/// The console sign-in service.
pub struct ConsoleAuth {
    pool: PgPool,
    keys: ConsoleKeys,
    settings: ConsoleSettings,
    mailer: Arc<EmailBackend>,
}

impl ConsoleAuth {
    /// Builds the service.
    pub fn new(
        pool: PgPool,
        keys: ConsoleKeys,
        settings: ConsoleSettings,
        mailer: Arc<EmailBackend>,
    ) -> Self {
        Self {
            pool,
            keys,
            settings,
            mailer,
        }
    }

    /// The settings in force.
    pub fn settings(&self) -> &ConsoleSettings {
        &self.settings
    }

    /// Asks for a one-time code (`POST /v1/console/auth/code`).
    ///
    /// Returns `Ok(())` in every case where the address is well formed, whether or not it has an
    /// account, a code was sent, the cooldown applies, the daily budget is spent or the provider
    /// failed: the response must not reveal any of those (FR-CON-009, `10` T-42).
    pub async fn request_code(&self, raw_email: &str) -> Result<(), ConsoleError> {
        let email = EmailAddress::parse(raw_email).map_err(|_| ConsoleError::InvalidEmail)?;
        let email_hash = self.keys.email_hash(&email);
        let code = zeroize::Zeroizing::new(generate_code(self.settings.otp_length));
        let code_hmac = self.keys.code_hmac(&email, &code);

        let issued = ConsoleRepository::issue_otp(
            &self.pool,
            &email_hash,
            &code_hmac,
            self.settings.otp_ttl.as_secs_f64(),
            self.settings.otp_cooldown.as_secs_f64(),
        )
        .await?;
        if issued == OtpIssue::CoolingDown {
            return Ok(());
        }
        if !ConsoleRepository::take_email_budget(&self.pool, self.settings.email_daily_cap).await? {
            warn!(
                target: "gs_service::console",
                "daily e-mail budget reached: no code was sent"
            );
            return Ok(());
        }
        if let Err(mail_error) = self
            .mailer
            .send_code(&email, &code, self.settings.otp_ttl)
            .await
        {
            // Only the keyed hash prefix identifies the request; never the address.
            warn!(
                target: "gs_service::console",
                email_hash = %short_hash(&email_hash),
                outcome = %mail_error,
                "the sign-in code could not be delivered"
            );
        }
        Ok(())
    }

    /// Redeems a code and opens a session (`POST /v1/console/auth/verify`).
    ///
    /// Creates the account on first use, which requires `accept_terms`. Every failure of the
    /// code itself is the same `Unauthenticated`.
    pub async fn verify_code(
        &self,
        raw_email: &str,
        code: &str,
        accept_terms: bool,
    ) -> Result<OpenedSession, ConsoleError> {
        let email = EmailAddress::parse(raw_email).map_err(|_| ConsoleError::InvalidEmail)?;
        if !is_code_shaped(code) {
            return Err(ConsoleError::InvalidCodeFormat);
        }
        let email_hash = self.keys.email_hash(&email);

        // The attempt is counted before the comparison (one statement): parallel guesses cannot
        // exceed the limit.
        let attempt = ConsoleRepository::take_otp_attempt(
            &self.pool,
            &email_hash,
            self.settings.otp_max_attempts,
        )
        .await?
        .ok_or(ConsoleError::Unauthenticated)?;
        if !self.keys.verify_code(&email, code, &attempt.code_hmac) {
            return Err(ConsoleError::Unauthenticated);
        }

        // A new account needs the terms. Checked before consuming the code so the person can
        // accept and retry with the same code.
        let existing = ConsoleRepository::get_account_by_email(&self.pool, email.as_str()).await?;
        if existing.is_none() && !accept_terms {
            return Err(ConsoleError::TermsRequired);
        }
        if !ConsoleRepository::consume_otp(&self.pool, attempt.id).await? {
            return Err(ConsoleError::Unauthenticated);
        }
        let (account, created) = ConsoleRepository::find_or_create_account(
            &self.pool,
            email.as_str(),
            &email_hash,
            accept_terms,
        )
        .await?
        .ok_or(ConsoleError::TermsRequired)?;
        if account.status != gs_store::AccountStatus::Active {
            return Err(ConsoleError::Unauthenticated);
        }

        let token = zeroize::Zeroizing::new(generate_session_token());
        let expires_at = ConsoleRepository::create_session(
            &self.pool,
            account.id,
            &hash_session_token(&token),
            self.settings.session_ttl.as_secs_f64(),
        )
        .await?;
        Ok(OpenedSession {
            token,
            expires_at,
            account,
            created,
        })
    }

    /// The account of a session token, or `Unauthenticated`.
    pub async fn authenticate(&self, token: &str) -> Result<ConsoleAccount, ConsoleError> {
        if !is_session_token_shaped(token) {
            return Err(ConsoleError::Unauthenticated);
        }
        ConsoleRepository::find_active_session(&self.pool, &hash_session_token(token))
            .await?
            .ok_or(ConsoleError::Unauthenticated)
    }

    /// Revokes a session (`POST /v1/console/auth/logout`).
    pub async fn logout(&self, token: &str) -> Result<(), ConsoleError> {
        if !is_session_token_shaped(token) {
            return Err(ConsoleError::Unauthenticated);
        }
        ConsoleRepository::revoke_session(&self.pool, &hash_session_token(token)).await?;
        Ok(())
    }

    /// Deletes the account and its personal data (`DELETE /v1/console/me`).
    pub async fn delete_account(&self, account_id: AccountId) -> Result<(), ConsoleError> {
        if ConsoleRepository::is_sole_owner_of_active_projects(&self.pool, account_id).await? {
            return Err(ConsoleError::Conflict);
        }
        ConsoleRepository::delete_account(&self.pool, account_id).await?;
        Ok(())
    }
}

/// The first 4 bytes of a keyed hash in hex: enough to correlate log lines, useless to reverse.
fn short_hash(hash: &[u8; 32]) -> String {
    hash.iter().take(4).map(|b| format!("{b:02x}")).collect()
}
