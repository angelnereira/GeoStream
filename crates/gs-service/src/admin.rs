//! Administrative use cases behind `geostream admin` (docs/16 §3, FR-ADM-001, FR-ADM-002).
//!
//! Every mutation is audited with `actor_kind = OPERATOR` using the action names of
//! `docs/10_SECURITY_PRIVACY.md` §8.3. Bus invalidation (`TenantDisabled`,
//! `ApplicationDisabled`, `ApiKeyRevoked`) is published by the server once `gs-bus`
//! exists (T-F2-05): a one-shot CLI process has no ephemeral store of its own to notify.
#![forbid(unsafe_code)]

use std::str::FromStr;

use time::{Duration, OffsetDateTime};
use zeroize::Zeroizing;

use gs_auth::api_key::{format_credential, generate_secret, hash_secret};
use gs_auth::signing_key::{self, SigningKeyError};
use gs_auth::{PolicyError, TenantPolicy};
use gs_bus::{Bus, InMemoryBus};
use gs_domain::ids::{ApiKeyId, ApplicationId, AuditId, TenantId};
use gs_store::models::{
    ActorKind, ApiKey, ApiKeyStatus, Application, ApplicationStatus, AuditEvent, AuditOutcome,
    Tenant, TenantStatus,
};
use gs_store::repos::{ApiKeyRepository, ApplicationRepository, AuditRepository, TenantRepository};
use gs_store::{PgPool, StoreError};
use uuid::Uuid;

/// Default `allowed_device_scopes` of a new application (docs/11 §2, docs/06 §3.4).
pub const DEFAULT_APPLICATION_DEVICE_SCOPES: &[&str] = &[
    "sessions:create",
    "invitations:redeem",
    "stream:connect",
    "sharing:write",
    "ingest:write",
];

/// Maximum length of a tenant name, application name or api key label (docs/11 §2).
pub const NAME_MAX_LEN: usize = 100;

/// Errors returned by the administrative use cases.
#[derive(Debug, thiserror::Error)]
pub enum AdminError {
    /// A tenant id could not be parsed.
    #[error("invalid tenant id '{value}': {source}")]
    InvalidTenantId {
        value: String,
        source: gs_domain::ids::IdParseError,
    },

    /// An application id could not be parsed.
    #[error("invalid application id '{value}': {source}")]
    InvalidApplicationId {
        value: String,
        source: gs_domain::ids::IdParseError,
    },

    /// An api key id could not be parsed.
    #[error("invalid api key id '{value}': {source}")]
    InvalidApiKeyId {
        value: String,
        source: gs_domain::ids::IdParseError,
    },

    /// The tenant name violates the schema constraint of docs/11 §2.
    #[error("tenant name must be between 1 and 100 characters")]
    InvalidTenantName,

    /// The application name violates the schema constraint of docs/11 §2.
    #[error("application name must be between 1 and 100 characters")]
    InvalidApplicationName,

    /// The api key label violates the schema constraint of docs/11 §2.
    #[error("api key label must be at most 100 characters")]
    InvalidApiKeyLabel,

    /// The tenant does not exist.
    #[error("tenant not found: {tenant_id}")]
    TenantNotFound { tenant_id: String },

    /// The application does not exist inside the given tenant.
    #[error("application not found in tenant {tenant_id}: {app_id}")]
    ApplicationNotFound { tenant_id: String, app_id: String },

    /// The api key does not exist inside the given tenant.
    #[error("api key not found in tenant {tenant_id}: {key_id}")]
    ApiKeyNotFound { tenant_id: String, key_id: String },

    /// The scope list is not part of the catalog of docs/06 §3.4.
    #[error("invalid scopes: {0}")]
    InvalidScopes(#[from] gs_auth::scopes::ScopeError),

    /// A tenant policy value failed validation (docs/17 §4).
    #[error("invalid tenant policy: {0}")]
    InvalidPolicy(#[from] PolicyError),

    /// A signing key pair could not be generated or written (docs/06 §4.4).
    #[error("signing key error: {0}")]
    SigningKey(#[from] SigningKeyError),

    /// A persistence operation failed.
    #[error("storage error: {0}")]
    Store(#[from] StoreError),
}

/// A freshly created api key: the stored record plus the credential shown once.
pub struct CreatedApiKey {
    /// The persisted record: never contains the secret.
    pub key: ApiKey,
    /// The `gsk_…` credential. Displayed once; never logged nor persisted.
    pub credential: Zeroizing<String>,
}

/// Paths of a signing key pair written to `GS_JWT_KEYS_DIR` (docs/06 §4.4).
pub struct GeneratedSigningKeyFiles {
    /// Key id of the generated pair.
    pub kid: String,
    /// Path of the private key file (mode `0600`).
    pub private_key_path: std::path::PathBuf,
    /// Path of the public key file.
    pub public_key_path: std::path::PathBuf,
}

/// Creates a tenant and audits `tenant.created` (docs/16 §3).
pub async fn create_tenant(pool: &PgPool, name: &str) -> Result<Tenant, AdminError> {
    let name = name.trim();
    if name.is_empty() || name.len() > NAME_MAX_LEN {
        return Err(AdminError::InvalidTenantName);
    }

    let now = OffsetDateTime::now_utc();
    let tenant = Tenant {
        id: TenantId::new(),
        name: name.to_string(),
        status: TenantStatus::Active,
        policy: serde_json::json!({}),
        created_at: now,
        updated_at: now,
    };

    TenantRepository::create(pool, &tenant).await?;
    record_audit(
        pool,
        tenant.id,
        "tenant.created",
        "tenant",
        tenant.id.into_uuid(),
        serde_json::json!({"name": tenant.name}),
    )
    .await?;

    Ok(tenant)
}

/// Lists every tenant (operator scope, docs/16 §3).
pub async fn list_tenants(pool: &PgPool) -> Result<Vec<Tenant>, AdminError> {
    Ok(TenantRepository::list(pool).await?)
}

/// Fetches one tenant by id.
pub async fn get_tenant(pool: &PgPool, tenant_id: &str) -> Result<Tenant, AdminError> {
    let tenant_id = parse_tenant_id(tenant_id)?;
    let tenant = TenantRepository::get_by_id(pool, tenant_id)
        .await?
        .ok_or_else(|| AdminError::TenantNotFound {
            tenant_id: tenant_id.to_string(),
        })?;
    Ok(tenant)
}

/// Enables or disables a tenant with optional bus invalidation (docs/16 §3, FR-ADM-003).
pub async fn set_tenant_enabled_with_bus<B: Bus>(
    pool: &PgPool,
    tenant_id: &str,
    enabled: bool,
    bus: Option<&B>,
    key_prefix: Option<&str>,
) -> Result<Tenant, AdminError> {
    let tenant_id = parse_tenant_id(tenant_id)?;
    let current = TenantRepository::get_by_id(pool, tenant_id)
        .await?
        .ok_or_else(|| AdminError::TenantNotFound {
            tenant_id: tenant_id.to_string(),
        })?;

    let desired = if enabled {
        TenantStatus::Active
    } else {
        TenantStatus::Disabled
    };
    if current.status == desired {
        return Ok(current);
    }

    let now = OffsetDateTime::now_utc();
    let updated = TenantRepository::update_status(pool, tenant_id, desired, now).await?;
    if !updated {
        return Err(AdminError::TenantNotFound {
            tenant_id: tenant_id.to_string(),
        });
    }

    let action = if enabled {
        "tenant.enabled"
    } else {
        "tenant.disabled"
    };
    record_audit(
        pool,
        tenant_id,
        action,
        "tenant",
        tenant_id.into_uuid(),
        serde_json::json!({"previous_status": current.status.to_string()}),
    )
    .await?;

    if let Some(bus) = bus {
        let prefix = key_prefix.unwrap_or("gs");
        let topic = format!("{prefix}:bus:global");
        let event_name = if enabled {
            "TenantEnabled"
        } else {
            "TenantDisabled"
        };
        let payload = serde_json::to_vec(&serde_json::json!({
            "event": event_name,
            "tenant_id": tenant_id.to_string(),
            "at": now.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
        }))
        .unwrap_or_default();
        let _ = bus.publish(&topic, &payload).await;
    }

    get_tenant(pool, &tenant_id.to_string()).await
}

/// Enables or disables a tenant, auditing the transition (docs/16 §3).
pub async fn set_tenant_enabled(
    pool: &PgPool,
    tenant_id: &str,
    enabled: bool,
) -> Result<Tenant, AdminError> {
    set_tenant_enabled_with_bus::<InMemoryBus>(pool, tenant_id, enabled, None, None).await
}

/// Creates an application inside a tenant and audits `application.created` (docs/16 §3).
pub async fn create_application(
    pool: &PgPool,
    tenant_id: &str,
    name: &str,
    device_scopes: Option<Vec<String>>,
    allowed_origins: Option<Vec<String>>,
) -> Result<Application, AdminError> {
    let tenant = get_tenant(pool, tenant_id).await?;

    let name = name.trim();
    if name.is_empty() || name.len() > NAME_MAX_LEN {
        return Err(AdminError::InvalidApplicationName);
    }

    let scopes = match device_scopes {
        Some(scopes) => scopes,
        None => DEFAULT_APPLICATION_DEVICE_SCOPES
            .iter()
            .map(|scope| (*scope).to_string())
            .collect(),
    };
    gs_auth::scopes::validate_device_scopes(&scopes)?;

    let origins = allowed_origins.unwrap_or_default();
    let now = OffsetDateTime::now_utc();
    let application = Application {
        id: ApplicationId::new(),
        tenant_id: tenant.id,
        name: name.to_string(),
        status: ApplicationStatus::Active,
        allowed_device_scopes: scopes.clone(),
        allowed_origins: origins.clone(),
        created_at: now,
        updated_at: now,
    };

    ApplicationRepository::create(pool, &application).await?;
    record_audit(
        pool,
        tenant.id,
        "application.created",
        "application",
        application.id.into_uuid(),
        serde_json::json!({
            "name": application.name,
            "allowed_device_scopes": scopes,
            "allowed_origins": origins,
        }),
    )
    .await?;

    Ok(application)
}

/// Lists the applications of a tenant (docs/16 §3).
pub async fn list_applications(
    pool: &PgPool,
    tenant_id: &str,
) -> Result<Vec<Application>, AdminError> {
    let tenant = get_tenant(pool, tenant_id).await?;
    Ok(ApplicationRepository::list_by_tenant(pool, tenant.id).await?)
}

/// Enables or disables an application with optional bus invalidation (docs/16 §3, FR-ADM-003).
///
/// The owning tenant is derived from the application itself, as `docs/16` §3 documents
/// `app disable --app <app_…>` without `--tenant`. The write stays tenant-scoped.
pub async fn set_application_enabled_with_bus<B: Bus>(
    pool: &PgPool,
    app_id: &str,
    enabled: bool,
    bus: Option<&B>,
    key_prefix: Option<&str>,
) -> Result<Application, AdminError> {
    let app_id = parse_application_id(app_id)?;
    let application = ApplicationRepository::find_by_id(pool, app_id)
        .await?
        .ok_or_else(|| AdminError::ApplicationNotFound {
            tenant_id: "unknown".to_string(),
            app_id: app_id.to_string(),
        })?;

    let desired = if enabled {
        ApplicationStatus::Active
    } else {
        ApplicationStatus::Disabled
    };
    if application.status != desired {
        let now = OffsetDateTime::now_utc();
        ApplicationRepository::update_status(pool, application.tenant_id, app_id, desired, now)
            .await?;

        let action = if enabled {
            "application.enabled"
        } else {
            "application.disabled"
        };
        record_audit(
            pool,
            application.tenant_id,
            action,
            "application",
            app_id.into_uuid(),
            serde_json::json!({"previous_status": application.status.to_string()}),
        )
        .await?;

        if let Some(bus) = bus {
            let prefix = key_prefix.unwrap_or("gs");
            let topic = format!("{prefix}:bus:global");
            let event_name = if enabled {
                "ApplicationEnabled"
            } else {
                "ApplicationDisabled"
            };
            let payload = serde_json::to_vec(&serde_json::json!({
                "event": event_name,
                "tenant_id": application.tenant_id.to_string(),
                "application_id": app_id.to_string(),
                "at": now.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
            }))
            .unwrap_or_default();
            let _ = bus.publish(&topic, &payload).await;
        }
    }

    ApplicationRepository::find_by_id(pool, app_id)
        .await?
        .ok_or_else(|| AdminError::ApplicationNotFound {
            tenant_id: application.tenant_id.to_string(),
            app_id: app_id.to_string(),
        })
}

/// Enables or disables an application, auditing the transition (docs/16 §3).
///
/// The owning tenant is derived from the application itself, as `docs/16` §3 documents
/// `app disable --app <app_…>` without `--tenant`. The write stays tenant-scoped.
pub async fn set_application_enabled(
    pool: &PgPool,
    app_id: &str,
    enabled: bool,
) -> Result<Application, AdminError> {
    set_application_enabled_with_bus::<InMemoryBus>(pool, app_id, enabled, None, None).await
}

/// Creates an api key for an application and audits `api_key.created` (docs/16 §3).
///
/// The owning tenant is derived from the application, as `docs/16` §3 documents
/// `key create --app <app_…>` without `--tenant`. The write stays tenant-scoped.
///
/// The returned credential is the only moment the secret exists: it is never stored,
/// logged nor returned again.
pub async fn create_api_key(
    pool: &PgPool,
    app_id: &str,
    scopes: Option<Vec<String>>,
    label: Option<&str>,
    expires_in: Option<Duration>,
) -> Result<CreatedApiKey, AdminError> {
    let app_id = parse_application_id(app_id)?;
    let application = ApplicationRepository::find_by_id(pool, app_id)
        .await?
        .ok_or_else(|| AdminError::ApplicationNotFound {
            tenant_id: "unknown".to_string(),
            app_id: app_id.to_string(),
        })?;
    let tenant_id = application.tenant_id;

    let label = match label {
        Some(label) => {
            let trimmed = label.trim();
            if trimmed.len() > NAME_MAX_LEN {
                return Err(AdminError::InvalidApiKeyLabel);
            }
            Some(trimmed.to_string()).filter(|label| !label.is_empty())
        }
        None => None,
    };

    let scopes = match scopes {
        Some(scopes) => scopes,
        None => gs_auth::scopes::default_api_key_scopes(),
    };
    gs_auth::scopes::validate_scopes(&scopes)?;

    let now = OffsetDateTime::now_utc();
    let expires_at = expires_in.map(|duration| now + duration);

    let key_id = ApiKeyId::new();
    let secret = generate_secret();
    let key = ApiKey {
        id: key_id,
        tenant_id,
        application_id: application.id,
        secret_sha256: hash_secret(&secret),
        scopes: scopes.clone(),
        label: label.clone(),
        status: ApiKeyStatus::Active,
        expires_at,
        last_used_at: None,
        created_at: now,
        revoked_at: None,
    };

    ApiKeyRepository::create(pool, &key).await?;
    record_audit(
        pool,
        tenant_id,
        "api_key.created",
        "api_key",
        key.id.into_uuid(),
        serde_json::json!({
            "application_id": application.id.to_string(),
            "scopes": scopes,
            "label": label,
            "expires_at": expires_at.map(format_rfc3339),
        }),
    )
    .await?;

    Ok(CreatedApiKey {
        key,
        credential: Zeroizing::new(format_credential(key_id, &secret)),
    })
}

/// Lists the api keys of an application. Never includes secrets (docs/16 §3).
pub async fn list_api_keys(pool: &PgPool, app_id: &str) -> Result<Vec<ApiKey>, AdminError> {
    let app_id = parse_application_id(app_id)?;
    let application = ApplicationRepository::find_by_id(pool, app_id)
        .await?
        .ok_or_else(|| AdminError::ApplicationNotFound {
            tenant_id: "unknown".to_string(),
            app_id: app_id.to_string(),
        })?;

    Ok(ApiKeyRepository::list_by_application(pool, application.tenant_id, application.id).await?)
}

/// Revokes an api key with optional bus invalidation (docs/16 §3, docs/06 §3.3).
///
/// The owning tenant is derived from the key itself, as `docs/16` §3 documents
/// `key revoke --key <key_…>` without `--tenant`. The write stays tenant-scoped.
pub async fn revoke_api_key_with_bus<B: Bus>(
    pool: &PgPool,
    key_id: &str,
    bus: Option<&B>,
    key_prefix: Option<&str>,
) -> Result<ApiKey, AdminError> {
    let key_id = parse_api_key_id(key_id)?;
    let current = ApiKeyRepository::find_by_id(pool, key_id)
        .await?
        .ok_or_else(|| AdminError::ApiKeyNotFound {
            tenant_id: "unknown".to_string(),
            key_id: key_id.to_string(),
        })?;
    let tenant_id = current.tenant_id;

    if current.status != ApiKeyStatus::Revoked {
        let now = OffsetDateTime::now_utc();
        let revoked = ApiKeyRepository::revoke(pool, tenant_id, key_id, now).await?;
        if !revoked {
            return Err(AdminError::ApiKeyNotFound {
                tenant_id: tenant_id.to_string(),
                key_id: key_id.to_string(),
            });
        }
        record_audit(
            pool,
            tenant_id,
            "api_key.revoked",
            "api_key",
            key_id.into_uuid(),
            serde_json::json!({"label": current.label}),
        )
        .await?;

        if let Some(bus) = bus {
            let prefix = key_prefix.unwrap_or("gs");
            let topic = format!("{prefix}:bus:global");
            let payload = serde_json::to_vec(&serde_json::json!({
                "event": "ApiKeyRevoked",
                "tenant_id": tenant_id.to_string(),
                "api_key_id": key_id.to_string(),
                "at": now.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
            }))
            .unwrap_or_default();
            let _ = bus.publish(&topic, &payload).await;
        }
    }

    ApiKeyRepository::find_by_id(pool, key_id)
        .await?
        .ok_or_else(|| AdminError::ApiKeyNotFound {
            tenant_id: tenant_id.to_string(),
            key_id: key_id.to_string(),
        })
}

/// Revokes an api key and audits `api_key.revoked`. Revocation is terminal (docs/06 §3.3).
///
/// The owning tenant is derived from the key itself, as `docs/16` §3 documents
/// `key revoke --key <key_…>` without `--tenant`. The write stays tenant-scoped.
pub async fn revoke_api_key(pool: &PgPool, key_id: &str) -> Result<ApiKey, AdminError> {
    revoke_api_key_with_bus::<InMemoryBus>(pool, key_id, None, None).await
}

/// Reads the effective policy of a tenant: stored overrides over the defaults (docs/17 §4).
pub async fn get_effective_policy(
    pool: &PgPool,
    tenant_id: &str,
) -> Result<TenantPolicy, AdminError> {
    let tenant = get_tenant(pool, tenant_id).await?;
    Ok(TenantPolicy::from_json_value(tenant.policy)?)
}

/// Stores policy overrides of a tenant, validating them against docs/17 §4.
///
/// Each assignment is `clave=valor`. Unknown keys, wrong types and out-of-range values fail
/// with an explicit message; keys that are absent keep their default (FR-ADM-002).
pub async fn set_policy_values(
    pool: &PgPool,
    tenant_id: &str,
    assignments: &[String],
) -> Result<TenantPolicy, AdminError> {
    let tenant = get_tenant(pool, tenant_id).await?;

    let overrides = parse_policy_assignments(&tenant.policy, assignments)?;
    let policy = TenantPolicy::from_json_value(serde_json::Value::Object(overrides.clone()))?;

    TenantRepository::update_policy(
        pool,
        tenant.id,
        &serde_json::Value::Object(overrides),
        OffsetDateTime::now_utc(),
    )
    .await?;

    Ok(policy)
}

/// Generates an Ed25519 signing key pair on disk (docs/06 §4.4).
pub fn generate_signing_key_pair(
    kid: &str,
    out_dir: &std::path::Path,
) -> Result<GeneratedSigningKeyFiles, AdminError> {
    let generated = signing_key::generate(kid)?;
    let (private_key_path, public_key_path) = generated.write_pair(out_dir)?;
    Ok(GeneratedSigningKeyFiles {
        kid: generated.kid,
        private_key_path,
        public_key_path,
    })
}

/// Parses `clave=valor` assignments over the stored overrides (docs/17 §4).
fn parse_policy_assignments(
    stored: &serde_json::Value,
    assignments: &[String],
) -> Result<serde_json::Map<String, serde_json::Value>, PolicyError> {
    let mut overrides = match stored {
        serde_json::Value::Object(map) => map.clone(),
        _ => serde_json::Map::new(),
    };

    for assignment in assignments {
        let (key, raw_value) = assignment
            .split_once('=')
            .ok_or_else(|| PolicyError::UnknownKey(assignment.clone()))?;
        let key = key.trim();
        if key.is_empty() {
            return Err(PolicyError::UnknownKey(assignment.clone()));
        }
        let value: serde_json::Value =
            serde_json::from_str(raw_value.trim()).map_err(|e| PolicyError::InvalidType {
                key: key.to_string(),
                message: format!("expected a JSON value (number, boolean or string): {e}"),
            })?;
        overrides.insert(key.to_string(), value);
    }

    Ok(overrides)
}

/// Formats an instant as RFC 3339 (docs/00 §4.5).
fn format_rfc3339(instant: OffsetDateTime) -> String {
    instant
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| instant.to_string())
}

/// Parses a tenant id argument of the CLI.
fn parse_tenant_id(value: &str) -> Result<TenantId, AdminError> {
    TenantId::from_str(value).map_err(|source| AdminError::InvalidTenantId {
        value: value.to_string(),
        source,
    })
}

/// Parses an application id argument of the CLI.
fn parse_application_id(value: &str) -> Result<ApplicationId, AdminError> {
    ApplicationId::from_str(value).map_err(|source| AdminError::InvalidApplicationId {
        value: value.to_string(),
        source,
    })
}

/// Parses an api key id argument of the CLI.
fn parse_api_key_id(value: &str) -> Result<ApiKeyId, AdminError> {
    ApiKeyId::from_str(value).map_err(|source| AdminError::InvalidApiKeyId {
        value: value.to_string(),
        source,
    })
}

/// Records an audit event with `actor_kind = OPERATOR` (docs/10 §8.3).
async fn record_audit(
    pool: &PgPool,
    tenant_id: TenantId,
    action: &str,
    target_kind: &str,
    target_id: Uuid,
    details: serde_json::Value,
) -> Result<(), AdminError> {
    let event = AuditEvent {
        id: AuditId::new(),
        tenant_id,
        occurred_at: OffsetDateTime::now_utc(),
        actor_kind: ActorKind::Operator,
        actor_id: None,
        subject_id: None,
        action: action.to_string(),
        target_kind: Some(target_kind.to_string()),
        target_id: Some(target_id),
        outcome: AuditOutcome::Success,
        request_id: None,
        details,
    };
    AuditRepository::record(pool, &event).await?;
    Ok(())
}
