//! Use cases for subjects and devices (docs/06 §7, docs/07 §9.5, FR-AUTH-005).
#![forbid(unsafe_code)]

use gs_bus::Bus;
use gs_domain::error::{AsErrorCode, ErrorCode};
use gs_domain::ids::{ApplicationId, DeviceId, ExternalSubjectId, SubjectId, TenantId};
use gs_ephemeral::EphemeralStore;
use gs_store::models::{ActorKind, AuditOutcome, Device, DeviceStatus, Subject, SubjectStatus};
use gs_store::repos::{DeviceRepository, SubjectRepository};
use gs_store::{PgPool, StoreError};

use crate::audit::record_audit;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Public REST representation of a subject (`docs/07` §9.5, OpenAPI `Subject`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubjectResource {
    pub id: SubjectId,
    pub external_subject_id: String,
    pub status: String,
    pub token_epoch: u32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// Paginated list of subjects (`docs/07` §9.5, OpenAPI `SubjectList`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubjectListResource {
    pub data: Vec<SubjectResource>,
    pub next_cursor: Option<String>,
}

/// Public REST representation of a device (`docs/07` §9.5, OpenAPI `Device`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceResource {
    pub id: DeviceId,
    pub subject_id: SubjectId,
    pub application_id: ApplicationId,
    pub platform: String,
    pub app_version: Option<String>,
    pub sdk_version: Option<String>,
    pub status: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub last_seen_at: OffsetDateTime,
}

/// Errors occurring during subject and device use cases.
#[derive(Debug, thiserror::Error)]
pub enum SubjectUseCaseError {
    #[error("subject not found")]
    SubjectNotFound,

    #[error("device not found")]
    DeviceNotFound,

    #[error("insufficient scope: {0}")]
    InsufficientScope(&'static str),

    #[error("permission denied")]
    PermissionDenied,

    #[error("invalid external_subject_id: {0}")]
    InvalidExternalSubjectId(String),

    #[error("database error: {0}")]
    Store(#[from] StoreError),
}

impl AsErrorCode for SubjectUseCaseError {
    fn error_code(&self) -> ErrorCode {
        match self {
            Self::SubjectNotFound | Self::DeviceNotFound => ErrorCode::NotFound,
            Self::InsufficientScope(_) => ErrorCode::InsufficientScope,
            Self::PermissionDenied => ErrorCode::PermissionDenied,
            Self::InvalidExternalSubjectId(_) => ErrorCode::ValidationFailed,
            Self::Store(_) => ErrorCode::Internal,
        }
    }
}

/// Converts a store `Subject` to `SubjectResource`.
pub fn subject_to_resource(subject: Subject) -> SubjectResource {
    SubjectResource {
        id: subject.id,
        external_subject_id: subject.external_subject_id.unwrap_or_default(),
        status: subject.status.as_str().to_string(),
        token_epoch: subject.token_epoch,
        created_at: subject.created_at,
    }
}

/// Converts a store `Device` to `DeviceResource`.
pub fn device_to_resource(device: Device) -> DeviceResource {
    DeviceResource {
        id: device.id,
        subject_id: device.subject_id,
        application_id: device.application_id,
        platform: device.platform.as_str().to_string(),
        app_version: device.app_version,
        sdk_version: device.sdk_version,
        status: device.status.as_str().to_string(),
        created_at: device.created_at,
        last_seen_at: device.last_seen_at,
    }
}

/// Searches for an active subject by external identifier within a tenant (`docs/07` §9.5, OpenAPI `findSubjects`).
pub async fn find_subject_by_external_id(
    pool: &PgPool,
    tenant_id: TenantId,
    external_subject_id: &str,
) -> Result<Option<SubjectResource>, SubjectUseCaseError> {
    let ext = ExternalSubjectId::new(external_subject_id)
        .map_err(|e| SubjectUseCaseError::InvalidExternalSubjectId(e.to_string()))?;
    let subject =
        SubjectRepository::find_active_by_external_id(pool, tenant_id, ext.as_str()).await?;
    Ok(subject.map(subject_to_resource))
}

/// Fetches a subject by identifier (`docs/07` §9.5, OpenAPI `getSubject`).
pub async fn get_subject(
    pool: &PgPool,
    tenant_id: TenantId,
    subject_id: SubjectId,
) -> Result<SubjectResource, SubjectUseCaseError> {
    let subject = SubjectRepository::get_by_id(pool, tenant_id, subject_id)
        .await?
        .ok_or(SubjectUseCaseError::SubjectNotFound)?;
    if subject.status != SubjectStatus::Active {
        return Err(SubjectUseCaseError::SubjectNotFound);
    }
    Ok(subject_to_resource(subject))
}

/// Atomically invalidates all tokens of a subject by incrementing `token_epoch` (`docs/06` §7, docs/07 §9.5, OpenAPI `revokeSubjectTokens`).
pub async fn revoke_subject_tokens(
    pool: &PgPool,
    tenant_id: TenantId,
    subject_id: SubjectId,
    now: OffsetDateTime,
) -> Result<SubjectResource, SubjectUseCaseError> {
    let subject = SubjectRepository::revoke_tokens(pool, tenant_id, subject_id, now)
        .await
        .map_err(|e| match e {
            StoreError::NotFound { .. } => SubjectUseCaseError::SubjectNotFound,
            other => SubjectUseCaseError::Store(other),
        })?;
    let res = subject_to_resource(subject);
    let _ = record_audit(
        pool,
        tenant_id,
        ActorKind::Application,
        None,
        Some(subject_id),
        "subject.tokens_revoked",
        Some("subject"),
        Some(subject_id.into_uuid()),
        AuditOutcome::Success,
        None,
        serde_json::json!({
            "token_epoch": res.token_epoch,
        }),
    )
    .await;
    Ok(res)
}

/// Fetches a device by identifier (`docs/07` §9.5, OpenAPI `getDevice`).
///
/// If `caller_subject_id_if_device` is provided (caller is a device), enforces `own_subject_device`:
/// the target device must belong to the caller's subject (FR-API-006: non-enumeration).
pub async fn get_device(
    pool: &PgPool,
    tenant_id: TenantId,
    device_id: DeviceId,
    caller_subject_id_if_device: Option<SubjectId>,
) -> Result<DeviceResource, SubjectUseCaseError> {
    let device = DeviceRepository::get_by_id(pool, tenant_id, device_id)
        .await?
        .ok_or(SubjectUseCaseError::DeviceNotFound)?;

    if caller_subject_id_if_device.is_some_and(|sub| sub != device.subject_id) {
        return Err(SubjectUseCaseError::DeviceNotFound);
    }

    Ok(device_to_resource(device))
}

/// Parameters for deactivating a device.
pub struct DeactivateDeviceParams<'a, E: EphemeralStore, B: Bus> {
    pub pool: &'a PgPool,
    pub ephemeral: &'a E,
    pub bus: &'a B,
    pub tenant_id: TenantId,
    pub device_id: DeviceId,
    pub caller_device_id: Option<DeviceId>,
    pub key_prefix: &'a str,
    pub now: OffsetDateTime,
}

/// Deactivates a device, stopping sharing where it was the publisher and clearing snapshots
/// (`docs/06` §7, `docs/07` §9.5, OpenAPI `deactivateDevice`, FR-AUTH-005).
///
/// Idempotent: returns success if the device is already INACTIVE.
pub async fn deactivate_device<E: EphemeralStore, B: Bus>(
    params: DeactivateDeviceParams<'_, E, B>,
) -> Result<(), SubjectUseCaseError> {
    let DeactivateDeviceParams {
        pool,
        ephemeral,
        bus,
        tenant_id,
        device_id,
        caller_device_id,
        key_prefix,
        now,
    } = params;

    let device = DeviceRepository::get_by_id(pool, tenant_id, device_id)
        .await?
        .ok_or(SubjectUseCaseError::DeviceNotFound)?;

    if caller_device_id.is_some_and(|did| did != device.id) {
        // FR-API-006: non-enumeration
        return Err(SubjectUseCaseError::DeviceNotFound);
    }

    if device.status == DeviceStatus::Inactive {
        return Ok(());
    }

    let affected =
        DeviceRepository::deactivate_and_stop_publishing(pool, tenant_id, device_id, now).await?;

    let (actor_kind, actor_id) = match caller_device_id {
        Some(did) => (ActorKind::Device, Some(did.into_uuid())),
        None => (ActorKind::Application, None),
    };
    let _ = record_audit(
        pool,
        tenant_id,
        actor_kind,
        actor_id,
        Some(device.subject_id),
        "device.deactivated",
        Some("device"),
        Some(device_id.into_uuid()),
        AuditOutcome::Success,
        None,
        serde_json::json!({
            "affected_sessions_count": affected.len(),
        }),
    )
    .await;

    for (session_id, member_id) in affected {
        let _ = record_audit(
            pool,
            tenant_id,
            ActorKind::System,
            None,
            Some(device.subject_id),
            "sharing.stopped",
            Some("member"),
            Some(member_id.into_uuid()),
            AuditOutcome::Success,
            None,
            serde_json::json!({
                "session_id": session_id.to_string(),
                "sharing": false,
                "reason": "device_deactivated",
            }),
        )
        .await;

        // 1. Delete ephemeral snapshot (NFR-PRIV-002, retry up to 3 times)
        let snap_key = format!("{key_prefix}:snap:{session_id}:{member_id}");
        for attempt in 0..3 {
            match ephemeral.delete_snapshot(&snap_key).await {
                Ok(_) => break,
                Err(e) => {
                    tracing::warn!(attempt, error = %e, "failed to delete ephemeral snapshot during device deactivation");
                }
            }
        }

        // 2. Increment membership version
        let mver_key = format!("{key_prefix}:mver:{session_id}");
        if let Err(e) = ephemeral.increment_membership_version(&mver_key).await {
            tracing::warn!(error = %e, "failed to increment membership version during device deactivation");
        }

        // 3. Publish control event on bus
        let ctl_topic = format!("{key_prefix}:bus:ctl:{session_id}");
        let payload = serde_json::to_vec(&serde_json::json!({
            "event": "SharingStopped",
            "member_id": member_id.to_string(),
            "device_id": device_id.to_string(),
            "at": now.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
        }))
        .unwrap_or_default();
        let _ = bus.publish(&ctl_topic, &payload).await;
    }

    Ok(())
}
