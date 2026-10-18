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
    body: Result<Json<IngestRequest>, axum::extract::rejection::JsonRejection>,
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

    let body = match body {
        Ok(Json(b)) => b,
        Err(_) => {
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
        max_future_skew_ms: 0,
        max_event_age_ms: 0,
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

    // Trajectory engine: load the checkpoint, apply, save. The version CAS of
    // `track_cas.lua` is a Redis-backend concern (T-E2-06, pending); with the
    // in-memory store a single instance is the only writer.
    let trk_key = format!("{}:trk:{}", state.sessions.redis_key_prefix, device_id);
    let mut tracker = match state.sessions.ephemeral.get_snapshot(&trk_key).await {
        Ok(Some(bytes)) => gs_track::TrackStateMachine::decode_checkpoint(&bytes)
            .unwrap_or_else(|| gs_track::TrackStateMachine::new(gs_track::TrackConfig::defaults())),
        _ => gs_track::TrackStateMachine::new(gs_track::TrackConfig::defaults()),
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

    // Checkpoint the engine for the next request or a reconnect elsewhere.
    if !outcome.accepted.is_empty() {
        let blob = tracker.encode_checkpoint();
        let _ = state
            .sessions
            .ephemeral
            .set_snapshot(&trk_key, &blob, TRACK_STATE_TTL)
            .await;
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
