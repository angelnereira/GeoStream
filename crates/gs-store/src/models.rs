//! Storage entity models and enums matching docs/11 §2 and docs/05 §3.
#![forbid(unsafe_code)]

use std::fmt;
use std::str::FromStr;
use time::OffsetDateTime;
use uuid::Uuid;

use gs_domain::ids::{
    AccountId, ApiKeyId, ApplicationId, AuditId, DeviceId, InstallationId, RequestId, SubjectId,
    TenantId,
};

/// Status of a tenant (docs/05 §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TenantStatus {
    #[serde(rename = "ACTIVE")]
    Active,
    #[serde(rename = "DISABLED")]
    Disabled,
}

impl TenantStatus {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Disabled => "DISABLED",
        }
    }
}

impl fmt::Display for TenantStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for TenantStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ACTIVE" => Ok(Self::Active),
            "DISABLED" => Ok(Self::Disabled),
            _ => Err(()),
        }
    }
}

/// Tenant entity record (tenants table).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Tenant {
    pub id: TenantId,
    pub name: String,
    pub status: TenantStatus,
    pub policy: serde_json::Value,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

/// Status of an application (docs/05 §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ApplicationStatus {
    #[serde(rename = "ACTIVE")]
    Active,
    #[serde(rename = "DISABLED")]
    Disabled,
}

impl ApplicationStatus {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Disabled => "DISABLED",
        }
    }
}

impl fmt::Display for ApplicationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for ApplicationStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ACTIVE" => Ok(Self::Active),
            "DISABLED" => Ok(Self::Disabled),
            _ => Err(()),
        }
    }
}

/// Application entity record (applications table).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Application {
    pub id: ApplicationId,
    pub tenant_id: TenantId,
    pub name: String,
    pub status: ApplicationStatus,
    pub allowed_device_scopes: Vec<String>,
    pub allowed_origins: Vec<String>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

/// Status of an API key (docs/06 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ApiKeyStatus {
    #[serde(rename = "ACTIVE")]
    Active,
    #[serde(rename = "REVOKED")]
    Revoked,
}

impl ApiKeyStatus {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Revoked => "REVOKED",
        }
    }
}

impl fmt::Display for ApiKeyStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for ApiKeyStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ACTIVE" => Ok(Self::Active),
            "REVOKED" => Ok(Self::Revoked),
            _ => Err(()),
        }
    }
}

/// API Key entity record (api_keys table). Secret hash is redacted in Debug.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ApiKey {
    pub id: ApiKeyId,
    pub tenant_id: TenantId,
    pub application_id: ApplicationId,
    pub secret_sha256: [u8; 32],
    pub scopes: Vec<String>,
    pub label: Option<String>,
    pub status: ApiKeyStatus,
    pub expires_at: Option<OffsetDateTime>,
    pub last_used_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub revoked_at: Option<OffsetDateTime>,
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiKey")
            .field("id", &self.id)
            .field("tenant_id", &self.tenant_id)
            .field("application_id", &self.application_id)
            .field("secret_sha256", &"<redacted>")
            .field("scopes", &self.scopes)
            .field("label", &self.label)
            .field("status", &self.status)
            .field("expires_at", &self.expires_at)
            .field("last_used_at", &self.last_used_at)
            .field("created_at", &self.created_at)
            .field("revoked_at", &self.revoked_at)
            .finish()
    }
}

/// Status of a subject (docs/05 §3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SubjectStatus {
    #[serde(rename = "ACTIVE")]
    Active,
    #[serde(rename = "ERASED")]
    Erased,
}

impl SubjectStatus {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Erased => "ERASED",
        }
    }
}

impl fmt::Display for SubjectStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for SubjectStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ACTIVE" => Ok(Self::Active),
            "ERASED" => Ok(Self::Erased),
            _ => Err(()),
        }
    }
}

/// Subject entity record (subjects table).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Subject {
    pub id: SubjectId,
    pub tenant_id: TenantId,
    pub external_subject_id: Option<String>,
    pub status: SubjectStatus,
    pub token_epoch: u32,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub erased_at: Option<OffsetDateTime>,
}

/// Device operating platform (devices table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DevicePlatform {
    #[serde(rename = "ANDROID")]
    Android,
    #[serde(rename = "IOS")]
    Ios,
    #[serde(rename = "WEB")]
    Web,
    #[serde(rename = "SERVER")]
    Server,
    #[serde(rename = "OTHER")]
    Other,
}

impl DevicePlatform {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Android => "ANDROID",
            Self::Ios => "IOS",
            Self::Web => "WEB",
            Self::Server => "SERVER",
            Self::Other => "OTHER",
        }
    }
}

impl fmt::Display for DevicePlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for DevicePlatform {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ANDROID" => Ok(Self::Android),
            "IOS" => Ok(Self::Ios),
            "WEB" => Ok(Self::Web),
            "SERVER" => Ok(Self::Server),
            "OTHER" => Ok(Self::Other),
            _ => Err(()),
        }
    }
}

/// Status of a device (docs/05 §3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DeviceStatus {
    #[serde(rename = "ACTIVE")]
    Active,
    #[serde(rename = "INACTIVE")]
    Inactive,
}

impl DeviceStatus {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Inactive => "INACTIVE",
        }
    }
}

impl fmt::Display for DeviceStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for DeviceStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ACTIVE" => Ok(Self::Active),
            "INACTIVE" => Ok(Self::Inactive),
            _ => Err(()),
        }
    }
}

/// Device entity record (devices table).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Device {
    pub id: DeviceId,
    pub tenant_id: TenantId,
    pub subject_id: SubjectId,
    pub application_id: ApplicationId,
    pub installation_id: InstallationId,
    pub platform: DevicePlatform,
    pub app_version: Option<String>,
    pub sdk_version: Option<String>,
    pub status: DeviceStatus,
    pub created_at: OffsetDateTime,
    pub last_seen_at: OffsetDateTime,
    pub deactivated_at: Option<OffsetDateTime>,
}

/// Actor kind for audit records (docs/11 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ActorKind {
    #[serde(rename = "DEVICE")]
    Device,
    #[serde(rename = "APPLICATION")]
    Application,
    #[serde(rename = "OPERATOR")]
    Operator,
    #[serde(rename = "SYSTEM")]
    System,
}

impl ActorKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Device => "DEVICE",
            Self::Application => "APPLICATION",
            Self::Operator => "OPERATOR",
            Self::System => "SYSTEM",
        }
    }
}

impl fmt::Display for ActorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for ActorKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "DEVICE" => Ok(Self::Device),
            "APPLICATION" => Ok(Self::Application),
            "OPERATOR" => Ok(Self::Operator),
            "SYSTEM" => Ok(Self::System),
            _ => Err(()),
        }
    }
}

/// Audit event outcome (docs/11 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AuditOutcome {
    #[serde(rename = "SUCCESS")]
    Success,
    #[serde(rename = "DENIED")]
    Denied,
    #[serde(rename = "FAILED")]
    Failed,
}

impl AuditOutcome {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "SUCCESS",
            Self::Denied => "DENIED",
            Self::Failed => "FAILED",
        }
    }
}

impl fmt::Display for AuditOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for AuditOutcome {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "SUCCESS" => Ok(Self::Success),
            "DENIED" => Ok(Self::Denied),
            "FAILED" => Ok(Self::Failed),
            _ => Err(()),
        }
    }
}

/// Audit event record (audit_events table).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AuditEvent {
    pub id: AuditId,
    pub tenant_id: TenantId,
    pub occurred_at: OffsetDateTime,
    pub actor_kind: ActorKind,
    pub actor_id: Option<Uuid>,
    pub subject_id: Option<SubjectId>,
    pub action: String,
    pub target_kind: Option<String>,
    pub target_id: Option<Uuid>,
    pub outcome: AuditOutcome,
    pub request_id: Option<RequestId>,
    pub details: serde_json::Value,
}

/// Status of a console account (docs/11 §2.0.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AccountStatus {
    #[serde(rename = "ACTIVE")]
    Active,
    #[serde(rename = "DISABLED")]
    Disabled,
}

impl AccountStatus {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Disabled => "DISABLED",
        }
    }
}

impl FromStr for AccountStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ACTIVE" => Ok(Self::Active),
            "DISABLED" => Ok(Self::Disabled),
            other => Err(format!("unknown account status '{other}'")),
        }
    }
}

/// Role of a console account inside an organization (docs/06 §8.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ConsoleRole {
    #[serde(rename = "VIEWER")]
    Viewer,
    #[serde(rename = "ADMIN")]
    Admin,
    #[serde(rename = "OWNER")]
    Owner,
}

impl ConsoleRole {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Viewer => "VIEWER",
            Self::Admin => "ADMIN",
            Self::Owner => "OWNER",
        }
    }

    /// Whether this role satisfies a minimum role (`OWNER` ⊃ `ADMIN` ⊃ `VIEWER`).
    pub const fn at_least(&self, minimum: ConsoleRole) -> bool {
        self.rank() >= minimum.rank()
    }

    const fn rank(&self) -> u8 {
        match self {
            Self::Viewer => 0,
            Self::Admin => 1,
            Self::Owner => 2,
        }
    }
}

impl FromStr for ConsoleRole {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "VIEWER" => Ok(Self::Viewer),
            "ADMIN" => Ok(Self::Admin),
            "OWNER" => Ok(Self::Owner),
            other => Err(format!("unknown console role '{other}'")),
        }
    }
}

/// Console account (`console_accounts`). Personal data: never log `email` (docs/10 §8.2).
#[derive(Clone, PartialEq, Eq)]
pub struct ConsoleAccount {
    pub id: AccountId,
    pub email: String,
    pub status: AccountStatus,
    pub terms_accepted_at: OffsetDateTime,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl fmt::Debug for ConsoleAccount {
    // The e-mail is personal data and must not reach logs through `{:?}` (docs/10 §8.2).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConsoleAccount")
            .field("id", &self.id)
            .field("email", &"<redacted>")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}
