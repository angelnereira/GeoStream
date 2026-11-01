//! Organizations, projects and API keys managed from the console (ADR-0033 §3, ADR-0036,
//! FR-CON-002 to FR-CON-006).
//!
//! An organization is a tenant and a project is an application. Every function takes the
//! account that acts and checks its role in the organization first: an organization the
//! account does not belong to answers `NotFound`, the same as one that does not exist, so
//! the existence of other people's organizations is not observable (docs/06 §8.6).
#![forbid(unsafe_code)]

use std::str::FromStr;

use thiserror::Error;
use time::{Duration, OffsetDateTime};
use tracing::error;

use gs_bus::Bus;
use gs_domain::error::{AsErrorCode, ErrorCode};
use gs_domain::ids::{AccountId, ApiKeyId, ApplicationId, TenantId};
use gs_store::{
    ApiKey, Application, ApplicationRepository, ApplicationStatus, ConsoleRepository, ConsoleRole,
    OrganizationMembership, PgPool, Tenant, TenantRepository, TenantStatus,
};

use crate::admin::{self, AdminError, CreatedApiKey, NAME_MAX_LEN};

/// Longest organization or project name accepted from the console (contract `maxLength`).
pub const CONSOLE_NAME_MAX_LEN: usize = 80;
/// Most origins one project may list (contract `maxItems`).
pub const MAX_ORIGINS: usize = 20;
/// Longest origin (contract `maxLength`).
pub const ORIGIN_MAX_LEN: usize = 200;

/// Failures of the workspace operations, each mapped to one canonical error code.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorkspaceError {
    /// The organization, project or key does not exist for this account.
    #[error("not found")]
    NotFound,
    /// The account belongs to the organization but its role is not enough.
    #[error("permission denied")]
    PermissionDenied,
    /// The input breaks the contract. The message names the field, never a value.
    #[error("validation failed: {0}")]
    Validation(&'static str),
    /// The account already owns the maximum number of organizations.
    #[error("organization limit reached")]
    LimitReached,
    /// A dependency failed. Carries no detail: it could echo connection data.
    #[error("internal error")]
    Internal,
}

impl AsErrorCode for WorkspaceError {
    fn error_code(&self) -> ErrorCode {
        match self {
            Self::NotFound => ErrorCode::NotFound,
            Self::PermissionDenied => ErrorCode::PermissionDenied,
            Self::Validation(_) => ErrorCode::ValidationFailed,
            Self::LimitReached => ErrorCode::RateLimited,
            Self::Internal => ErrorCode::Internal,
        }
    }
}

impl From<gs_store::StoreError> for WorkspaceError {
    fn from(_: gs_store::StoreError) -> Self {
        error!(kind = "store", "console workspace store operation failed");
        Self::Internal
    }
}

impl From<AdminError> for WorkspaceError {
    fn from(error: AdminError) -> Self {
        match error {
            AdminError::InvalidApplicationName | AdminError::InvalidTenantName => {
                Self::Validation("name")
            }
            AdminError::InvalidApiKeyLabel => Self::Validation("label"),
            AdminError::InvalidScopes(_) => Self::Validation("scopes"),
            AdminError::ApplicationNotFound { .. }
            | AdminError::ApiKeyNotFound { .. }
            | AdminError::TenantNotFound { .. } => Self::NotFound,
            _ => {
                error!(kind = "admin", "console workspace admin operation failed");
                Self::Internal
            }
        }
    }
}

/// Fields a project update may change. `None` leaves the field as it is.
#[derive(Debug, Default, Clone)]
pub struct ProjectUpdate {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub allowed_device_scopes: Option<Vec<String>>,
    pub allowed_origins: Option<Vec<String>>,
}

/// Role check shared by every operation. `NotFound` when the account is not a member.
pub async fn require_role(
    pool: &PgPool,
    account_id: AccountId,
    organization_id: TenantId,
    minimum: ConsoleRole,
) -> Result<ConsoleRole, WorkspaceError> {
    let role = ConsoleRepository::get_role(pool, account_id, organization_id)
        .await?
        .ok_or(WorkspaceError::NotFound)?;
    if role.at_least(minimum) {
        Ok(role)
    } else {
        Err(WorkspaceError::PermissionDenied)
    }
}

fn clean_name(raw: &str) -> Result<String, WorkspaceError> {
    let name = raw.trim();
    if name.is_empty()
        || name.chars().count() > CONSOLE_NAME_MAX_LEN
        || name.len() > NAME_MAX_LEN
        || name.chars().any(char::is_control)
    {
        return Err(WorkspaceError::Validation("name"));
    }
    Ok(name.to_string())
}

/// An origin is `http(s)://host[:port]` with no path, query, fragment or credentials.
fn validate_origins(origins: &[String]) -> Result<(), WorkspaceError> {
    if origins.len() > MAX_ORIGINS {
        return Err(WorkspaceError::Validation("allowed_origins"));
    }
    for origin in origins {
        let rest = origin
            .strip_prefix("https://")
            .or_else(|| origin.strip_prefix("http://"))
            .ok_or(WorkspaceError::Validation("allowed_origins"))?;
        let valid = origin.len() <= ORIGIN_MAX_LEN
            && !rest.is_empty()
            && rest
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
            && !rest.starts_with(['.', '-', ':']);
        if !valid {
            return Err(WorkspaceError::Validation("allowed_origins"));
        }
    }
    let mut sorted: Vec<&String> = origins.iter().collect();
    sorted.sort();
    sorted.dedup();
    if sorted.len() != origins.len() {
        return Err(WorkspaceError::Validation("allowed_origins"));
    }
    Ok(())
}

/// Organizations of the account.
pub async fn list_organizations(
    pool: &PgPool,
    account_id: AccountId,
) -> Result<Vec<OrganizationMembership>, WorkspaceError> {
    Ok(ConsoleRepository::list_organizations(pool, account_id).await?)
}

/// One organization of the account.
pub async fn get_organization(
    pool: &PgPool,
    account_id: AccountId,
    organization_id: TenantId,
) -> Result<OrganizationMembership, WorkspaceError> {
    let role = require_role(pool, account_id, organization_id, ConsoleRole::Viewer).await?;
    let tenant = TenantRepository::get_by_id(pool, organization_id)
        .await?
        .ok_or(WorkspaceError::NotFound)?;
    Ok(OrganizationMembership { tenant, role })
}

/// Creates an organization owned by the account, up to `max_owned` of them.
pub async fn create_organization(
    pool: &PgPool,
    account_id: AccountId,
    name: &str,
    max_owned: i64,
) -> Result<OrganizationMembership, WorkspaceError> {
    let name = clean_name(name)?;
    let now = OffsetDateTime::now_utc();
    let tenant = Tenant {
        id: TenantId::new(),
        name,
        status: TenantStatus::Active,
        policy: serde_json::json!({}),
        created_at: now,
        updated_at: now,
    };
    if !ConsoleRepository::create_organization(pool, account_id, &tenant, max_owned).await? {
        return Err(WorkspaceError::LimitReached);
    }
    Ok(OrganizationMembership {
        tenant,
        role: ConsoleRole::Owner,
    })
}

/// Projects of an organization.
pub async fn list_projects(
    pool: &PgPool,
    account_id: AccountId,
    organization_id: TenantId,
) -> Result<Vec<Application>, WorkspaceError> {
    require_role(pool, account_id, organization_id, ConsoleRole::Viewer).await?;
    Ok(ApplicationRepository::list_by_tenant(pool, organization_id).await?)
}

/// A project that belongs to the organization, or `NotFound`.
async fn project_of(
    pool: &PgPool,
    organization_id: TenantId,
    project_id: ApplicationId,
) -> Result<Application, WorkspaceError> {
    ApplicationRepository::get_by_id(pool, organization_id, project_id)
        .await?
        .ok_or(WorkspaceError::NotFound)
}

/// One project.
pub async fn get_project(
    pool: &PgPool,
    account_id: AccountId,
    organization_id: TenantId,
    project_id: ApplicationId,
) -> Result<Application, WorkspaceError> {
    require_role(pool, account_id, organization_id, ConsoleRole::Viewer).await?;
    project_of(pool, organization_id, project_id).await
}

/// Creates a project (an application of the tenant). Needs `ADMIN`.
pub async fn create_project(
    pool: &PgPool,
    account_id: AccountId,
    organization_id: TenantId,
    name: &str,
    device_scopes: Option<Vec<String>>,
    origins: Option<Vec<String>>,
) -> Result<Application, WorkspaceError> {
    require_role(pool, account_id, organization_id, ConsoleRole::Admin).await?;
    let name = clean_name(name)?;
    if let Some(origins) = &origins {
        validate_origins(origins)?;
    }
    Ok(admin::create_application(
        pool,
        &organization_id.to_string(),
        &name,
        device_scopes,
        origins,
    )
    .await?)
}

/// Updates a project. Disabling it invalidates its credentials through the bus (FR-ADM-003).
pub async fn update_project<B: Bus>(
    pool: &PgPool,
    account_id: AccountId,
    organization_id: TenantId,
    project_id: ApplicationId,
    update: ProjectUpdate,
    bus: Option<&B>,
    key_prefix: Option<&str>,
) -> Result<Application, WorkspaceError> {
    require_role(pool, account_id, organization_id, ConsoleRole::Admin).await?;
    let current = project_of(pool, organization_id, project_id).await?;
    let now = OffsetDateTime::now_utc();

    // Validate everything before the first write: a rejected request changes nothing.
    let name = update.name.as_deref().map(clean_name).transpose()?;
    if let Some(scopes) = &update.allowed_device_scopes {
        gs_auth::scopes::validate_device_scopes(scopes)
            .map_err(|_| WorkspaceError::Validation("allowed_device_scopes"))?;
    }
    if let Some(origins) = &update.allowed_origins {
        validate_origins(origins)?;
    }

    if let Some(name) = name {
        ApplicationRepository::update_name(pool, organization_id, project_id, &name, now).await?;
    }
    if update.allowed_device_scopes.is_some() || update.allowed_origins.is_some() {
        let scopes = update
            .allowed_device_scopes
            .unwrap_or(current.allowed_device_scopes);
        let origins = update.allowed_origins.unwrap_or(current.allowed_origins);
        ApplicationRepository::update_scopes_and_origins(
            pool,
            organization_id,
            project_id,
            &scopes,
            &origins,
            now,
        )
        .await?;
    }
    if let Some(enabled) = update.enabled {
        admin::set_application_enabled_with_bus(
            pool,
            &project_id.to_string(),
            enabled,
            bus,
            key_prefix,
        )
        .await?;
    }
    project_of(pool, organization_id, project_id).await
}

/// API keys of a project, without secrets.
pub async fn list_api_keys(
    pool: &PgPool,
    account_id: AccountId,
    organization_id: TenantId,
    project_id: ApplicationId,
) -> Result<Vec<ApiKey>, WorkspaceError> {
    require_role(pool, account_id, organization_id, ConsoleRole::Viewer).await?;
    project_of(pool, organization_id, project_id).await?;
    Ok(admin::list_api_keys(pool, &project_id.to_string()).await?)
}

/// Creates an API key. The secret is in the returned value and nowhere else. Needs `ADMIN`.
pub async fn create_api_key(
    pool: &PgPool,
    account_id: AccountId,
    organization_id: TenantId,
    project_id: ApplicationId,
    label: &str,
    scopes: Vec<String>,
    expires_in_days: Option<u16>,
) -> Result<CreatedApiKey, WorkspaceError> {
    require_role(pool, account_id, organization_id, ConsoleRole::Admin).await?;
    let project = project_of(pool, organization_id, project_id).await?;
    if project.status != ApplicationStatus::Active {
        return Err(WorkspaceError::Validation("project"));
    }
    let label = label.trim();
    if label.is_empty() || label.chars().count() > 64 || label.chars().any(char::is_control) {
        return Err(WorkspaceError::Validation("label"));
    }
    if scopes.is_empty() {
        return Err(WorkspaceError::Validation("scopes"));
    }
    if expires_in_days.is_some_and(|days| !(1..=365).contains(&days)) {
        return Err(WorkspaceError::Validation("expires_in_days"));
    }
    Ok(admin::create_api_key(
        pool,
        &project_id.to_string(),
        Some(scopes),
        Some(label),
        expires_in_days.map(|days| Duration::days(i64::from(days))),
    )
    .await?)
}

/// Revokes an API key of the project. Needs `ADMIN`. Revoking twice is not an error.
pub async fn revoke_api_key<B: Bus>(
    pool: &PgPool,
    account_id: AccountId,
    organization_id: TenantId,
    project_id: ApplicationId,
    key_id: ApiKeyId,
    bus: Option<&B>,
    key_prefix: Option<&str>,
) -> Result<(), WorkspaceError> {
    require_role(pool, account_id, organization_id, ConsoleRole::Admin).await?;
    project_of(pool, organization_id, project_id).await?;
    // The key must belong to this very project: another project's key is `NotFound`.
    let key = admin::list_api_keys(pool, &project_id.to_string())
        .await?
        .into_iter()
        .find(|key| key.id == key_id)
        .ok_or(WorkspaceError::NotFound)?;
    admin::revoke_api_key_with_bus(pool, &key.id.to_string(), bus, key_prefix).await?;
    Ok(())
}

/// Parses a path id, answering `NotFound` for anything malformed (no oracle on shapes).
pub fn parse_id<T: FromStr>(raw: &str) -> Result<T, WorkspaceError> {
    raw.parse().map_err(|_| WorkspaceError::NotFound)
}
