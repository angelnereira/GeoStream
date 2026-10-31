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
use axum::extract::{Path, Query, State};
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
use time::format_description::well_known::Rfc3339;

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
        .route(
            "/v1/sessions/{session_id}/trace",
            axum::routing::get(trace_handler),
        )
        .route("/v1/trips/{trip_id}", axum::routing::get(get_trip))
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
        reject_mock_samples: policy.reject_mock_samples,
        reject_network_only: policy.reject_network_only,
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

#[derive(Debug, Deserialize)]
pub struct TraceQuery {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub simplify_m: Option<f64>,
    #[serde(default)]
    pub format: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TracePointDto {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Serialize)]
pub struct TraceResponse {
    pub session_id: String,
    pub from: String,
    pub to: String,
    pub points: Vec<TracePointDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoded: Option<String>,
}

async fn trace_handler(
    State(state): State<HistoryState>,
    Path(session_id_raw): Path<String>,
    Query(query): Query<TraceQuery>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state.sessions, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };
    // Application `history:read` only (OpenAPI `device: none`).
    let tenant_id = match &caller {
        CallerPrincipal::Application(app) => {
            if !app.api_key.scopes.iter().any(|s| s == "history:read") {
                return problem(
                    ErrorCode::InsufficientScope,
                    &request_id,
                    "application requires history:read scope",
                );
            }
            app.tenant.id
        }
        CallerPrincipal::Device(_) => {
            return problem(
                ErrorCode::PermissionDenied,
                &request_id,
                "devices cannot read a session trace",
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
    let from = match OffsetDateTime::parse(&query.from, &Rfc3339) {
        Ok(t) => t,
        Err(_) => return problem(ErrorCode::ValidationFailed, &request_id, "invalid from"),
    };
    let to = match OffsetDateTime::parse(&query.to, &Rfc3339) {
        Ok(t) => t,
        Err(_) => return problem(ErrorCode::ValidationFailed, &request_id, "invalid to"),
    };
    if to <= from {
        return problem(
            ErrorCode::ValidationFailed,
            &request_id,
            "to must be after from",
        );
    }
    let simplify_m = query.simplify_m.unwrap_or(0.0);
    if !(0.0..=1000.0).contains(&simplify_m) {
        return problem(
            ErrorCode::ValidationFailed,
            &request_id,
            "simplify_m out of range",
        );
    }
    let format = query.format.as_deref().unwrap_or("json");
    if !matches!(format, "json" | "polyline" | "gsp1") {
        return problem(ErrorCode::ValidationFailed, &request_id, "invalid format");
    }

    // Tenant policy: the trace is history, so `history_allowed` must hold.
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

    let raw = match HistoryRepository::read_points(
        &state.sessions.pool,
        tenant_id,
        session_id.into_uuid(),
        from,
        to,
        100_000,
    )
    .await
    {
        Ok(p) => p,
        Err(_) => return problem(ErrorCode::Internal, &request_id, "failed to read trace"),
    };

    let mut points: Vec<gs_geo::LatLon> = raw
        .iter()
        .map(|p| gs_geo::LatLon::new(p.latitude, p.longitude))
        .collect();
    if simplify_m > 0.0 {
        points = gs_geo::simplify_rdp(&points, simplify_m);
    }

    let encoded = match format {
        "polyline" => Some(encode_polyline(&points)),
        "gsp1" => Some(encode_gsp1(&points)),
        _ => None,
    };

    json_ok(
        &request_id,
        TraceResponse {
            session_id: session_id_raw,
            from: query.from,
            to: query.to,
            points: points
                .iter()
                .map(|p| TracePointDto {
                    lat: p.lat,
                    lon: p.lon,
                })
                .collect(),
            encoded,
        },
    )
}

/// Google encoded polyline (precision 5). A widely supported, well-specified
/// format, which is why `polyline` is the interoperable choice.
fn encode_polyline(points: &[gs_geo::LatLon]) -> String {
    let mut out = String::new();
    let mut prev_lat = 0i64;
    let mut prev_lon = 0i64;
    for p in points {
        let lat = (p.lat * 1e5).round() as i64;
        let lon = (p.lon * 1e5).round() as i64;
        encode_value(lat - prev_lat, &mut out);
        encode_value(lon - prev_lon, &mut out);
        prev_lat = lat;
        prev_lon = lon;
    }
    out
}

fn encode_value(value: i64, out: &mut String) {
    let mut v = if value < 0 { !(value << 1) } else { value << 1 };
    while v >= 0x20 {
        out.push(char::from_u32(((0x20 | (v & 0x1f)) + 63) as u32).unwrap_or('?'));
        v >>= 5;
    }
    out.push(char::from_u32((v + 63) as u32).unwrap_or('?'));
}

/// `gsp1`: GeoStream's compact polyline.
///
/// The format is **not specified in the normative documents** (see
/// `DR-20261018-01`); this implementation is the proposal in that request:
/// a `gsp1` tag followed by base64url of varint delta-encoded `lat_e7`/`lon_e7`
/// pairs (degrees × 1e7), matching the `lat_e7`/`lon_e7` columns of
/// `trip_tracks`. It is deterministic and lossless at 1e-7 degrees.
fn encode_gsp1(points: &[gs_geo::LatLon]) -> String {
    let mut bytes = Vec::with_capacity(points.len() * 8);
    bytes.extend_from_slice(b"gsp1");
    let mut prev_lat = 0i64;
    let mut prev_lon = 0i64;
    for p in points {
        let lat = (p.lat * 1e7).round() as i64;
        let lon = (p.lon * 1e7).round() as i64;
        encode_varint(zigzag(lat - prev_lat), &mut bytes);
        encode_varint(zigzag(lon - prev_lon), &mut bytes);
        prev_lat = lat;
        prev_lon = lon;
    }
    base64url(&bytes)
}

fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

fn encode_varint(mut v: u64, out: &mut Vec<u8>) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(n & 0x3f) as usize] as char);
        }
    }
    out
}

#[derive(Debug, Serialize)]
pub struct TripStopDto {
    pub started_at: String,
    pub ended_at: String,
    pub duration_s: i32,
    pub latitude: f64,
    pub longitude: f64,
    pub radius_m: f64,
    pub point_count: i32,
}

#[derive(Debug, Serialize)]
pub struct TripDto {
    pub id: String,
    pub session_id: String,
    pub member_id: String,
    pub started_at: String,
    pub ended_at: String,
    pub duration_s: i32,
    pub distance_m: f64,
    pub point_count: i32,
    pub mode: Option<String>,
    pub stops: Vec<TripStopDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_gsp1: Option<String>,
}

async fn get_trip(
    State(state): State<HistoryState>,
    Path(trip_id_raw): Path<String>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state.sessions, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };
    let tenant_id = match &caller {
        CallerPrincipal::Application(app) => {
            if !app.api_key.scopes.iter().any(|s| s == "history:read") {
                return problem(
                    ErrorCode::InsufficientScope,
                    &request_id,
                    "application requires history:read scope",
                );
            }
            app.tenant.id
        }
        CallerPrincipal::Device(_) => {
            return problem(
                ErrorCode::PermissionDenied,
                &request_id,
                "devices cannot read a trip",
            );
        }
    };
    let trip_id = match gs_service::gs_domain::ids::TripId::from_str(&trip_id_raw) {
        Ok(id) => id,
        Err(_) => {
            return problem(
                ErrorCode::ValidationFailed,
                &request_id,
                "malformed trip id",
            );
        }
    };

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

    let loaded =
        match HistoryRepository::get_trip(&state.sessions.pool, tenant_id, trip_id.into_uuid())
            .await
        {
            Ok(Some(v)) => v,
            Ok(None) => return problem(ErrorCode::NotFound, &request_id, "trip not found"),
            Err(_) => return problem(ErrorCode::Internal, &request_id, "failed to load trip"),
        };
    let (trip, stops, track) = loaded;

    let fmt = |i: OffsetDateTime| i.format(&Rfc3339).unwrap_or_default();
    let track_points: Vec<gs_geo::LatLon> = track
        .iter()
        .map(|(lat, lon)| gs_geo::LatLon::new(*lat, *lon))
        .collect();

    json_ok(
        &request_id,
        TripDto {
            id: gs_service::gs_domain::ids::TripId::from_uuid(trip.id).to_string(),
            session_id: SessionId::from_uuid(trip.session_id).to_string(),
            member_id: gs_service::gs_domain::ids::MemberId::from_uuid(trip.member_id).to_string(),
            started_at: fmt(trip.started_at),
            ended_at: fmt(trip.ended_at),
            duration_s: trip.duration_s,
            distance_m: trip.distance_m,
            point_count: trip.point_count,
            mode: trip.mode,
            stops: stops
                .into_iter()
                .map(|s| TripStopDto {
                    started_at: fmt(s.started_at),
                    ended_at: fmt(s.ended_at),
                    duration_s: s.duration_s,
                    latitude: s.latitude,
                    longitude: s.longitude,
                    radius_m: s.radius_m,
                    point_count: s.point_count,
                })
                .collect(),
            track_gsp1: (!track_points.is_empty()).then(|| encode_gsp1(&track_points)),
        },
    )
}

fn json_ok<T: Serialize>(request_id: &str, body: T) -> Response {
    let mut response = (StatusCode::OK, Json(body)).into_response();
    if let Ok(value) = axum::http::HeaderValue::from_str(request_id) {
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

#[cfg(test)]
mod trace_tests {
    use super::*;

    #[test]
    fn polyline_matches_the_reference_example() {
        // The canonical Google example: three points encode to this string.
        let points = [
            gs_geo::LatLon::new(38.5, -120.2),
            gs_geo::LatLon::new(40.7, -120.95),
            gs_geo::LatLon::new(43.252, -126.453),
        ];
        assert_eq!(encode_polyline(&points), "_p~iF~ps|U_ulLnnqC_mqNvxq`@");
    }

    #[test]
    fn polyline_of_no_points_is_empty() {
        assert_eq!(encode_polyline(&[]), "");
    }

    #[test]
    fn gsp1_has_the_tag_and_is_deterministic() {
        let points = [
            gs_geo::LatLon::new(8.9824, -79.5199),
            gs_geo::LatLon::new(8.9830, -79.5199),
        ];
        let a = encode_gsp1(&points);
        let b = encode_gsp1(&points);
        assert_eq!(a, b);
        // The first four characters are the base64url of "gsp" (the tag spans a
        // chunk boundary with the first coordinate, so only these are fixed).
        assert!(a.starts_with("Z3Nw"), "gsp1 was {a}");
        assert!(
            a.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
    }
}
