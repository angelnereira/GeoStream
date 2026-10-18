//! `POST /v1/sessions/{session_id}/history` (OpenAPI `ingestHistory`, `docs/24` §2, E6).
//!
//! The backfill path for the client's `buffered` queue (`22` §3.5). It writes
//! **only** history: it never touches the live snapshot, the fan-out or the live
//! sequence. The triple key of `docs/24` §1 must hold:
//!
//! 1. the tenant policy has `history_allowed`;
//! 2. the session has `history_mode != OFF`;
//! 3. the member's `consent_purposes` contains `HISTORY` — granted by the
//!    subject, never by the backend (INV-SHR-5).
#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use gs_service::gs_domain::error::ErrorCode;
use gs_service::gs_domain::ids::SessionId;
use gs_service::gs_domain::location::{
    LocationPolicy, RawLocationSample, SampleResult, validate_sample,
};
use gs_service::gs_store::repos::{
    HistoryRepository, LocationPointRow, MembershipRepository, SessionRepository, TenantRepository,
};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use time::OffsetDateTime;

use crate::sessions::{CallerPrincipal, SessionApiState, authenticate_caller};
use crate::{Problem, ProblemResponse, request_id_from};

/// Resolution at which history points store their H3 cell.
///
/// Fixed at 9 for now, matching `pool_index_resolution`; a policy key for the
/// history resolution is a follow-up (see PROGRESS).
const HISTORY_H3_RESOLUTION: u8 = 9;

/// State for the history endpoints.
#[derive(Clone)]
pub struct HistoryState {
    /// Shared authentication/rate-limit state.
    pub sessions: SessionApiState,
}

/// Mounts `/v1/sessions/{session_id}/history`.
pub fn history_router(state: HistoryState) -> axum::Router {
    axum::Router::new()
        .route(
            "/v1/sessions/{session_id}/history",
            axum::routing::post(ingest_history_handler),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IngestHistoryRequest {
    pub samples: Vec<gs_protocol::LocationSample>,
}

#[derive(Debug, Serialize)]
pub struct IngestHistoryResponse {
    pub session_id: String,
    pub results: Vec<SampleResult>,
}

async fn ingest_history_handler(
    State(state): State<HistoryState>,
    Path(session_id_raw): Path<String>,
    headers: HeaderMap,
    body: Result<Json<IngestHistoryRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state.sessions, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };
    let (tenant_id, subject_id) = match &caller {
        CallerPrincipal::Device(dev) => {
            if !dev.claims.has_scope("ingest:write") {
                return problem(
                    ErrorCode::InsufficientScope,
                    &request_id,
                    "device requires ingest:write scope",
                );
            }
            (dev.tenant.id, dev.subject.id)
        }
        CallerPrincipal::Application(_) => {
            return problem(
                ErrorCode::PermissionDenied,
                &request_id,
                "applications cannot ingest history",
            );
        }
    };
    let session_id = match SessionId::from_str(&session_id_raw) {
        Ok(id) => id,
        Err(_) => {
            return problem(
                ErrorCode::ValidationFailed,
                &request_id,
                "malformed session id",
            );
        }
    };
    let body = match body {
        Ok(Json(b)) => b,
        Err(_) => {
            return problem(
                ErrorCode::ValidationFailed,
                &request_id,
                "body must be an IngestHistoryRequest",
            );
        }
    };
    if body.samples.is_empty() {
        return problem(
            ErrorCode::ValidationFailed,
            &request_id,
            "samples must not be empty",
        );
    }

    // 1. Tenant policy: `history_allowed`.
    let tenant = match TenantRepository::get_by_id(&state.sessions.pool, tenant_id).await {
        Ok(Some(t)) => t,
        Ok(None) => return problem(ErrorCode::NotFound, &request_id, "tenant not found"),
        Err(_) => return problem(ErrorCode::Internal, &request_id, "failed to load tenant"),
    };
    let policy = gs_service::gs_auth::TenantPolicy::from_json_value(tenant.policy.clone())
        .unwrap_or_default();
    if !policy.history_allowed {
        return problem(
            ErrorCode::FeatureDisabled,
            &request_id,
            "history is disabled for this tenant",
        );
    }

    // 2. Session `history_mode != OFF`.
    let history_mode =
        match SessionRepository::history_mode(&state.sessions.pool, tenant_id, session_id).await {
            Ok(Some(mode)) => mode,
            Ok(None) => return problem(ErrorCode::NotFound, &request_id, "session not found"),
            Err(_) => return problem(ErrorCode::Internal, &request_id, "failed to load session"),
        };
    if history_mode == "OFF" {
        return problem(
            ErrorCode::PreconditionFailed,
            &request_id,
            "session has history disabled",
        );
    }

    // 3. The member's consent, granted by the subject (INV-SHR-5).
    let member = match gs_service::members::get_member_for_device_me(
        &state.sessions.pool,
        tenant_id,
        session_id,
        subject_id,
    )
    .await
    {
        Ok(m) => m,
        Err(_) => return problem(ErrorCode::NotFound, &request_id, "member not found"),
    };
    let member_id = match gs_service::gs_domain::ids::MemberId::from_str(&member.id) {
        Ok(id) => id,
        Err(_) => return problem(ErrorCode::Internal, &request_id, "malformed member id"),
    };
    let purposes = MembershipRepository::consent_purposes(
        &state.sessions.pool,
        tenant_id,
        session_id,
        member_id,
    )
    .await
    .unwrap_or_default();
    if !purposes.iter().any(|p| p == "HISTORY") {
        return problem(
            ErrorCode::ConsentRequired,
            &request_id,
            "HISTORY consent has not been granted",
        );
    }

    // Validation and the age window of the backfill.
    let now = OffsetDateTime::now_utc();
    let location_policy = LocationPolicy {
        max_horizontal_accuracy_m: policy.max_horizontal_accuracy_m,
        reject_null_island: policy.reject_null_island,
        max_future_skew_ms: 0,
        // The backfill window replaces the live age window.
        max_event_age_ms: (policy.history_backfill_max_age_s as i64) * 1000,
        max_samples_per_publish: u32::MAX,
    };
    let device_id = match &caller {
        CallerPrincipal::Device(dev) => dev.device.id,
        _ => unreachable!("checked above"),
    };

    let mut results = Vec::with_capacity(body.samples.len());
    let mut rows = Vec::new();
    for sample in &body.samples {
        let json = match serde_json::to_value(sample) {
            Ok(v) => v,
            Err(_) => {
                results.push(SampleResult::rejected(
                    sample.sequence,
                    "VALIDATION_FAILED",
                    "sample",
                ));
                continue;
            }
        };
        let raw = RawLocationSample::from_json(&json);
        let validated = match validate_sample(&raw, &location_policy, now) {
            Ok(v) => v,
            Err(e) => {
                results.push(SampleResult::rejected(
                    sample.sequence,
                    e.error_code.to_string(),
                    e.field,
                ));
                continue;
            }
        };
        // The oldest accepted sample is `history_backfill_max_age_s`; older is
        // stale, not invalid — accepting more would write invented history.
        let age_s = (now - validated.captured_at).whole_seconds().max(0) as u64;
        if age_s > policy.history_backfill_max_age_s {
            results.push(SampleResult::ignored_stale(validated.sequence));
            continue;
        }
        if validated.captured_at > now {
            results.push(SampleResult::rejected(
                validated.sequence,
                "VALIDATION_FAILED",
                "captured_at",
            ));
            continue;
        }
        let h3_cell = gs_geo::h3::cell_of(
            gs_geo::LatLon::new(
                validated.position.latitude(),
                validated.position.longitude(),
            ),
            HISTORY_H3_RESOLUTION,
        )
        .unwrap_or(0) as i64;
        rows.push(LocationPointRow {
            id: uuid::Uuid::now_v7(),
            tenant_id,
            session_id: session_id.into_uuid(),
            member_id: member_id.into_uuid(),
            subject_id: subject_id.into_uuid(),
            device_id: device_id.into_uuid(),
            sequence: validated.sequence as i64,
            captured_at: validated.captured_at,
            received_at: now,
            latitude: validated.position.latitude(),
            longitude: validated.position.longitude(),
            accuracy_m: validated.position.horizontal_accuracy_m(),
            speed_mps: validated.position.speed_mps(),
            heading_deg: validated.position.heading_deg(),
            motion: Some(format!("{:?}", validated.quality.motion).to_uppercase()),
            is_mock: validated.quality.is_mock.unwrap_or(false),
            h3_cell,
        });
        results.push(SampleResult::accepted(validated.sequence));
    }

    // Create the partitions the batch spans, then write in one transaction.
    if !rows.is_empty() {
        for row in &rows {
            if HistoryRepository::ensure_month_partition(&state.sessions.pool, row.captured_at)
                .await
                .is_err()
            {
                return problem(
                    ErrorCode::Internal,
                    &request_id,
                    "failed to ensure partition",
                );
            }
        }
        if HistoryRepository::insert_points(&state.sessions.pool, &rows)
            .await
            .is_err()
        {
            return problem(ErrorCode::Internal, &request_id, "failed to write history");
        }
    }

    let mut response = (
        StatusCode::OK,
        Json(IngestHistoryResponse {
            session_id: session_id_raw,
            results,
        }),
    )
        .into_response();
    if let Ok(value) = axum::http::HeaderValue::from_str(&request_id) {
        response
            .headers_mut()
            .insert(crate::REQUEST_ID_HEADER, value);
    }
    response
}

fn problem(code: ErrorCode, request_id: &str, detail: &str) -> Response {
    let problem = Problem::new(code, request_id).with_detail(detail);
    ProblemResponse::new(problem).into_response()
}
