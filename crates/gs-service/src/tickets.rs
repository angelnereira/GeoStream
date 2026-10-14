//! Stream tickets: issuance and atomic redemption (docs/06 §5, docs/08 §3, FR-AUTH-006).
//!
//! `POST /v1/stream/tickets` returns an opaque ticket that authenticates a single WebSocket
//! upgrade. The ticket lives only in the ephemeral store, under the key
//! `{prefix}:ticket:{sha256(ticket)}` with TTL `GS_TICKET_TTL_S` (docs/11 §3), and is consumed
//! with an atomic get-and-delete. `auth_expires_at` bounds how long the connection it grants
//! stays authenticated: the `exp` of the device token that asked for it, or
//! `now + GS_STREAM_APP_MAX_CONNECTION_AGE_S` for an application (docs/06 §5).
//!
//! The ticket value itself is a secret: it travels in a [`zeroize::Zeroizing`] string, is
//! never logged and only ever reaches storage as a SHA-256 digest (docs/10 §8.2).
#![forbid(unsafe_code)]

use std::time::Duration;

use gs_auth::ticket::{generate_ticket, hash_ticket_hex, validate_ticket};
use gs_domain::error::{AsErrorCode, ErrorCode};
use gs_domain::ids::{ApiKeyId, ApplicationId, DeviceId, SubjectId, TenantId};
use gs_ephemeral::EphemeralStore;
use gs_store::models::{ApplicationStatus, TenantStatus};
use gs_store::repos::{ApplicationRepository, TenantRepository};
use gs_store::{PgPool, StoreError};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;
use zeroize::Zeroizing;

use crate::auth::{ApplicationPrincipal, VerifiedPrincipal};

/// Scope required to issue a stream ticket (docs/06 §3.4, §6 and §8.2).
pub const STREAM_CONNECT_SCOPE: &str = "stream:connect";

/// Kind of principal a ticket was issued to (docs/06 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TicketPrincipalKind {
    /// Application API key holder.
    Application,
    /// Device token holder.
    Device,
}

/// Grant stored in the ephemeral store as the value of `{p}:ticket:{sha256}` (docs/06 §5).
///
/// The JSON is compact and carries no coordinates and no secrets of any kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketGrant {
    /// Kind of principal the ticket authenticates.
    pub principal_kind: TicketPrincipalKind,
    /// Tenant of the principal.
    pub tenant_id: TenantId,
    /// Application of the principal.
    pub application_id: ApplicationId,
    /// Subject of a device principal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_id: Option<SubjectId>,
    /// Device of a device principal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<DeviceId>,
    /// API key of an application principal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_id: Option<ApiKeyId>,
    /// Scopes granted to the connection.
    pub scopes: Vec<String>,
    /// End of the authentication window, RFC 3339 (docs/06 §5).
    pub auth_expires_at: String,
}

/// Ticket issuance parameters (`GS_TICKET_TTL_S`, `GS_STREAM_APP_MAX_CONNECTION_AGE_S`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TicketConfig {
    /// Life of the ticket itself (docs/17 §2.4).
    pub ttl: Duration,
    /// Maximum life of an application connection (docs/17 §2.6).
    pub app_max_connection_age: Duration,
}

/// A ticket just issued: the secret value and the grant it maps to.
#[derive(Debug, Clone)]
pub struct IssuedTicket {
    /// The ticket value, `gst_…` (docs/06 §5). Never log it.
    pub ticket: Zeroizing<String>,
    /// The grant stored for this ticket.
    pub grant: TicketGrant,
    /// Instant at which the ticket itself stops being redeemable (`GS_TICKET_TTL_S`).
    pub expires_at: OffsetDateTime,
}

/// Principal asking for a ticket: what `authenticate_caller` already verified.
pub enum TicketCaller<'a> {
    /// Application key holder.
    Application(&'a ApplicationPrincipal),
    /// Device token holder.
    Device(&'a VerifiedPrincipal),
}

/// Errors of ticket issuance and redemption, mapped to the catalog of `docs/09` §6.
#[derive(Debug, Error)]
pub enum TicketError {
    /// The ticket is absent, malformed, expired or already used (docs/08 §3).
    #[error("invalid or expired stream ticket")]
    Unauthenticated,

    /// The ticket is valid but its tenant or application was disabled since issuance
    /// (docs/08 §3).
    #[error("account disabled: {0}")]
    AccountDisabled(String),

    /// The ephemeral store is degraded or unreachable (`docs/04` §7.6).
    #[error("stream ticket store temporarily unavailable")]
    TemporarilyUnavailable,

    /// A server side invariant of the grant was violated (never caused by the client).
    #[error("ticket grant is not representable: {0}")]
    Internal(String),

    /// A persistence operation failed.
    #[error("storage error: {0}")]
    Store(#[from] StoreError),
}

impl AsErrorCode for TicketError {
    fn error_code(&self) -> ErrorCode {
        match self {
            Self::Unauthenticated => ErrorCode::Unauthenticated,
            Self::AccountDisabled(_) => ErrorCode::AccountDisabled,
            Self::TemporarilyUnavailable => ErrorCode::TemporarilyUnavailable,
            // The grant is built and parsed by the server: a failure here is its own fault.
            Self::Internal(_) | Self::Store(_) => ErrorCode::Internal,
        }
    }
}

/// Builds the ephemeral key of a ticket: `{prefix}:ticket:{sha256_hex(ticket)}` (docs/11 §3).
pub fn ticket_key(key_prefix: &str, ticket: &str) -> String {
    format!("{key_prefix}:ticket:{}", hash_ticket_hex(ticket))
}

/// Issues a single-use ticket for the given principal (docs/06 §5).
///
/// Returns [`TicketError::TemporarilyUnavailable`] when the ephemeral store cannot take the
/// write, which is the degradation behaviour of `docs/04` §7.6.
pub async fn issue_stream_ticket<E: EphemeralStore + ?Sized>(
    ephemeral: &E,
    key_prefix: &str,
    caller: &TicketCaller<'_>,
    config: &TicketConfig,
    now: OffsetDateTime,
) -> Result<IssuedTicket, TicketError> {
    let ticket = Zeroizing::new(generate_ticket());
    // The ticket expires with its TTL; the grant it maps to carries the longer
    // authentication window of the connection it will open (docs/06 §5).
    let expires_at = now + config.ttl;
    let grant = match caller {
        TicketCaller::Application(principal) => TicketGrant {
            principal_kind: TicketPrincipalKind::Application,
            tenant_id: principal.tenant.id,
            application_id: principal.application.id,
            subject_id: None,
            device_id: None,
            api_key_id: Some(principal.api_key.id),
            scopes: principal.api_key.scopes.clone(),
            auth_expires_at: format_rfc3339(now + config.app_max_connection_age),
        },
        TicketCaller::Device(principal) => {
            let auth_expires_at = OffsetDateTime::from_unix_timestamp(principal.claims.exp)
                .map_err(|error| TicketError::Internal(format!("token exp: {error}")))?;
            TicketGrant {
                principal_kind: TicketPrincipalKind::Device,
                tenant_id: principal.tenant.id,
                application_id: principal.application.id,
                subject_id: Some(principal.subject.id),
                device_id: Some(principal.device.id),
                api_key_id: None,
                scopes: principal.claims.scp.clone(),
                auth_expires_at: format_rfc3339(auth_expires_at),
            }
        }
    };

    let payload = serde_json::to_vec(&grant)
        .map_err(|error| TicketError::Internal(format!("grant: {error}")))?;
    ephemeral
        .set_ticket(
            &ticket_key(key_prefix, ticket.as_str()),
            &payload,
            config.ttl,
        )
        .await
        .map_err(|_| TicketError::TemporarilyUnavailable)?;

    Ok(IssuedTicket {
        ticket,
        grant,
        expires_at,
    })
}

/// Redeems a ticket with an atomic get-and-delete (docs/06 §5, docs/08 §3).
///
/// A ticket that does not exist, has expired, has already been used, or whose authentication
/// window has closed is rejected as [`TicketError::Unauthenticated`]: the upgrade answers
/// HTTP 401 without switching protocols.
pub async fn redeem_stream_ticket<E: EphemeralStore + ?Sized>(
    ephemeral: &E,
    key_prefix: &str,
    ticket: &str,
    now: OffsetDateTime,
) -> Result<TicketGrant, TicketError> {
    validate_ticket(ticket).map_err(|_| TicketError::Unauthenticated)?;

    let payload = ephemeral
        .take_ticket(&ticket_key(key_prefix, ticket))
        .await
        .map_err(|_| TicketError::TemporarilyUnavailable)?;
    let Some(payload) = payload else {
        return Err(TicketError::Unauthenticated);
    };

    let grant: TicketGrant =
        serde_json::from_slice(&payload).map_err(|_| TicketError::Unauthenticated)?;
    let auth_expires_at = OffsetDateTime::parse(
        &grant.auth_expires_at,
        &time::format_description::well_known::Rfc3339,
    )
    .map_err(|_| TicketError::Unauthenticated)?;
    if auth_expires_at <= now {
        return Err(TicketError::Unauthenticated);
    }

    Ok(grant)
}

/// Re-checks the state the ticket was issued with: tenant and application `ACTIVE`
/// (docs/08 §3, docs/06 §7).
///
/// A tenant or application disabled between issuance and upgrade answers HTTP 403
/// `ACCOUNT_DISABLED` without switching protocols. Subject and device state are revalidated
/// by the authorization cache of the connection itself (docs/06 §7, T-F4-03).
pub async fn verify_ticket_principal_state(
    pool: &PgPool,
    grant: &TicketGrant,
) -> Result<(), TicketError> {
    let tenant = TenantRepository::get_by_id(pool, grant.tenant_id)
        .await?
        .ok_or_else(|| TicketError::AccountDisabled("tenant not found".to_string()))?;
    if tenant.status != TenantStatus::Active {
        return Err(TicketError::AccountDisabled("tenant disabled".to_string()));
    }

    let application = ApplicationRepository::find_by_id(pool, grant.application_id)
        .await?
        .ok_or_else(|| TicketError::AccountDisabled("application not found".to_string()))?;
    if application.tenant_id != grant.tenant_id {
        return Err(TicketError::AccountDisabled(
            "application belongs to another tenant".to_string(),
        ));
    }
    if application.status != ApplicationStatus::Active {
        return Err(TicketError::AccountDisabled(
            "application disabled".to_string(),
        ));
    }

    Ok(())
}

/// Returns true when the grant carries `scope` (docs/06 §5).
pub fn grant_has_scope(grant: &TicketGrant, scope: &str) -> bool {
    grant.scopes.iter().any(|granted| granted == scope)
}

/// Formats an instant as RFC 3339 (docs/00 §4.5).
// INVARIANT: formatting an `OffsetDateTime` as RFC 3339 cannot fail; the offset is always
// representable. The fallback keeps the function total, like the sibling helpers of this crate.
fn format_rfc3339(instant: OffsetDateTime) -> String {
    instant
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}
