//! `POST /v1/ingest/locations` (OpenAPI `ingestLocations`, `docs/09` §4, F8).
//!
//! The HTTP counterpart of the WebSocket `PublishLocation`: same acceptance
//! rules, same engine, but without a live connection. A device uploads the
//! `buffered` queue of `22` §3.5 here after a network gap.
//!
//! The sample limit is `max_samples_per_publish_http` (50), not the stream's
//! `max_samples_per_publish` (1). The REST rate limiter of the credential is the
//! gate, so the per-connection token bucket is created fresh per request.
#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use gs_service::EphemeralStore as _;
use gs_service::gs_domain::error::ErrorCode;
use gs_service::gs_domain::ids::SessionId;
use gs_service::gs_domain::location::LocationPolicy;
use gs_service::gs_store::repos::TenantRepository;
use gs_service::publishing::{PublishCaller, PublishParams, PublishThrottle, publish_batch};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::time::Duration;

use crate::sessions::{CallerPrincipal, SessionApiState, authenticate_caller};
use crate::{Problem, ProblemResponse, request_id_from};

/// TTL of the checkpoint written by the HTTP ingest (`GS_TRACK_STATE_TTL_S`).
const TRACK_STATE_TTL: Duration = Duration::from_secs(600);

/// State for the HTTP ingest.
#[derive(Clone)]
pub struct IngestState {
    /// Shared authentication/rate-limit state.
    pub sessions: SessionApiState,
    /// `GS_LOCATION_MAX_EVENT_AGE_MS`: older samples are `IGNORED_STALE` (`09` §3.2).
    pub max_event_age_ms: i64,
    /// `GS_LOCATION_MAX_FUTURE_SKEW_MS`: tolerance for `captured_at` in the future (`09` §3.2).
    pub max_future_skew_ms: i64,
}

/// Defaults of `17` §2 for `GS_LOCATION_MAX_EVENT_AGE_MS` and `GS_LOCATION_MAX_FUTURE_SKEW_MS`.
pub const DEFAULT_MAX_EVENT_AGE_MS: i64 = 120_000;
/// See [`DEFAULT_MAX_EVENT_AGE_MS`].
pub const DEFAULT_MAX_FUTURE_SKEW_MS: i64 = 5_000;

impl IngestState {
    /// State with the documented default limits.
    pub fn with_defaults(sessions: SessionApiState) -> Self {
        Self {
            sessions,
            max_event_age_ms: DEFAULT_MAX_EVENT_AGE_MS,
            max_future_skew_ms: DEFAULT_MAX_FUTURE_SKEW_MS,
        }
    }
}

/// The OpenAPI documents the enums by their short names (`MEASURED`, `GNSS`, `AUTOMOTIVE`); the
/// Protobuf JSON mapping wants the full ones (`LOCATION_KIND_MEASURED`…). A client typed against the
/// contract must work, so the short names are accepted and expanded before decoding. The full names
/// still work.
fn expand_enum_aliases(body: &mut serde_json::Value) {
    const FIELDS: [(&str, &str); 3] = [
        ("source", "LOCATION_SOURCE_"),
        ("kind", "LOCATION_KIND_"),
        ("motion", "MOTION_ACTIVITY_"),
    ];
    let Some(samples) = body.get_mut("samples").and_then(|v| v.as_array_mut()) else {
        return;
    };
    for sample in samples {
        let Some(quality) = sample.get_mut("quality").and_then(|v| v.as_object_mut()) else {
            continue;
        };
        for (field, prefix) in FIELDS {
            if let Some(serde_json::Value::String(name)) = quality.get_mut(field)
                && !name.starts_with(prefix)
            {
                *name = format!("{prefix}{name}");
            }
        }
    }
}

/// Mounts `/v1/ingest/*`.
pub fn ingest_router(state: IngestState) -> axum::Router {
    axum::Router::new()
        .route(
            "/v1/ingest/locations",
            axum::routing::post(ingest_locations_handler),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IngestRequest {
    pub session_id: String,
    pub samples: Vec<gs_protocol::LocationSample>,
}

#[derive(Debug, Serialize)]
pub struct IngestResponse {
    pub session_id: String,
    pub results: Vec<gs_service::gs_domain::location::SampleResult>,
}

async fn ingest_locations_handler(
    State(state): State<IngestState>,
    headers: HeaderMap,
    body: Result<Json<serde_json::Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state.sessions, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };
    let (tenant_id, subject_id, device_id) = match &caller {
        CallerPrincipal::Device(dev) => {
            if !dev.claims.has_scope("ingest:write") {
                return problem(
                    ErrorCode::InsufficientScope,
                    &request_id,
                    "device requires ingest:write scope",
                );
            }
            (dev.tenant.id, dev.subject.id, dev.device.id)
        }
        CallerPrincipal::Application(_) => {
            return problem(
                ErrorCode::PermissionDenied,
                &request_id,
                "applications cannot ingest locations",
            );
        }
    };

    let body = match body
        .ok()
        .map(|Json(mut raw)| {
            expand_enum_aliases(&mut raw);
            serde_json::from_value::<IngestRequest>(raw)
        })
        .and_then(Result::ok)
    {
        Some(b) => b,
        None => {
            return problem(
                ErrorCode::ValidationFailed,
                &request_id,
                "body must be an IngestRequest",
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
    let session_id = match SessionId::from_str(&body.session_id) {
        Ok(id) => id,
        Err(_) => {
            return problem(
                ErrorCode::ValidationFailed,
                &request_id,
                "malformed session id",
            );
        }
    };

    // Tenant policy: the HTTP batch limit and the throttle parameters.
    let tenant = match TenantRepository::get_by_id(&state.sessions.pool, tenant_id).await {
        Ok(Some(t)) => t,
        Ok(None) => return problem(ErrorCode::NotFound, &request_id, "tenant not found"),
        Err(_) => return problem(ErrorCode::Internal, &request_id, "failed to load tenant"),
    };
    let policy = gs_service::gs_auth::TenantPolicy::from_json_value(tenant.policy.clone())
        .unwrap_or_default();

    let location_policy = LocationPolicy {
        max_horizontal_accuracy_m: policy.max_horizontal_accuracy_m,
        reject_null_island: policy.reject_null_island,
        reject_mock_samples: policy.reject_mock_samples,
        reject_network_only: policy.reject_network_only,
        max_future_skew_ms: state.max_future_skew_ms,
        max_event_age_ms: state.max_event_age_ms,
        // The HTTP path accepts the batch limit of `max_samples_per_publish_http`.
        max_samples_per_publish: policy.max_samples_per_publish_http,
    };
    if body.samples.len() as u32 > policy.max_samples_per_publish_http {
        return problem(
            ErrorCode::PayloadTooLarge,
            &request_id,
            "too many samples for one ingest request",
        );
    }

    // Trajectory engine: read the checkpoint with its version, apply, and write
    // it back with a version check (`track_cas.lua`, `docs/22` §2.1). The HTTP
    // ingest has no instance affinity, so two instances can race on the same
    // device; the CAS makes the loser retry instead of losing an update.
    let trk_key = format!("{}:trk:{}", state.sessions.redis_key_prefix, device_id);
    let (mut tracker, checkpoint_version) =
        match state.sessions.ephemeral.get_checkpoint(&trk_key).await {
            Ok(Some((bytes, version))) => (
                gs_track::TrackStateMachine::decode_checkpoint(&bytes).unwrap_or_else(|| {
                    gs_track::TrackStateMachine::new(gs_track::TrackConfig::defaults())
                }),
                version,
            ),
            _ => (
                gs_track::TrackStateMachine::new(gs_track::TrackConfig::defaults()),
                0,
            ),
        };

    let mut throttle = PublishThrottle::new(
        policy.publish_burst,
        Duration::from_millis(policy.min_publish_interval_ms),
        time::OffsetDateTime::now_utc(),
    );

    let outcome = match publish_batch(PublishParams {
        pool: &state.sessions.pool,
        ephemeral: &*state.sessions.ephemeral,
        key_prefix: &state.sessions.redis_key_prefix,
        tenant_id,
        session_id,
        caller: PublishCaller {
            subject_id,
            device_id,
        },
        samples: &body.samples,
        policy: location_policy,
        throttle: &mut throttle,
        tracker: Some(&mut tracker),
        now: time::OffsetDateTime::now_utc(),
    })
    .await
    {
        Ok(outcome) => outcome,
        Err(error) => return publish_error(&error, &request_id),
    };

    // Checkpoint the engine with a version check. On conflict we retry once;
    // if it conflicts again the other instance won, and we leave its checkpoint
    // (the next sample corrects the trajectory). The estimate already travelled
    // in `outcome.accepted`, so the batch is never silently degraded.
    if !outcome.accepted.is_empty() {
        let blob = tracker.encode_checkpoint();
        let mut applied = false;
        for _ in 0..2 {
            match state
                .sessions
                .ephemeral
                .compare_and_set_checkpoint(&trk_key, checkpoint_version, &blob, TRACK_STATE_TTL)
                .await
            {
                Ok(gs_service::gs_ephemeral::CheckpointCas::Applied { .. }) => {
                    applied = true;
                    break;
                }
                Ok(gs_service::gs_ephemeral::CheckpointCas::Conflict { .. }) => continue,
                Err(_) => break,
            }
        }
        if !applied {
            metrics::counter!("gs_track_state_conflict_total").increment(1);
        }
    }

    let mut response = (
        StatusCode::OK,
        Json(IngestResponse {
            session_id: body.session_id,
            results: outcome.results,
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

fn publish_error(error: &gs_service::publishing::PublishError, request_id: &str) -> Response {
    use gs_service::gs_domain::error::AsErrorCode;
    let code = error.error_code();
    let retry_after = match error {
        gs_service::publishing::PublishError::RateLimited { retry_after_ms }
        | gs_service::publishing::PublishError::TemporarilyUnavailable { retry_after_ms } => {
            Some(*retry_after_ms)
        }
        _ => None,
    };
    let problem = Problem::new(code, request_id).with_detail(error.to_string());
    let mut response = ProblemResponse::new(problem).into_response();
    if let Some(ms) = retry_after {
        // `Retry-After` in whole seconds, rounded up.
        let seconds = ms.div_ceil(1000).max(1);
        if let Ok(value) = axum::http::HeaderValue::from_str(&seconds.to_string()) {
            response.headers_mut().insert("retry-after", value);
        }
    }
    response
}
