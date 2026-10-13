//! Verification of a device token against the truth in PostgreSQL: step 4 of `docs/06` §4.3.
//!
//! `gs-auth` covers steps 1 to 3 (format, EdDSA signature, `iss`/`aud`/`nbf`/`exp`). This
//! module adds the state checks that need the database: tenant and application `ACTIVE`,
//! subject `ACTIVE` with `token_epoch == sep`, and device `ACTIVE` for the same subject and
//! application. Step 5 (scope) is [`require_scope`].
#![forbid(unsafe_code)]

use gs_auth::jwt::{DeviceTokenClaims, SigningKeySet, verify_device_token};
use gs_auth::jwt::{JwtError, SigningKey};
use gs_domain::error::{AsErrorCode, ErrorCode};
use gs_store::models::{
    ApiKeyStatus, Application, ApplicationStatus, Device, Subject, SubjectStatus, Tenant,
    TenantStatus,
};
use gs_store::repos::{
    ApiKeyRepository, ApplicationRepository, DeviceRepository, SubjectRepository, TenantRepository,
};
use gs_store::{PgPool, StoreError};
use thiserror::Error;

/// A verified principal: the token claims plus the entities the database says they belong to.
#[derive(Debug, Clone)]
pub struct VerifiedPrincipal {
    /// Claims of the token.
    pub claims: DeviceTokenClaims,
    /// Tenant of the token.
    pub tenant: Tenant,
    /// Application of the token.
    pub application: Application,
    /// Subject of the token.
    pub subject: Subject,
    /// Device of the token.
    pub device: Device,
}

/// Errors of principal verification, mapped to the catalog of `docs/09` §6.
#[derive(Debug, Error)]
pub enum AuthError {
    /// The token failed steps 1 to 3 (docs/06 §4.3).
    #[error("invalid device token: {0}")]
    Token(#[from] JwtError),

    /// The tenant or application is disabled (docs/06 §4.3 step 4a, FR-ADM-003).
    #[error("account disabled: {0}")]
    AccountDisabled(String),

    /// The credential is no longer valid or deactivated (docs/06 §4.3 step 4b/4c: epoch mismatch, device deactivated, api key revoked/expired).
    #[error("unauthenticated: {0}")]
    Unauthenticated(String),

    /// The principal lacks the required scope.
    #[error("missing scope '{0}'")]
    MissingScope(String),

    /// A persistence operation failed.
    #[error("storage error: {0}")]
    Store(#[from] StoreError),
}

impl AsErrorCode for AuthError {
    fn error_code(&self) -> ErrorCode {
        match self {
            Self::Token(error) => error.error_code(),
            Self::AccountDisabled(_) => ErrorCode::AccountDisabled,
            Self::Unauthenticated(_) => ErrorCode::Unauthenticated,
            Self::MissingScope(_) => ErrorCode::InsufficientScope,
            Self::Store(_) => ErrorCode::Internal,
        }
    }
}

/// Verifies a device token and its principal state (docs/06 §4.3).
pub async fn verify_device_token_principal(
    pool: &PgPool,
    token: &str,
    keys: &SigningKeySet,
    issuer: &str,
    leeway_seconds: u64,
) -> Result<VerifiedPrincipal, AuthError> {
    // Steps 1 to 3: cryptographic verification (gs-auth).
    let claims = verify_device_token(token, keys, issuer, leeway_seconds)?.claims;

    // Step 4a: tenant and application state.
    let tenant = TenantRepository::get_by_id(pool, claims.tid)
        .await?
        .ok_or_else(|| AuthError::AccountDisabled("tenant not found".to_string()))?;
    if tenant.status != TenantStatus::Active {
        return Err(AuthError::AccountDisabled("tenant disabled".to_string()));
    }

    let application = ApplicationRepository::find_by_id(pool, claims.aid)
        .await?
        .ok_or_else(|| AuthError::AccountDisabled("application not found".to_string()))?;
    if application.tenant_id != tenant.id {
        return Err(AuthError::AccountDisabled(
            "application belongs to another tenant".to_string(),
        ));
    }
    if application.status != ApplicationStatus::Active {
        return Err(AuthError::AccountDisabled(
            "application disabled".to_string(),
        ));
    }

    // Step 4b: subject state and epoch (docs/06 §4.2 `sep`).
    let subject = SubjectRepository::get_by_id(pool, tenant.id, claims.sub)
        .await?
        .ok_or_else(|| AuthError::Unauthenticated("subject not found".to_string()))?;
    if subject.status != SubjectStatus::Active || subject.token_epoch != claims.sep {
        return Err(AuthError::Unauthenticated(
            "subject epoch changed or subject erased".to_string(),
        ));
    }

    // Step 4c: device state, bound to the same subject and application.
    let device = DeviceRepository::get_by_id(pool, tenant.id, claims.did)
        .await?
        .ok_or_else(|| AuthError::Unauthenticated("device not found".to_string()))?;
    if device.subject_id != subject.id || device.application_id != application.id {
        return Err(AuthError::Unauthenticated(
            "device belongs to another subject or application".to_string(),
        ));
    }
    if device.status != gs_store::models::DeviceStatus::Active {
        return Err(AuthError::Unauthenticated("device deactivated".to_string()));
    }

    Ok(VerifiedPrincipal {
        claims,
        tenant,
        application,
        subject,
        device,
    })
}

/// Step 5 of `docs/06` §4.3: the scope must be one of the granted `scp`.
pub fn require_scope(principal: &VerifiedPrincipal, scope: &str) -> Result<(), AuthError> {
    if principal.claims.has_scope(scope) {
        Ok(())
    } else {
        Err(AuthError::MissingScope(scope.to_string()))
    }
}

/// Verifies an application key against the database (docs/06 §3.2–§3.3).
pub async fn verify_application_key(
    pool: &PgPool,
    credential: &str,
) -> Result<gs_store::models::ApiKey, AuthError> {
    use gs_auth::api_key::parse_credential;

    let (key_id, secret) = parse_credential(credential)
        .map_err(|_| AuthError::Unauthenticated("malformed credential".to_string()))?;

    let api_key = ApiKeyRepository::find_by_id(pool, key_id)
        .await?
        .ok_or_else(|| AuthError::Unauthenticated("unknown api key".to_string()))?;

    if gs_auth::api_key::hash_secret(secret) != api_key.secret_sha256 {
        return Err(AuthError::Unauthenticated(
            "api key secret mismatch".to_string(),
        ));
    }
    if api_key.status != ApiKeyStatus::Active {
        return Err(AuthError::Unauthenticated("api key revoked".to_string()));
    }
    if let Some(expires_at) = api_key.expires_at
        && expires_at <= time::OffsetDateTime::now_utc()
    {
        return Err(AuthError::Unauthenticated("api key expired".to_string()));
    }

    Ok(api_key)
}

/// Identity of a verified application key: key, application and tenant.
#[derive(Debug, Clone)]
pub struct ApplicationPrincipal {
    /// The key that authenticated the request.
    pub api_key: gs_store::models::ApiKey,
    /// The application the key belongs to.
    pub application: Application,
    /// The tenant the application belongs to.
    pub tenant: Tenant,
}

/// Verifies an application key and its state, returning the full principal (docs/06 §3).
pub async fn verify_application_principal(
    pool: &PgPool,
    credential: &str,
) -> Result<ApplicationPrincipal, AuthError> {
    let api_key = verify_application_key(pool, credential).await?;

    let application = ApplicationRepository::find_by_id(pool, api_key.application_id)
        .await?
        .ok_or_else(|| AuthError::AccountDisabled("application not found".to_string()))?;
    if application.tenant_id != api_key.tenant_id {
        return Err(AuthError::AccountDisabled(
            "application belongs to another tenant".to_string(),
        ));
    }
    if application.status != ApplicationStatus::Active {
        return Err(AuthError::AccountDisabled(
            "application disabled".to_string(),
        ));
    }

    let tenant = TenantRepository::get_by_id(pool, application.tenant_id)
        .await?
        .ok_or_else(|| AuthError::AccountDisabled("tenant not found".to_string()))?;
    if tenant.status != TenantStatus::Active {
        return Err(AuthError::AccountDisabled("tenant disabled".to_string()));
    }

    Ok(ApplicationPrincipal {
        api_key,
        application,
        tenant,
    })
}

/// Convenience alias used by callers that hold the keyset.
pub type DeviceTokenKeys = SigningKeySet;

/// Convenience alias for the signing key type (docs/06 §4.4).
pub type ActiveSigningKey = SigningKey;
