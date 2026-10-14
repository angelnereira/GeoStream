//! Authorization cache with bounded TTL, bus invalidation and principal revalidation
//! (ADR-0009, docs/04 §6.2, docs/06 §7, NFR-SEC-002).
//!
//! The cache exists so that a delivery decision does not cost a PostgreSQL query, while a
//! revocation cannot outlive `GS_AUTHZ_CACHE_TTL_MS`. Three mechanisms keep it bounded:
//!
//! 1. **TTL**: an entry older than the TTL is revalidated against `mver`
//!    (`{p}:mver:{session_id}`); if the version did not change the entry is renewed, and if it
//!    changed the memberships are reloaded from PostgreSQL (docs/04 §6.2).
//! 2. **Bus**: a control event of the session invalidates its entries immediately, and the
//!    global events of `docs/04` §6.1 invalidate the affected principals and their decisions.
//! 3. **Reconnect**: when the bus reconnects the whole cache is flushed and every decision is
//!    revalidated before the next delivery.
//!
//! The principal of a connection is revalidated by the same TTL, so a revoked token, a
//! deactivated device or a disabled tenant stops working within `GS_AUTHZ_CACHE_TTL_MS` even
//! if a bus message is lost (docs/06 §7, NFR-SEC-002).
#![forbid(unsafe_code)]

use std::time::{Duration, Instant};

use dashmap::DashMap;
use gs_domain::error::{AsErrorCode, ErrorCode};
use gs_domain::ids::{ApplicationId, DeviceId, MemberId, SessionId, SubjectId, TenantId};
use gs_domain::membership::{Capabilities, MemberRole, MemberStatusKind};
use gs_domain::session::SessionStatus;
use gs_ephemeral::EphemeralStore;
use gs_store::models::{
    Application, ApplicationStatus, DeviceStatus, SubjectStatus, Tenant, TenantStatus,
};
use gs_store::repos::{
    ApplicationRepository, DeviceRepository, MembershipRepository, SessionRepository,
    SubjectRepository, TenantRepository,
};
use gs_store::{PgPool, StoreError};
use serde_json::Value;
use thiserror::Error;
use time::OffsetDateTime;

/// Metric of cache events (docs/16 §4.2).
const METRIC_AUTHZ_CACHE_EVENTS_TOTAL: &str = "gs_authz_cache_events_total";

/// Default of `GS_AUTHZ_CACHE_TTL_MS` (docs/17 §2.2).
pub const DEFAULT_AUTHZ_CACHE_TTL: Duration = Duration::from_millis(5_000);

/// Identity of the principal of a connection, as revalidated by the cache (docs/06 §4.3).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PrincipalIdentity {
    /// Tenant of the principal.
    pub tenant_id: TenantId,
    /// Application of the principal.
    pub application_id: ApplicationId,
    /// Subject of a device principal.
    pub subject_id: Option<SubjectId>,
    /// Device of a device principal.
    pub device_id: Option<DeviceId>,
    /// Subject token epoch the credential was issued with (`sep`, docs/06 §4.2).
    pub token_epoch: Option<u32>,
}

/// A cached membership decision of a device principal (docs/04 §6.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceMembershipView {
    /// Tenant the decision belongs to.
    pub tenant_id: TenantId,
    /// Subject the decision is about.
    pub subject_id: SubjectId,
    /// Membership of the subject in the session.
    pub member_id: MemberId,
    /// Role of the membership.
    pub role: MemberRole,
    /// Capabilities of the membership.
    pub capabilities: Capabilities,
    /// Status of the membership.
    pub status: MemberStatusKind,
    /// Whether the subject is sharing its location.
    pub sharing: bool,
    /// Device publishing for this membership, when sharing.
    pub publisher_device_id: Option<DeviceId>,
    /// Status of the session.
    pub session_status: SessionStatus,
    /// Expiry of the session.
    pub session_expires_at: OffsetDateTime,
    /// Membership version the entry was loaded with.
    pub mver: u64,
    /// Instant the entry was loaded or renewed at.
    pub loaded_at: Instant,
}

/// A cached decision of an application principal (docs/04 §6.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationView {
    /// Tenant the decision belongs to.
    pub tenant_id: TenantId,
    /// Application the decision is about.
    pub application_id: ApplicationId,
    /// Whether the application is `ACTIVE` (docs/06 §4.3 step 4a).
    pub account_active: bool,
    /// Status of the session.
    pub session_status: SessionStatus,
    /// Expiry of the session.
    pub session_expires_at: OffsetDateTime,
    /// Scopes of the API key that asked the question.
    pub scopes: Vec<String>,
    /// Membership version the entry was loaded with.
    pub mver: u64,
    /// Instant the entry was loaded or renewed at.
    pub loaded_at: Instant,
}

/// A cached authorization decision (docs/04 §6.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CachedDecision {
    /// Decision of a device principal.
    Device(DeviceMembershipView),
    /// Decision of an application principal.
    Application(ApplicationView),
}

/// Key of a cached decision (docs/04 §6.2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AuthzKey {
    /// Device principal: `(session_id, subject_id)`.
    Device {
        /// Session the decision is about.
        session_id: SessionId,
        /// Subject of the principal.
        subject_id: SubjectId,
    },
    /// Application principal: `(session_id, application_id)`.
    Application {
        /// Session the decision is about.
        session_id: SessionId,
        /// Application of the principal.
        application_id: ApplicationId,
    },
}

/// Errors of the authorization cache, mapped to the catalog of `docs/09` §6.
#[derive(Debug, Error)]
pub enum AuthzError {
    /// The principal is no longer valid (docs/06 §4.3 step 4b/4c).
    #[error("unauthenticated: {0}")]
    Unauthenticated(String),

    /// The tenant or the application is disabled (docs/06 §4.3 step 4a).
    #[error("account disabled: {0}")]
    AccountDisabled(String),

    /// A persistence operation failed.
    #[error("storage error: {0}")]
    Store(#[from] StoreError),
}

impl AsErrorCode for AuthzError {
    fn error_code(&self) -> ErrorCode {
        match self {
            Self::Unauthenticated(_) => ErrorCode::Unauthenticated,
            Self::AccountDisabled(_) => ErrorCode::AccountDisabled,
            Self::Store(_) => ErrorCode::Internal,
        }
    }
}

/// Per instance authorization cache (ADR-0009, docs/04 §6.2).
#[derive(Debug)]
pub struct AuthzCache {
    decisions: DashMap<AuthzKey, CachedDecision>,
    principals: DashMap<PrincipalIdentity, Instant>,
    ttl: Duration,
}

impl Default for AuthzCache {
    fn default() -> Self {
        Self::new(DEFAULT_AUTHZ_CACHE_TTL)
    }
}

impl AuthzCache {
    /// Builds a cache with the given TTL (`GS_AUTHZ_CACHE_TTL_MS`).
    pub fn new(ttl: Duration) -> Self {
        Self {
            decisions: DashMap::new(),
            principals: DashMap::new(),
            ttl,
        }
    }

    /// Maximum age of a cached decision (docs/17 §2.2).
    pub const fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Number of cached decisions and principals: diagnostic helper for tests and metrics.
    pub fn len(&self) -> usize {
        self.decisions.len() + self.principals.len()
    }

    /// Whether the cache holds nothing.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the decision of a device principal, loading or revalidating it.
    ///
    /// A fresh entry answers immediately. Otherwise the membership version is read from the
    /// ephemeral store: unchanged renews the entry, changed reloads the memberships from
    /// PostgreSQL (docs/04 §6.2).
    pub async fn decision_for_device<E: EphemeralStore + ?Sized>(
        &self,
        pool: &PgPool,
        ephemeral: &E,
        key_prefix: &str,
        tenant_id: TenantId,
        session_id: SessionId,
        subject_id: SubjectId,
    ) -> Result<Option<DeviceMembershipView>, AuthzError> {
        let key = AuthzKey::Device {
            session_id,
            subject_id,
        };
        if let Some(CachedDecision::Device(view)) = self.fresh_decision(&key) {
            return Ok(Some(view));
        }

        let mver = read_mver(ephemeral, key_prefix, session_id).await;
        if self.renew_if_unchanged(&key, mver)
            && let Some(entry) = self.decisions.get(&key)
            && let CachedDecision::Device(view) = entry.value()
        {
            record_event("revalidate");
            return Ok(Some(view.clone()));
        }

        self.reload_device(pool, tenant_id, session_id, subject_id, mver)
            .await
    }

    /// Returns the decision of an application principal, loading or revalidating it.
    // The argument list is the documented cache key plus the question being asked
    /// (`04` §6.2); a params struct would hide which value is part of the key.
    #[allow(clippy::too_many_arguments)]
    pub async fn decision_for_application<E: EphemeralStore + ?Sized>(
        &self,
        pool: &PgPool,
        ephemeral: &E,
        key_prefix: &str,
        tenant_id: TenantId,
        session_id: SessionId,
        application_id: ApplicationId,
        scopes: Vec<String>,
    ) -> Result<Option<ApplicationView>, AuthzError> {
        let key = AuthzKey::Application {
            session_id,
            application_id,
        };
        if let Some(CachedDecision::Application(view)) = self.fresh_decision(&key) {
            return Ok(Some(view));
        }

        let mver = read_mver(ephemeral, key_prefix, session_id).await;
        if self.renew_if_unchanged(&key, mver)
            && let Some(entry) = self.decisions.get(&key)
            && let CachedDecision::Application(view) = entry.value()
        {
            record_event("revalidate");
            return Ok(Some(view.clone()));
        }

        self.reload_application(pool, tenant_id, session_id, application_id, scopes, mver)
            .await
    }

    /// Revalidates the principal of a connection at most once per TTL (docs/06 §7).
    ///
    /// Returns [`AuthzError::Unauthenticated`] when the subject epoch changed or the device is
    /// no longer active, and [`AuthzError::AccountDisabled`] when the tenant or the application
    /// were disabled.
    pub async fn revalidate_principal(
        &self,
        pool: &PgPool,
        identity: &PrincipalIdentity,
    ) -> Result<(), AuthzError> {
        if let Some(verified_at) = self.principals.get(identity)
            && verified_at.elapsed() < self.ttl
        {
            record_event("hit");
            return Ok(());
        }

        self.verify_principal(pool, identity).await?;
        self.principals.insert(identity.clone(), Instant::now());
        record_event("revalidate");
        Ok(())
    }

    /// Drops the principal entry of an identity, so the next check reaches PostgreSQL.
    pub fn invalidate_principal(&self, identity: &PrincipalIdentity) {
        self.principals.remove(identity);
    }

    /// Invalidates every cached decision of a session (bus accelerator, ADR-0009).
    ///
    /// Returns the number of entries dropped.
    pub fn invalidate_session(&self, session_id: SessionId) -> usize {
        let keys: Vec<AuthzKey> = self
            .decisions
            .iter()
            .filter(|entry| entry.key().session_id() == Some(session_id))
            .map(|entry| entry.key().clone())
            .collect();
        for key in &keys {
            self.decisions.remove(key);
        }
        keys.len()
    }

    /// Drops every entry: the bus reconnected (ADR-0009).
    pub fn flush(&self) {
        self.decisions.clear();
        self.principals.clear();
        record_event("flush");
    }

    /// Applies a message of `{prefix}:bus:global` published by the control plane
    /// (`TenantDisabled`, `ApplicationDisabled`, `ApiKeyRevoked`, `SubjectTokensRevoked`).
    ///
    /// The affected principals and their decisions are invalidated by identity. Unknown
    /// payloads are ignored: the TTL bound still applies (ADR-0009).
    pub fn note_global_bus_event(&self, payload: &[u8]) {
        let Ok(event): Result<Value, _> = serde_json::from_slice(payload) else {
            return;
        };
        let Some(name) = event.get("event").and_then(Value::as_str) else {
            return;
        };
        let tenant: Option<TenantId> = parse_id_field(&event, "tenant_id");
        let application: Option<ApplicationId> = parse_id_field(&event, "application_id");
        let subject: Option<SubjectId> = parse_id_field(&event, "subject_id");

        match name {
            "TenantDisabled" | "TenantEnabled" => {
                if let Some(tenant_id) = tenant {
                    self.invalidate_tenant(tenant_id);
                }
            }
            "ApplicationDisabled" | "ApplicationEnabled" | "ApiKeyRevoked" => {
                if let Some(application_id) = application {
                    self.invalidate_application(application_id);
                }
            }
            "SubjectTokensRevoked" => {
                if let Some(subject_id) = subject {
                    self.invalidate_subject(subject_id);
                }
            }
            _ => {}
        }
    }

    /// Invalidates every principal and decision of a tenant.
    pub fn invalidate_tenant(&self, tenant_id: TenantId) {
        self.principals
            .retain(|identity, _| identity.tenant_id != tenant_id);
        self.decisions
            .retain(|_, decision| decision.tenant_id() != Some(tenant_id));
    }

    /// Invalidates every principal and decision of an application.
    pub fn invalidate_application(&self, application_id: ApplicationId) {
        self.principals
            .retain(|identity, _| identity.application_id != application_id);
        self.decisions
            .retain(|_, decision| decision.application_id() != Some(application_id));
    }

    /// Invalidates every principal and decision of a subject.
    pub fn invalidate_subject(&self, subject_id: SubjectId) {
        self.principals
            .retain(|identity, _| identity.subject_id != Some(subject_id));
        self.decisions
            .retain(|_, decision| decision.subject_id() != Some(subject_id));
    }

    /// Returns a cached decision while it is inside the TTL.
    fn fresh_decision(&self, key: &AuthzKey) -> Option<CachedDecision> {
        let entry = self.decisions.get(key)?;
        if entry.loaded_at().elapsed() < self.ttl {
            record_event("hit");
            return Some(entry.value().clone());
        }
        None
    }

    /// Renews an entry whose membership version did not change (docs/04 §6.2).
    fn renew_if_unchanged(&self, key: &AuthzKey, mver: u64) -> bool {
        let Some(mut entry) = self.decisions.get_mut(key) else {
            return false;
        };
        if entry.mver() != mver {
            return false;
        }
        *entry.loaded_at_mut() = Instant::now();
        true
    }

    /// Reloads the membership of a device principal from PostgreSQL.
    async fn reload_device(
        &self,
        pool: &PgPool,
        tenant_id: TenantId,
        session_id: SessionId,
        subject_id: SubjectId,
        mver: u64,
    ) -> Result<Option<DeviceMembershipView>, AuthzError> {
        let membership = MembershipRepository::find_active_by_session_and_subject(
            pool, tenant_id, session_id, subject_id,
        )
        .await?;
        let session = SessionRepository::get_by_id(pool, tenant_id, session_id).await?;
        let key = AuthzKey::Device {
            session_id,
            subject_id,
        };

        let Some((membership, session)) = membership.zip(session) else {
            // A membership that no longer exists is not cached: the question is cheap while
            // nothing changes, and a revocation must not be remembered as a decision.
            self.decisions.remove(&key);
            return Ok(None);
        };

        let view = DeviceMembershipView {
            tenant_id,
            subject_id,
            member_id: membership.id,
            role: membership.role,
            capabilities: membership.capabilities,
            status: membership.status,
            sharing: membership.sharing,
            publisher_device_id: membership.publisher_device_id,
            session_status: session.status,
            session_expires_at: session.expires_at,
            mver,
            loaded_at: Instant::now(),
        };
        record_event("reload");
        self.decisions
            .insert(key, CachedDecision::Device(view.clone()));
        Ok(Some(view))
    }

    /// Reloads the decision of an application principal from PostgreSQL.
    async fn reload_application(
        &self,
        pool: &PgPool,
        tenant_id: TenantId,
        session_id: SessionId,
        application_id: ApplicationId,
        scopes: Vec<String>,
        mver: u64,
    ) -> Result<Option<ApplicationView>, AuthzError> {
        let application = ApplicationRepository::find_by_id(pool, application_id).await?;
        let session = SessionRepository::get_by_id(pool, tenant_id, session_id).await?;
        let key = AuthzKey::Application {
            session_id,
            application_id,
        };

        let Some((application, session)) = application.zip(session) else {
            self.decisions.remove(&key);
            return Ok(None);
        };
        if application.tenant_id != tenant_id {
            // Another tenant's application is not enumerable (FR-API-006).
            self.decisions.remove(&key);
            return Ok(None);
        }

        let view = ApplicationView {
            tenant_id,
            application_id,
            account_active: application.status == ApplicationStatus::Active,
            session_status: session.status,
            session_expires_at: session.expires_at,
            scopes,
            mver,
            loaded_at: Instant::now(),
        };
        record_event("reload");
        self.decisions
            .insert(key, CachedDecision::Application(view.clone()));
        Ok(Some(view))
    }

    /// Verifies the state of a principal against PostgreSQL (docs/06 §4.3 step 4).
    async fn verify_principal(
        &self,
        pool: &PgPool,
        identity: &PrincipalIdentity,
    ) -> Result<(), AuthzError> {
        verify_principal_state(pool, identity).await
    }
}

impl AuthzKey {
    /// Session the key is about.
    pub const fn session_id(&self) -> Option<SessionId> {
        match self {
            Self::Device { session_id, .. } | Self::Application { session_id, .. } => {
                Some(*session_id)
            }
        }
    }
}

impl CachedDecision {
    /// Instant the entry was loaded or renewed at.
    fn loaded_at(&self) -> Instant {
        match self {
            Self::Device(view) => view.loaded_at,
            Self::Application(view) => view.loaded_at,
        }
    }

    /// Mutable instant of the entry, used to renew it inside the TTL.
    fn loaded_at_mut(&mut self) -> &mut Instant {
        match self {
            Self::Device(view) => &mut view.loaded_at,
            Self::Application(view) => &mut view.loaded_at,
        }
    }

    /// Membership version the entry was loaded with.
    const fn mver(&self) -> u64 {
        match self {
            Self::Device(view) => view.mver,
            Self::Application(view) => view.mver,
        }
    }

    /// Tenant the decision belongs to.
    const fn tenant_id(&self) -> Option<TenantId> {
        match self {
            Self::Device(view) => Some(view.tenant_id),
            Self::Application(view) => Some(view.tenant_id),
        }
    }

    /// Application the decision belongs to.
    const fn application_id(&self) -> Option<ApplicationId> {
        match self {
            Self::Device(_) => None,
            Self::Application(view) => Some(view.application_id),
        }
    }

    /// Subject the decision belongs to.
    const fn subject_id(&self) -> Option<SubjectId> {
        match self {
            Self::Device(view) => Some(view.subject_id),
            Self::Application(_) => None,
        }
    }
}

/// Verifies the state of a principal against the truth in PostgreSQL (docs/06 §4.3 step 4).
///
/// Shared with the upgrade of `docs/08` §3 and with the periodic revalidation of a
/// connection, so that both paths apply the same checks in the same order.
pub async fn verify_principal_state(
    pool: &PgPool,
    identity: &PrincipalIdentity,
) -> Result<(), AuthzError> {
    let tenant = load_active_tenant(pool, identity.tenant_id).await?;
    let application = load_active_application(pool, &tenant, identity.application_id).await?;
    let _ = application;

    if let (Some(subject_id), Some(device_id)) = (identity.subject_id, identity.device_id) {
        verify_device_principal(pool, identity, subject_id, device_id).await?;
    }

    Ok(())
}

/// Loads the tenant and checks that it is `ACTIVE` (docs/06 §4.3 step 4a).
async fn load_active_tenant(pool: &PgPool, tenant_id: TenantId) -> Result<Tenant, AuthzError> {
    let tenant = TenantRepository::get_by_id(pool, tenant_id)
        .await?
        .ok_or_else(|| AuthzError::AccountDisabled("tenant not found".to_string()))?;
    if tenant.status != TenantStatus::Active {
        return Err(AuthzError::AccountDisabled("tenant disabled".to_string()));
    }
    Ok(tenant)
}

/// Loads the application, checks its tenant binding and that it is `ACTIVE`.
async fn load_active_application(
    pool: &PgPool,
    tenant: &Tenant,
    application_id: ApplicationId,
) -> Result<Application, AuthzError> {
    let application = ApplicationRepository::find_by_id(pool, application_id)
        .await?
        .ok_or_else(|| AuthzError::AccountDisabled("application not found".to_string()))?;
    if application.tenant_id != tenant.id {
        return Err(AuthzError::AccountDisabled(
            "application belongs to another tenant".to_string(),
        ));
    }
    if application.status != ApplicationStatus::Active {
        return Err(AuthzError::AccountDisabled(
            "application disabled".to_string(),
        ));
    }
    Ok(application)
}

/// Checks subject and device state (docs/06 §4.3 steps 4b and 4c).
async fn verify_device_principal(
    pool: &PgPool,
    identity: &PrincipalIdentity,
    subject_id: SubjectId,
    device_id: DeviceId,
) -> Result<(), AuthzError> {
    let subject = SubjectRepository::get_by_id(pool, identity.tenant_id, subject_id)
        .await?
        .ok_or_else(|| AuthzError::Unauthenticated("subject not found".to_string()))?;
    if subject.status != SubjectStatus::Active {
        return Err(AuthzError::Unauthenticated("subject erased".to_string()));
    }
    if let Some(epoch) = identity.token_epoch
        && subject.token_epoch != epoch
    {
        return Err(AuthzError::Unauthenticated(
            "subject tokens were revoked".to_string(),
        ));
    }

    let device = DeviceRepository::get_by_id(pool, identity.tenant_id, device_id)
        .await?
        .ok_or_else(|| AuthzError::Unauthenticated("device not found".to_string()))?;
    if device.subject_id != subject_id || device.application_id != identity.application_id {
        return Err(AuthzError::Unauthenticated(
            "device belongs to another subject or application".to_string(),
        ));
    }
    if device.status != DeviceStatus::Active {
        return Err(AuthzError::Unauthenticated(
            "device deactivated".to_string(),
        ));
    }

    Ok(())
}

/// Reads `{prefix}:mver:{session_id}` (docs/04 §6.2, docs/11 §3).
async fn read_mver<E: EphemeralStore + ?Sized>(
    ephemeral: &E,
    key_prefix: &str,
    session_id: SessionId,
) -> u64 {
    let key = format!("{key_prefix}:mver:{session_id}");
    ephemeral
        .get_membership_version(&key)
        .await
        .ok()
        .flatten()
        .unwrap_or(0)
}

/// Parses a typed identifier from a field of a bus payload.
fn parse_id_field<T>(event: &Value, field: &str) -> Option<T>
where
    T: std::str::FromStr,
{
    event
        .get(field)
        .and_then(Value::as_str)
        .and_then(|value| value.parse().ok())
}

/// Records a cache event in the documented metric (docs/16 §4.2).
fn record_event(event: &'static str) {
    metrics::counter!(METRIC_AUTHZ_CACHE_EVENTS_TOTAL, "event" => event).increment(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(session: SessionId, subject: SubjectId) -> AuthzKey {
        AuthzKey::Device {
            session_id: session,
            subject_id: subject,
        }
    }

    #[test]
    fn test_invalidate_session_drops_only_that_session() {
        // covers: ADR-0009, docs/04 §6.2
        let cache = AuthzCache::new(Duration::from_secs(60));
        let session_a = SessionId::new();
        let session_b = SessionId::new();
        let subject = SubjectId::new();
        cache.decisions.insert(
            key(session_a, subject),
            CachedDecision::Device(view(subject)),
        );
        cache.decisions.insert(
            key(session_b, subject),
            CachedDecision::Device(view(subject)),
        );

        assert_eq!(cache.invalidate_session(session_a), 1);
        assert_eq!(cache.len(), 1, "only the invalidated session is dropped");
        assert!(
            cache.decisions.get(&key(session_b, subject)).is_some(),
            "the other session survives"
        );
    }

    #[test]
    fn test_flush_drops_everything() {
        // covers: ADR-0009
        let cache = AuthzCache::new(Duration::from_secs(60));
        cache.decisions.insert(
            key(SessionId::new(), SubjectId::new()),
            CachedDecision::Device(view(SubjectId::new())),
        );
        cache.principals.insert(
            PrincipalIdentity {
                tenant_id: TenantId::new(),
                application_id: ApplicationId::new(),
                subject_id: None,
                device_id: None,
                token_epoch: None,
            },
            Instant::now(),
        );
        assert!(!cache.is_empty());
        cache.flush();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_global_bus_events_invalidate_the_affected_identities() {
        // covers: ADR-0009, docs/04 §6.1
        let cache = AuthzCache::new(Duration::from_secs(60));
        let tenant_id = TenantId::new();
        let application_id = ApplicationId::new();
        let subject_id = SubjectId::new();
        let identity = PrincipalIdentity {
            tenant_id,
            application_id,
            subject_id: Some(subject_id),
            device_id: Some(DeviceId::new()),
            token_epoch: Some(1),
        };
        cache.principals.insert(identity.clone(), Instant::now());
        let session_id = SessionId::new();
        cache.decisions.insert(
            key(session_id, subject_id),
            CachedDecision::Device(view(subject_id)),
        );

        // A revocation of the subject tokens drops its principal and its decisions.
        cache.note_global_bus_event(
            format!(
                r#"{{"event":"SubjectTokensRevoked","tenant_id":"{tenant_id}","subject_id":"{subject_id}","token_epoch":2}}"#
            )
            .as_bytes(),
        );
        assert!(
            cache.principals.is_empty(),
            "the principal must be invalidated"
        );
        assert!(
            cache.decisions.is_empty(),
            "its decisions must be invalidated"
        );

        // Unknown payloads are ignored.
        cache.principals.insert(identity, Instant::now());
        cache.note_global_bus_event(b"not json");
        cache.note_global_bus_event(br#"{"event":"SomethingElse"}"#);
        assert_eq!(cache.principals.len(), 1);
    }

    fn view(subject_id: SubjectId) -> DeviceMembershipView {
        DeviceMembershipView {
            tenant_id: TenantId::new(),
            subject_id,
            member_id: MemberId::new(),
            role: MemberRole::Member,
            capabilities: Capabilities::none(),
            status: MemberStatusKind::Active,
            sharing: false,
            publisher_device_id: None,
            session_status: SessionStatus::Active,
            session_expires_at: OffsetDateTime::now_utc(),
            mver: 1,
            loaded_at: Instant::now(),
        }
    }

    #[test]
    fn test_decision_keys_are_distinguished_by_principal_kind() {
        // covers: docs/04 §6.2
        let session_id = SessionId::new();
        let application_id = ApplicationId::new();
        let subject_id = SubjectId::new();
        let device_key = key(session_id, subject_id);
        let application_key = AuthzKey::Application {
            session_id,
            application_id,
        };
        assert_eq!(device_key.session_id(), Some(session_id));
        assert_eq!(application_key.session_id(), Some(session_id));
        assert_eq!(device_key, key(session_id, subject_id));
        assert_ne!(device_key, application_key);
    }
}
