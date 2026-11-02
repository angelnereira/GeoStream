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
    ConsoleConfig, ConsoleKeys, EmailAddress, EmailConfig, PASSWORD_MAX_LENGTH, PasswordHasher,
    PasswordPolicyError, SecretString, generate_code, generate_session_token, hash_session_token,
    is_code_shaped, is_session_token_shaped, validate_password_policy,
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
    /// The password does not meet the policy (length, easy to guess). Never carries the password.
    #[error("the password does not meet the policy")]
    WeakPassword(PasswordPolicyError),
    /// The current password given to change it is wrong. The caller already holds a valid session.
    #[error("the current password is not correct")]
    WrongPassword,
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
            Self::InvalidEmail
            | Self::InvalidCodeFormat
            | Self::TermsRequired
            | Self::WeakPassword(_) => ErrorCode::ValidationFailed,
            Self::WrongPassword => ErrorCode::PermissionDenied,
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

    /// Sends a registration code (see [`Self::send_code_for`]).
    pub async fn send_code(
        &self,
        to: &EmailAddress,
        code: &str,
        ttl: Duration,
    ) -> Result<(), MailError> {
        self.send_code_for(CodePurpose::Register, to, code, ttl)
            .await
    }

    /// Sends a one-time code. The code is the message: it never goes anywhere else.
    pub async fn send_code_for(
        &self,
        purpose: CodePurpose,
        to: &EmailAddress,
        code: &str,
        ttl: Duration,
    ) -> Result<(), MailError> {
        let (subject, what) = match purpose {
            CodePurpose::Register => (
                "Tu código para crear tu cuenta de GeoStream",
                "Tu código de acceso a GeoStream es",
            ),
            CodePurpose::Login => (
                "Tu código para entrar a GeoStream",
                "Tu código de acceso a GeoStream es",
            ),
        };
        let subject = subject.to_string();
        let minutes = (ttl.as_secs() / 60).max(1);
        let body = format!(
            "{what}: {code}\n\n\
             Caduca en {minutes} minutos y solo sirve una vez.\n\n\
             Si no lo pediste tú, ignora este mensaje: nadie puede entrar sin el código y sin tu \
             contraseña. GeoStream nunca te pedirá este código por teléfono ni por chat.\n"
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

/// What a one-time code is for. The two purposes never share a code, a cooldown or an attempt counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodePurpose {
    /// Creating an account or recovering access: proves control of the mailbox.
    Register,
    /// Second step of a sign-in, issued only after the password was correct.
    Login,
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
    /// PBKDF2 rounds of new password hashes.
    pub password_iterations: u32,
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
            password_iterations: u32::try_from(config.password_iterations)
                .unwrap_or(gs_auth::PASSWORD_DEFAULT_ITERATIONS),
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
    hasher: PasswordHasher,
}

impl ConsoleAuth {
    /// Builds the service.
    pub fn new(
        pool: PgPool,
        keys: ConsoleKeys,
        settings: ConsoleSettings,
        mailer: Arc<EmailBackend>,
    ) -> Self {
        let hasher = PasswordHasher::new(settings.password_iterations);
        Self {
            pool,
            keys,
            settings,
            mailer,
            hasher,
        }
    }

    /// The settings in force.
    pub fn settings(&self) -> &ConsoleSettings {
        &self.settings
    }

    /// Hashes a password off the async executor: it is expensive on purpose.
    async fn hash_password(&self, password: &str) -> Result<String, ConsoleError> {
        let hasher = self.hasher;
        let password = zeroize::Zeroizing::new(password.to_string());
        tokio::task::spawn_blocking(move || hasher.hash(&password))
            .await
            .map_err(|_| ConsoleError::Internal)
    }

    /// Checks a password against a stored hash off the async executor. With `None` it spends the same
    /// work on a dummy and answers `false`, so an unknown account takes as long as a known one.
    async fn check_password(&self, stored: Option<String>, password: &str) -> bool {
        let hasher = self.hasher;
        let password = zeroize::Zeroizing::new(password.to_string());
        tokio::task::spawn_blocking(move || match stored {
            Some(stored) => hasher.verify(&stored, &password),
            None => {
                hasher.verify_dummy(&password);
                false
            }
        })
        .await
        .unwrap_or(false)
    }

    /// Stores a code under `key` and mails it, unless the cooldown or the daily budget says no. Always
    /// `Ok`: the answer must not reveal which of those happened (FR-CON-009).
    async fn issue_code(
        &self,
        purpose: CodePurpose,
        email: &EmailAddress,
    ) -> Result<(), ConsoleError> {
        let code = zeroize::Zeroizing::new(generate_code(self.settings.otp_length));
        let (key, code_hmac) = match purpose {
            CodePurpose::Register => (
                self.keys.email_hash(email),
                self.keys.code_hmac(email, &code),
            ),
            CodePurpose::Login => (
                self.keys.login_key(email),
                self.keys.login_code_hmac(email, &code),
            ),
        };
        let issued = ConsoleRepository::issue_otp(
            &self.pool,
            &key,
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
            .send_code_for(purpose, email, &code, self.settings.otp_ttl)
            .await
        {
            metrics::counter!("gs_console_mail_failed_total").increment(1);
            // Only the keyed hash prefix identifies the request; never the address.
            warn!(
                target: "gs_service::console",
                email_hash = %short_hash(&key),
                outcome = %mail_error,
                "the sign-in code could not be delivered"
            );
        }
        Ok(())
    }

    /// Asks for a registration code (`POST /v1/console/auth/code`): the first step of creating an
    /// account or of recovering access to one.
    ///
    /// Returns `Ok(())` in every case where the address is well formed, whether or not it has an
    /// account, a code was sent, the cooldown applies, the daily budget is spent or the provider
    /// failed: the response must not reveal any of those (FR-CON-009, `10` T-42).
    pub async fn request_code(&self, raw_email: &str) -> Result<(), ConsoleError> {
        let email = EmailAddress::parse(raw_email).map_err(|_| ConsoleError::InvalidEmail)?;
        self.issue_code(CodePurpose::Register, &email).await
    }

    /// Redeems a registration code, sets the password and opens a session
    /// (`POST /v1/console/auth/verify`).
    ///
    /// Creates the account on first use, which requires `accept_terms`. For an account that already
    /// exists it replaces the password: control of the mailbox is the proof, as in any e-mail recovery
    /// (ADR-0037). Every failure of the code itself is the same `Unauthenticated`.
    pub async fn verify_code(
        &self,
        raw_email: &str,
        code: &str,
        password: &str,
        accept_terms: bool,
    ) -> Result<OpenedSession, ConsoleError> {
        let email = EmailAddress::parse(raw_email).map_err(|_| ConsoleError::InvalidEmail)?;
        if !is_code_shaped(code) {
            return Err(ConsoleError::InvalidCodeFormat);
        }
        // The password is checked first and without touching the code: the person can fix it and retry
        // with the same code, and the answer reveals nothing about the account.
        validate_password_policy(password, email.as_str()).map_err(ConsoleError::WeakPassword)?;
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
        if existing
            .as_ref()
            .is_some_and(|a| a.status != gs_store::AccountStatus::Active)
        {
            return Err(ConsoleError::Unauthenticated);
        }
        let password_hash = self.hash_password(password).await?;
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
        ConsoleRepository::set_password_hash(&self.pool, account.id, &password_hash).await?;
        let mut account = account;
        account.has_password = true;
        self.open_session(account, created).await
    }

    /// First step of a sign-in (`POST /v1/console/auth/login`): checks the password and, only when it
    /// is right, mails the second-step code.
    ///
    /// The answer is `Ok(())` whatever happened — wrong password, unknown account, account without a
    /// password, disabled account — so it cannot be used to find out which e-mails are registered. An
    /// unknown account costs the same work as a known one.
    pub async fn login(&self, raw_email: &str, password: &str) -> Result<(), ConsoleError> {
        let email = EmailAddress::parse(raw_email).map_err(|_| ConsoleError::InvalidEmail)?;
        if password.chars().count() > PASSWORD_MAX_LENGTH {
            // Bounds the work one request can force; nobody's password is this long.
            return Ok(());
        }
        let record = ConsoleRepository::get_login_record(&self.pool, email.as_str()).await?;
        let (usable, stored) = match record {
            Some(record)
                if record.status == gs_store::AccountStatus::Active
                    && record.password_hash.is_some() =>
            {
                (Some(record.account_id), record.password_hash)
            }
            _ => (None, None),
        };
        let correct = self.check_password(stored.clone(), password).await;
        let Some(account_id) = usable.filter(|_| correct) else {
            return Ok(());
        };
        if stored.is_some_and(|hash| self.hasher.needs_rehash(&hash)) {
            // The iteration count went up since this hash was made: upgrade it with the password at hand.
            let upgraded = self.hash_password(password).await?;
            ConsoleRepository::set_password_hash(&self.pool, account_id, &upgraded).await?;
        }
        self.issue_code(CodePurpose::Login, &email).await
    }

    /// Second step of a sign-in (`POST /v1/console/auth/login/verify`): redeems the code mailed after
    /// a correct password and opens a session. Every failure is the same `Unauthenticated`.
    pub async fn verify_login(
        &self,
        raw_email: &str,
        code: &str,
    ) -> Result<OpenedSession, ConsoleError> {
        let email = EmailAddress::parse(raw_email).map_err(|_| ConsoleError::InvalidEmail)?;
        if !is_code_shaped(code) {
            return Err(ConsoleError::InvalidCodeFormat);
        }
        let key = self.keys.login_key(&email);
        let attempt =
            ConsoleRepository::take_otp_attempt(&self.pool, &key, self.settings.otp_max_attempts)
                .await?
                .ok_or(ConsoleError::Unauthenticated)?;
        if !self
            .keys
            .verify_login_code(&email, code, &attempt.code_hmac)
        {
            return Err(ConsoleError::Unauthenticated);
        }
        let record = ConsoleRepository::get_login_record(&self.pool, email.as_str())
            .await?
            .filter(|r| r.status == gs_store::AccountStatus::Active && r.password_hash.is_some())
            .ok_or(ConsoleError::Unauthenticated)?;
        if !ConsoleRepository::consume_otp(&self.pool, attempt.id).await? {
            return Err(ConsoleError::Unauthenticated);
        }
        let account = ConsoleRepository::get_account_by_email(&self.pool, email.as_str())
            .await?
            .ok_or(ConsoleError::Unauthenticated)?;
        debug_assert_eq!(account.id, record.account_id);
        self.open_session(account, false).await
    }

    /// Changes the password of the signed-in account (`PUT /v1/console/me/password`).
    ///
    /// Needs the current password (an account that never set one is exempt: its session is the proof),
    /// applies the policy and ends every other session of the account.
    pub async fn change_password(
        &self,
        account: &ConsoleAccount,
        token: &str,
        current: &str,
        new: &str,
    ) -> Result<(), ConsoleError> {
        validate_password_policy(new, &account.email).map_err(ConsoleError::WeakPassword)?;
        if current.chars().count() <= PASSWORD_MAX_LENGTH {
            let stored = ConsoleRepository::get_password_hash(&self.pool, account.id).await?;
            if stored.is_some() && !self.check_password(stored, current).await {
                return Err(ConsoleError::WrongPassword);
            }
        } else {
            return Err(ConsoleError::WrongPassword);
        }
        let hash = self.hash_password(new).await?;
        ConsoleRepository::set_password_hash(&self.pool, account.id, &hash).await?;
        ConsoleRepository::revoke_other_sessions(
            &self.pool,
            account.id,
            &hash_session_token(token),
        )
        .await?;
        Ok(())
    }

    /// Opens a session for an account whose identity has already been proven.
    async fn open_session(
        &self,
        account: ConsoleAccount,
        created: bool,
    ) -> Result<OpenedSession, ConsoleError> {
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
