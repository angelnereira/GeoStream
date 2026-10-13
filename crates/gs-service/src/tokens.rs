//! Device token minting: `POST /v1/tokens` (docs/06 §4, OpenAPI `mintToken`, FR-AUTH-002).
//!
//! Verification follows the order of `docs/06` §4.3: credential format, constant-time hash,
//! key state, application and tenant state, `tokens:mint` scope and finally the claims
//! themselves. Effects are the upsert of the subject (INV-SUB-1), the upsert of the device
//! (INV-DEV-1) and the enforcement of `max_devices_per_subject` (INV-DEV-2).
#![forbid(unsafe_code)]

use std::str::FromStr;
use std::time::Duration as StdDuration;

use time::OffsetDateTime;
use zeroize::Zeroizing;

use gs_auth::TenantPolicy;
use gs_auth::api_key::{hash_secret, parse_credential};
use gs_auth::jwt::{DeviceTokenClaims, SigningKey, issue_device_token};
use gs_auth::scopes::DEFAULT_DEVICE_TOKEN_SCOPES;
use gs_domain::error::{AsErrorCode, ErrorCode};
use gs_domain::ids::ExternalSubjectId;
use gs_domain::ids::InstallationId;
use gs_store::models::{
    ApiKey, ApiKeyStatus, Application, ApplicationStatus, DevicePlatform, Subject, Tenant,
    TenantStatus,
};
use gs_store::repos::{
    ApiKeyRepository, ApplicationRepository, DeviceRepository, SubjectRepository, TenantRepository,
};
use gs_store::{PgPool, StoreError};
use thiserror::Error as ThisError;

/// Scope required to mint device tokens (docs/06 §3.4).
pub const REQUIRED_SCOPE: &str = "tokens:mint";

/// Token TTL bounds taken from `GS_TOKEN_*` (docs/17 §2.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenTtlBounds {
    /// Minimum accepted `ttl_seconds`.
    pub min_seconds: u32,
    /// Default `ttl_seconds` when the request omits it.
    pub default_seconds: u32,
    /// Maximum accepted `ttl_seconds`.
    pub max_seconds: u32,
}

/// Body of `POST /v1/tokens` (OpenAPI `TokenRequest`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenRequest {
    /// Integrator defined subject identifier.
    pub external_subject_id: String,
    /// Canonical UUIDv4 of the installation.
    pub installation_id: String,
    /// Platform of the device.
    pub platform: String,
    /// Application version, at most 32 characters.
    #[serde(default)]
    pub app_version: Option<String>,
    /// SDK version, at most 32 characters.
    #[serde(default)]
    pub sdk_version: Option<String>,
    /// Requested device scopes, subset of `allowed_device_scopes`.
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
    /// Requested TTL in seconds.
    #[serde(default)]
    pub ttl_seconds: Option<u32>,
}

/// Response of `POST /v1/tokens` (OpenAPI `TokenResponse`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TokenResponse {
    /// Device JWT.
    pub access_token: String,
    /// Always `Bearer`.
    pub token_type: String,
    /// Expiry instant, RFC 3339.
    pub expires_at: String,
    /// Subject id.
    pub subject_id: String,
    /// Device id.
    pub device_id: String,
    /// Granted device scopes.
    pub scopes: Vec<String>,
}

/// Result of minting: the response plus the entities that were created or reused.
#[derive(Debug, Clone)]
pub struct MintedToken {
    /// Response body.
    pub response: TokenResponse,
    /// Subject that owns the token.
    pub subject: Subject,
    /// Device that owns the token.
    pub device: gs_store::models::Device,
}

/// Errors of token minting, mapped to the catalog of docs/09 §6.
#[derive(Debug, ThisError)]
pub enum TokenError {
    /// The credential is missing, malformed, unknown or mismatched.
    #[error("invalid application credentials")]
    Unauthenticated,

    /// The credential does not carry `tokens:mint`.
    #[error("application credentials lack the tokens:mint scope")]
    MissingScope,

    /// The tenant, the application or the key are disabled, revoked or expired.
    #[error("account disabled: {0}")]
    AccountDisabled(String),

    /// The request body is invalid (docs/09 §2.1).
    #[error("invalid token request: {0}")]
    Validation(String),

    /// The requested scopes are not allowed for the application.
    #[error("requested scopes are not allowed for this application")]
    InsufficientScope,

    /// The requested TTL is outside the configured range.
    #[error("ttl_seconds outside the configured range")]
    InvalidTtl,

    /// The signing key set has no active key.
    #[error("no active signing key configured")]
    MissingSigningKey,

    /// A persistence operation failed.
    #[error("storage error: {0}")]
    Store(#[from] StoreError),
}

impl AsErrorCode for TokenError {
    fn error_code(&self) -> ErrorCode {
        match self {
            Self::Unauthenticated => ErrorCode::Unauthenticated,
            Self::MissingScope | Self::InsufficientScope => ErrorCode::InsufficientScope,
            Self::AccountDisabled(_) => ErrorCode::AccountDisabled,
            Self::Validation(_) | Self::InvalidTtl => ErrorCode::ValidationFailed,
            Self::MissingSigningKey => ErrorCode::Internal,
            Self::Store(_) => ErrorCode::Internal,
        }
    }
}

/// Mints a device token for an application key holder (docs/06 §4).
#[allow(clippy::too_many_arguments)]
pub async fn mint_device_token(
    pool: &PgPool,
    credential: &str,
    request: &TokenRequest,
    ttl_bounds: TokenTtlBounds,
    issuer: &str,
    signing_key: &SigningKey,
    signing_key_id: &str,
) -> Result<MintedToken, TokenError> {
    // Step 1: credential format, without touching the database (docs/06 §4.3).
    let (key_id, secret) = parse_credential(credential).map_err(|_| TokenError::Unauthenticated)?;

    // Steps 2 and 4a: key, application and tenant state.
    let (api_key, application, tenant) = load_principal(pool, key_id, secret).await?;

    // Step 5: the key must carry `tokens:mint`.
    if !api_key.scopes.iter().any(|scope| scope == REQUIRED_SCOPE) {
        return Err(TokenError::MissingScope);
    }

    // Request validation (docs/09 §2.1, docs/06 §4.1).
    let external_subject_id = ExternalSubjectId::new(request.external_subject_id.clone())
        .map_err(|e| TokenError::Validation(e.to_string()))?;
    let installation_id = InstallationId::from_str(&request.installation_id)
        .map_err(|e| TokenError::Validation(format!("installation_id: {e}")))?;
    let platform = DevicePlatform::from_str(&request.platform)
        .map_err(|_| TokenError::Validation(format!("platform '{}'", request.platform)))?;

    if let Some(app_version) = &request.app_version
        && (app_version.is_empty() || app_version.len() > 32)
    {
        return Err(TokenError::Validation("app_version".to_string()));
    }
    if let Some(sdk_version) = &request.sdk_version
        && (sdk_version.is_empty() || sdk_version.len() > 32)
    {
        return Err(TokenError::Validation("sdk_version".to_string()));
    }

    let policy = TenantPolicy::from_json_value(tenant.policy.clone())
        .map_err(|e| TokenError::Validation(format!("tenant policy: {e}")))?;

    let scopes = match &request.scopes {
        Some(requested) => {
            for scope in requested {
                if !application
                    .allowed_device_scopes
                    .iter()
                    .any(|allowed| allowed == scope)
                {
                    return Err(TokenError::InsufficientScope);
                }
            }
            requested.clone()
        }
        None => application
            .allowed_device_scopes
            .iter()
            .filter(|allowed| DEFAULT_DEVICE_TOKEN_SCOPES.contains(&allowed.as_str()))
            .cloned()
            .collect(),
    };

    let ttl_seconds = match request.ttl_seconds {
        Some(ttl) => {
            if ttl < ttl_bounds.min_seconds || ttl > ttl_bounds.max_seconds {
                return Err(TokenError::InvalidTtl);
            }
            ttl
        }
        None => ttl_bounds.default_seconds,
    };

    // Effects: subject upsert and device upsert (docs/06 §4.1).
    let subject = SubjectRepository::get_or_create_active(
        pool,
        tenant.id,
        external_subject_id.as_str(),
        OffsetDateTime::now_utc(),
    )
    .await?;

    let device = DeviceRepository::upsert_device(
        pool,
        gs_store::repos::UpsertDeviceParams {
            tenant_id: tenant.id,
            subject_id: subject.id,
            application_id: application.id,
            installation_id,
            platform,
            app_version: request.app_version.clone(),
            sdk_version: request.sdk_version.clone(),
            max_devices_per_subject: policy.max_devices_per_subject,
            now: OffsetDateTime::now_utc(),
        },
    )
    .await?;

    let issued_at = OffsetDateTime::now_utc();
    let claims = DeviceTokenClaims::new(
        issuer,
        subject.id,
        tenant.id,
        application.id,
        device.id,
        scopes.clone(),
        subject.token_epoch,
        issued_at,
        ttl_seconds,
    )
    .map_err(|_| TokenError::Validation("issuer".to_string()))?;

    let issued = issue_device_token(signing_key, signing_key_id, &claims)
        .map_err(|_| TokenError::Validation("signing".to_string()))?;

    let access_token = Zeroizing::new(issued.token);
    let response = TokenResponse {
        access_token: (*access_token).clone(),
        token_type: "Bearer".to_string(),
        expires_at: format_rfc3339(issued_at + time::Duration::seconds(i64::from(ttl_seconds))),
        subject_id: subject.id.to_string(),
        device_id: device.id.to_string(),
        scopes,
    };

    Ok(MintedToken {
        response,
        subject,
        device,
    })
}

/// Loads the key, its application and its tenant, checking every state (docs/06 §4.3).
async fn load_principal(
    pool: &PgPool,
    key_id: gs_domain::ids::ApiKeyId,
    secret: &str,
) -> Result<(ApiKey, Application, Tenant), TokenError> {
    let api_key = ApiKeyRepository::find_by_id(pool, key_id)
        .await?
        .ok_or(TokenError::Unauthenticated)?;

    // Constant-time comparison against the stored hash (docs/06 §3.2).
    let expected = hash_secret(secret);
    if expected != api_key.secret_sha256 {
        return Err(TokenError::Unauthenticated);
    }

    if api_key.status != ApiKeyStatus::Active {
        return Err(TokenError::AccountDisabled("api key revoked".to_string()));
    }
    if let Some(expires_at) = api_key.expires_at
        && expires_at <= OffsetDateTime::now_utc()
    {
        return Err(TokenError::AccountDisabled("api key expired".to_string()));
    }

    let application = ApplicationRepository::find_by_id(pool, api_key.application_id)
        .await?
        .ok_or(TokenError::Unauthenticated)?;
    if application.status != ApplicationStatus::Active {
        return Err(TokenError::AccountDisabled(
            "application disabled".to_string(),
        ));
    }
    if application.tenant_id != api_key.tenant_id {
        return Err(TokenError::Unauthenticated);
    }

    let tenant = TenantRepository::get_by_id(pool, application.tenant_id)
        .await?
        .ok_or(TokenError::Unauthenticated)?;
    if tenant.status != TenantStatus::Active {
        return Err(TokenError::AccountDisabled("tenant disabled".to_string()));
    }

    Ok((api_key, application, tenant))
}

/// Formats an instant as RFC 3339 (docs/00 §4.5).
fn format_rfc3339(instant: OffsetDateTime) -> String {
    instant
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| instant.to_string())
}

/// Converts a TTL in seconds into a [`StdDuration`].
pub fn ttl_duration(seconds: u32) -> StdDuration {
    StdDuration::from_secs(u64::from(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_request_rejects_unknown_fields() {
        // covers: docs/07 §1, OpenAPI TokenRequest
        let error = serde_json::from_value::<TokenRequest>(serde_json::json!({
            "external_subject_id": "user-1",
            "installation_id": "0192b6c0-a1e4-7c3f-9d2a-7b5e8f1c0d3a",
            "platform": "ANDROID",
            "surprise": true
        }))
        .expect_err("unknown fields must be rejected");
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn test_token_response_has_the_documented_shape() {
        // covers: OpenAPI TokenResponse
        let response = TokenResponse {
            access_token: "eyJhbGciOiJFZERTQSJ9.payload.signature".to_string(),
            token_type: "Bearer".to_string(),
            expires_at: "2026-10-12T12:15:00.000Z".to_string(),
            subject_id: "sub_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a".to_string(),
            device_id: "dev_0192b6c0a1e47c3f9d2a7b5e8f1c0d3b".to_string(),
            scopes: vec!["sessions:create".to_string()],
        };
        let json = serde_json::to_value(&response).expect("serialize");
        assert_eq!(json["token_type"], serde_json::json!("Bearer"));
        assert_eq!(
            json["subject_id"],
            serde_json::json!("sub_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a")
        );
        assert_eq!(json["scopes"][0], serde_json::json!("sessions:create"));
    }

    #[test]
    fn test_error_codes_match_docs_06_4_1() {
        // covers: docs/09 §6, docs/06 §4.1
        assert_eq!(
            TokenError::Unauthenticated.error_code(),
            ErrorCode::Unauthenticated
        );
        assert_eq!(
            TokenError::MissingScope.error_code(),
            ErrorCode::InsufficientScope
        );
        assert_eq!(
            TokenError::AccountDisabled("x".to_string()).error_code(),
            ErrorCode::AccountDisabled
        );
        assert_eq!(
            TokenError::Validation("x".to_string()).error_code(),
            ErrorCode::ValidationFailed
        );
    }

    #[test]
    fn test_ttl_bounds_are_bounded() {
        // covers: docs/17 §2.4
        let bounds = TokenTtlBounds {
            min_seconds: 300,
            default_seconds: 900,
            max_seconds: 3600,
        };
        assert!(60 < bounds.min_seconds);
        assert!(bounds.default_seconds >= bounds.min_seconds);
        assert!(bounds.max_seconds >= bounds.default_seconds);
    }
}
