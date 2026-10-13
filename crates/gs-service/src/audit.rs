//! Audit logging services and helpers for sensitive actions (docs/10 §8.3, docs/11 §2,
//! FR-PRV-005, FR-SHR-005, INV-TEN-1).
#![forbid(unsafe_code)]

use time::OffsetDateTime;
use uuid::Uuid;

use gs_domain::ids::{AuditId, RequestId, SubjectId, TenantId};
pub use gs_store::models::{ActorKind, AuditEvent, AuditOutcome};
pub use gs_store::repos::AuditRepository;
use gs_store::{PgPool, StoreError};

/// Records an audit event with strict tenant isolation (INV-TEN-1, docs/10 §8.3).
///
/// Ensures compliance with:
/// - `FR-PRV-005`: All sensitive lifecycle actions are audited.
/// - `FR-SHR-005`: Location sharing changes are audited without position coordinates.
/// - `docs/10 §8.2`: Never writes coordinates, tokens, secrets, tickets, invitation codes,
///   session metadata or raw `external_subject_id` to audit logs.
#[allow(clippy::too_many_arguments)]
pub async fn record_audit(
    pool: &PgPool,
    tenant_id: TenantId,
    actor_kind: ActorKind,
    actor_id: Option<Uuid>,
    subject_id: Option<SubjectId>,
    action: &str,
    target_kind: Option<&str>,
    target_id: Option<Uuid>,
    outcome: AuditOutcome,
    request_id: Option<RequestId>,
    details: serde_json::Value,
) -> Result<(), StoreError> {
    let event = AuditEvent {
        id: AuditId::new(),
        tenant_id,
        occurred_at: OffsetDateTime::now_utc(),
        actor_kind,
        actor_id,
        subject_id,
        action: action.to_string(),
        target_kind: target_kind.map(str::to_string),
        target_id,
        outcome,
        request_id,
        details,
    };
    AuditRepository::record(pool, &event).await
}
