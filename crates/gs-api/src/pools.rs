//! `GET /v1/pools/{session_id}/nearby` (docs/23 §3.1, OpenAPI `getPoolNearby`).
//!
//! Application-only, scope `pools:query`, tenant policy `pools_allowed`.
//!
//! The live index is read from the ephemeral snapshots the ingest writes
//! (`{p}:snap:{session}:{member}`). A member appears only if it has a snapshot
//! **and** is sharing: a member that is not publishing has no position to offer
//! and is therefore not an option. `availability` is derived from that same
//! fact (`AVAILABLE` = has a live position and is sharing) until the member
//! status projection of T-E4-05 stores it explicitly.
#![forbid(unsafe_code)]
// The helpers return `Result<_, Response>`; `Response` is large by nature
// (axum's type) and boxing it would only move the allocation, not remove it.
#![allow(clippy::result_large_err)]

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use gs_service::EphemeralStore as _;
use gs_service::gs_domain::error::ErrorCode;
use gs_service::gs_domain::ids::SessionId;
use gs_service::gs_store::repos::{MembershipRepository, TenantRepository};
use prost::Message as _;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::sessions::{CallerPrincipal, SessionApiState, authenticate_caller};
use crate::{Problem, ProblemResponse, request_id_from};

/// Default radius cap (`pool_query_max_radius_m`, `17` §4).
pub const DEFAULT_POOL_MAX_RADIUS_M: f64 = 20_000.0;
/// Default limit of the `nearby` query.
pub const DEFAULT_NEARBY_LIMIT: usize = 20;
/// Maximum `limit` accepted.
pub const MAX_NEARBY_LIMIT: usize = 100;
/// Default H3 resolution of density markers (`pool_density_resolution`).
pub const DEFAULT_DENSITY_RESOLUTION: u8 = 9;
/// Maximum density markers (`pool_density_max_markers`).
pub const DEFAULT_DENSITY_MAX_MARKERS: usize = 15;

/// Pool query limits from the configuration.
#[derive(Debug, Clone, Copy)]
pub struct PoolsLimits {
    /// `pool_query_max_radius_m`.
    pub max_radius_m: f64,
    /// ETA speed used by `rank = eta` (DRIVING).
    pub eta_speed_mps: f64,
    /// Circuity factor of the own estimate.
    pub eta_circuity: f64,
    /// `pool_density_delay_s`: minimum delay before showing a marker.
    pub density_delay_s: u64,
}

impl Default for PoolsLimits {
    fn default() -> Self {
        Self {
            max_radius_m: DEFAULT_POOL_MAX_RADIUS_M,
            eta_speed_mps: 6.9,
            eta_circuity: gs_mobility::ETA_DEFAULT_CIRCUITY,
            density_delay_s: 10,
        }
    }
}

/// State for the pool endpoints.
#[derive(Clone)]
pub struct PoolsState {
    /// Shared authentication/rate-limit state.
    pub sessions: SessionApiState,
    /// Query limits.
    pub limits: PoolsLimits,
}

/// Mounts `/v1/pools/*`.
pub fn pools_router(state: PoolsState) -> axum::Router {
    axum::Router::new()
        .route(
            "/v1/pools/{session_id}/nearby",
            axum::routing::get(nearby_handler),
        )
        .route(
            "/v1/pools/{session_id}/rings",
            axum::routing::get(rings_handler),
        )
        .route(
            "/v1/pools/{session_id}/density",
            axum::routing::get(density_handler),
        )
        .route(
            "/v1/pools/{session_id}/members/{member_id}/availability",
            axum::routing::put(set_availability_handler),
        )
        .route(
            "/v1/pools/{session_id}/members/{member_id}/attributes",
            axum::routing::put(set_attributes_handler),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
pub struct NearbyQuery {
    pub lat: f64,
    pub lon: f64,
    pub radius_m: f64,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub rank: Option<String>,
    #[serde(default)]
    pub max_age_s: Option<u64>,
    #[serde(rename = "filter.attributes", default)]
    pub filter_attributes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PositionDto {
    pub latitude: f64,
    pub longitude: f64,
    pub horizontal_accuracy_m: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct NearbyMemberDto {
    pub member_id: String,
    pub distance_m: f64,
    pub age_ms: u64,
    pub availability: String,
    pub attributes: std::collections::BTreeMap<String, String>,
    pub position: PositionDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NearbyResponse {
    pub data: Vec<NearbyMemberDto>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RingDto {
    pub ring: u32,
    pub inner_radius_m: f64,
    pub outer_radius_m: f64,
    pub truncated: bool,
    pub members: Vec<NearbyMemberDto>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RingsResponse {
    pub rings: Vec<RingDto>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DensityMarkerDto {
    pub position: PositionDto,
    pub count: u32,
    pub resolution: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading_sector: Option<u8>,
    pub delay_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DensityResponse {
    pub markers: Vec<DensityMarkerDto>,
}

/// Authorizes a pool query and returns the tenant.
///
/// `app_scope` is required for application principals; `device_scope` is the
/// scope a device must hold (`None` means devices are forbidden).
async fn authorize(
    state: &PoolsState,
    caller: &CallerPrincipal,
    request_id: &str,
    app_scope: &str,
    device_scope: Option<&str>,
) -> Result<gs_service::gs_domain::ids::TenantId, Response> {
    let tenant_id = match caller {
        CallerPrincipal::Application(app) => {
            if !app.api_key.scopes.iter().any(|s| s == app_scope) {
                return Err(problem(
                    ErrorCode::InsufficientScope,
                    request_id,
                    "application lacks the required pool scope",
                ));
            }
            app.tenant.id
        }
        CallerPrincipal::Device(dev) => match device_scope {
            Some(scope) if dev.claims.has_scope(scope) => dev.tenant.id,
            Some(_) => {
                return Err(problem(
                    ErrorCode::InsufficientScope,
                    request_id,
                    "device lacks the required pool scope",
                ));
            }
            None => {
                return Err(problem(
                    ErrorCode::PermissionDenied,
                    request_id,
                    "devices cannot query this pool endpoint",
                ));
            }
        },
    };
    if let Some(resp) = require_pools_allowed(state, tenant_id, request_id).await {
        return Err(resp);
    }
    Ok(tenant_id)
}

/// Reads the live index of a session from the ephemeral snapshots.
async fn load_live_members(
    state: &PoolsState,
    tenant_id: gs_service::gs_domain::ids::TenantId,
    session_id: SessionId,
    request_id: &str,
) -> Result<Vec<gs_pools::PoolMember>, Response> {
    let page = MembershipRepository::list_active_by_session(
        &state.sessions.pool,
        tenant_id,
        session_id,
        1000,
        None,
    )
    .await
    .map_err(|_| problem(ErrorCode::Internal, request_id, "failed to load members"))?;

    let now_ms = (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as u64;
    let prefix = &state.sessions.redis_key_prefix;
    let mut members = Vec::new();
    for m in &page.members {
        if !m.membership.sharing {
            continue;
        }
        let key = gs_service::publishing::snapshot_key(prefix, session_id, m.membership.id);
        let Ok(Some(bytes)) = state.sessions.ephemeral.get_snapshot(&key).await else {
            continue;
        };
        let Ok(update) = gs_protocol::LocationUpdate::decode(bytes.as_slice()) else {
            continue;
        };
        let Some(position) = update.sample.as_ref().and_then(|s| s.position.as_ref()) else {
            continue;
        };
        let age_ms = update
            .received_at
            .as_ref()
            .and_then(|ts| u64::try_from(ts.seconds).ok())
            .map(|s| now_ms.saturating_sub(s * 1000))
            .unwrap_or(0);
        let member_id = m.membership.id.to_string();
        let member_state = load_member_state(state, session_id, &member_id).await;
        let availability = match member_state.availability.as_deref() {
            Some("BUSY") => gs_pools::Availability::Busy,
            Some("PAUSED") => gs_pools::Availability::Paused,
            _ => gs_pools::Availability::Available,
        };
        members.push(gs_pools::PoolMember {
            member_id,
            lat: position.latitude,
            lon: position.longitude,
            accuracy_m: position.horizontal_accuracy_m,
            availability,
            attributes: member_state.attributes.into_iter().collect(),
            connected: true,
            age_ms,
        });
    }
    Ok(members)
}

/// Validates the common centre/radius parameters.
fn validate_center(
    lat: f64,
    lon: f64,
    radius_m: f64,
    max_radius_m: f64,
) -> Result<(), &'static str> {
    if !lat.is_finite()
        || !lon.is_finite()
        || !(-90.0..=90.0).contains(&lat)
        || !(-180.0..=180.0).contains(&lon)
    {
        return Err("invalid lat/lon");
    }
    if !(radius_m.is_finite() && radius_m >= 1.0) || radius_m > max_radius_m {
        return Err("radius_m out of range");
    }
    Ok(())
}

async fn nearby_handler(
    State(state): State<PoolsState>,
    Path(session_id_raw): Path<String>,
    Query(query): Query<NearbyQuery>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state.sessions, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };
    let tenant_id = match authorize(&state, &caller, &request_id, "pools:query", None).await {
        Ok(t) => t,
        Err(resp) => return resp,
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
    if let Err(detail) = validate_center(
        query.lat,
        query.lon,
        query.radius_m,
        state.limits.max_radius_m,
    ) {
        return problem(ErrorCode::ValidationFailed, &request_id, detail);
    }
    let limit = query.limit.unwrap_or(DEFAULT_NEARBY_LIMIT);
    if limit == 0 || limit > MAX_NEARBY_LIMIT {
        return problem(
            ErrorCode::ValidationFailed,
            &request_id,
            "limit out of range",
        );
    }
    let rank = match query.rank.as_deref() {
        None | Some("distance") => gs_pools::Rank::Distance,
        Some("eta") => gs_pools::Rank::Eta,
        Some(_) => return problem(ErrorCode::ValidationFailed, &request_id, "invalid rank"),
    };
    let attributes = match parse_attributes(query.filter_attributes.as_deref()) {
        Ok(a) => a,
        Err(()) => {
            return problem(
                ErrorCode::ValidationFailed,
                &request_id,
                "filter.attributes must be up to 3 k=v pairs",
            );
        }
    };

    if let Err(resp) = ensure_session(&state, tenant_id, session_id, &request_id).await {
        return resp;
    }
    let members = match load_live_members(&state, tenant_id, session_id, &request_id).await {
        Ok(m) => m,
        Err(resp) => return resp,
    };

    let params = gs_pools::NearbyParams {
        lat: query.lat,
        lon: query.lon,
        radius_m: query.radius_m,
        limit,
        rank,
        max_age_s: query.max_age_s.unwrap_or(0),
        attributes,
        eta_speed_mps: state.limits.eta_speed_mps,
        eta_circuity: state.limits.eta_circuity,
    };
    let found = gs_pools::nearby(&members, &params);
    let data = found.into_iter().map(|m| nearby_dto(m, &members)).collect();
    json_ok(&request_id, NearbyResponse { data })
}

fn nearby_dto(m: gs_pools::NearbyMember, members: &[gs_pools::PoolMember]) -> NearbyMemberDto {
    let accuracy = members
        .iter()
        .find(|mm| mm.member_id == m.member_id)
        .map(|mm| mm.accuracy_m)
        .unwrap_or(0.0);
    NearbyMemberDto {
        member_id: m.member_id,
        distance_m: m.distance_m,
        age_ms: m.age_ms,
        availability: availability_str(m.availability).to_string(),
        attributes: m.attributes.into_iter().collect(),
        position: PositionDto {
            latitude: m.lat,
            longitude: m.lon,
            horizontal_accuracy_m: accuracy,
        },
        eta_s: m.eta_s,
        mode: m.mode.map(str::to_string),
    }
}

async fn ensure_session(
    state: &PoolsState,
    tenant_id: gs_service::gs_domain::ids::TenantId,
    session_id: SessionId,
    request_id: &str,
) -> Result<(), Response> {
    match gs_service::gs_store::repos::SessionRepository::get_by_id(
        &state.sessions.pool,
        tenant_id,
        session_id,
    )
    .await
    {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(problem(
            ErrorCode::NotFound,
            request_id,
            "session not found",
        )),
        Err(_) => Err(problem(
            ErrorCode::Internal,
            request_id,
            "failed to load session",
        )),
    }
}

#[derive(Debug, Deserialize)]
pub struct RingsQuery {
    pub lat: f64,
    pub lon: f64,
    #[serde(default)]
    pub ring_step_m: Option<f64>,
    #[serde(default)]
    pub k: Option<u32>,
    #[serde(default)]
    pub per_ring_limit: Option<usize>,
}

async fn rings_handler(
    State(state): State<PoolsState>,
    Path(session_id_raw): Path<String>,
    Query(query): Query<RingsQuery>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state.sessions, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };
    let tenant_id = match authorize(&state, &caller, &request_id, "pools:query", None).await {
        Ok(t) => t,
        Err(resp) => return resp,
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
    // Exactly one of ring_step_m / k (OpenAPI).
    match (query.ring_step_m, query.k) {
        (Some(_), Some(_)) | (None, None) => {
            return problem(
                ErrorCode::ValidationFailed,
                &request_id,
                "exactly one of ring_step_m or k is required",
            );
        }
        _ => {}
    }
    if let Some(step) = query.ring_step_m
        && !(step.is_finite() && step >= 1.0)
    {
        return problem(
            ErrorCode::ValidationFailed,
            &request_id,
            "ring_step_m out of range",
        );
    }
    if let Some(k) = query.k
        && k == 0
    {
        return problem(ErrorCode::ValidationFailed, &request_id, "k out of range");
    }
    let per_ring_limit = query.per_ring_limit.unwrap_or(DEFAULT_NEARBY_LIMIT);
    if per_ring_limit == 0 || per_ring_limit > MAX_NEARBY_LIMIT {
        return problem(
            ErrorCode::ValidationFailed,
            &request_id,
            "per_ring_limit out of range",
        );
    }
    if let Err(detail) = validate_center(
        query.lat,
        query.lon,
        state.limits.max_radius_m,
        state.limits.max_radius_m,
    ) {
        return problem(ErrorCode::ValidationFailed, &request_id, detail);
    }

    if let Err(resp) = ensure_session(&state, tenant_id, session_id, &request_id).await {
        return resp;
    }
    let members = match load_live_members(&state, tenant_id, session_id, &request_id).await {
        Ok(m) => m,
        Err(resp) => return resp,
    };

    let params = gs_pools::NearbyParams {
        lat: query.lat,
        lon: query.lon,
        radius_m: state.limits.max_radius_m,
        limit: usize::MAX,
        rank: gs_pools::Rank::Distance,
        max_age_s: 0,
        attributes: Vec::new(),
        eta_speed_mps: state.limits.eta_speed_mps,
        eta_circuity: state.limits.eta_circuity,
    };
    let rings = gs_pools::rings(
        &members,
        &params,
        query.ring_step_m,
        query.k,
        per_ring_limit,
    );
    let rings = rings
        .into_iter()
        .map(|r| RingDto {
            ring: r.ring,
            inner_radius_m: r.inner_radius_m,
            outer_radius_m: r.outer_radius_m,
            truncated: r.truncated,
            members: r
                .members
                .into_iter()
                .map(|m| nearby_dto(m, &members))
                .collect(),
        })
        .collect();
    json_ok(&request_id, RingsResponse { rings })
}

#[derive(Debug, Deserialize)]
pub struct DensityQuery {
    pub lat: f64,
    pub lon: f64,
    pub radius_m: f64,
    #[serde(default)]
    pub resolution: Option<u8>,
}

async fn density_handler(
    State(state): State<PoolsState>,
    Path(session_id_raw): Path<String>,
    Query(query): Query<DensityQuery>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state.sessions, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };
    // Application `pools:query` OR device `pools:density` (the only pool query a
    // device may run; it never exposes individual positions).
    let tenant_id = match authorize(
        &state,
        &caller,
        &request_id,
        "pools:query",
        Some("pools:density"),
    )
    .await
    {
        Ok(t) => t,
        Err(resp) => return resp,
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
    if let Err(detail) = validate_center(
        query.lat,
        query.lon,
        query.radius_m,
        state.limits.max_radius_m,
    ) {
        return problem(ErrorCode::ValidationFailed, &request_id, detail);
    }
    let resolution = query.resolution.unwrap_or(DEFAULT_DENSITY_RESOLUTION);
    if !(7..=10).contains(&resolution) {
        return problem(
            ErrorCode::ValidationFailed,
            &request_id,
            "resolution out of range",
        );
    }

    if let Err(resp) = ensure_session(&state, tenant_id, session_id, &request_id).await {
        return resp;
    }
    let members = match load_live_members(&state, tenant_id, session_id, &request_id).await {
        Ok(m) => m,
        Err(resp) => return resp,
    };

    let params = gs_pools::NearbyParams {
        lat: query.lat,
        lon: query.lon,
        radius_m: query.radius_m,
        limit: usize::MAX,
        rank: gs_pools::Rank::Distance,
        max_age_s: 0,
        attributes: Vec::new(),
        eta_speed_mps: state.limits.eta_speed_mps,
        eta_circuity: state.limits.eta_circuity,
    };
    let markers = gs_pools::density(
        &members,
        &params,
        resolution,
        DEFAULT_DENSITY_MAX_MARKERS,
        state.limits.density_delay_s,
    );
    let markers = markers
        .into_iter()
        .map(|m| DensityMarkerDto {
            position: PositionDto {
                latitude: m.lat,
                longitude: m.lon,
                horizontal_accuracy_m: h3_cell_radius_m(m.resolution),
            },
            count: m.count,
            resolution: m.resolution,
            heading_sector: m.heading_sector,
            delay_ms: m.delay_ms,
        })
        .collect();
    json_ok(&request_id, DensityResponse { markers })
}

/// Average H3 edge length (metres) per resolution, used as the marker's radius.
///
/// These are the documented average edge lengths of the H3 grid; a density
/// marker represents a cell, so the cell size is its honest uncertainty.
fn availability_str(a: gs_pools::Availability) -> &'static str {
    match a {
        gs_pools::Availability::Available => "AVAILABLE",
        gs_pools::Availability::Busy => "BUSY",
        gs_pools::Availability::Paused => "PAUSED",
        gs_pools::Availability::Unspecified => "AVAILABLE",
    }
}

fn h3_cell_radius_m(resolution: u8) -> f64 {
    match resolution {
        7 => 1_220.0,
        8 => 461.0,
        9 => 174.0,
        _ => 66.0,
    }
}

/// Per-member pool state (availability + attributes) stored in the ephemeral
/// store at `{p}:poolm:{session}:{member}`. It is only ever projected to
/// application principals (INV-PRS-2): a device never sees another driver's
/// availability or attributes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PoolMemberState {
    /// `AVAILABLE`, `BUSY` or `PAUSED`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability: Option<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub attributes: std::collections::BTreeMap<String, String>,
}

fn pool_member_key(prefix: &str, session_id: SessionId, member_id: &str) -> String {
    format!("{prefix}:poolm:{session_id}:{member_id}")
}

async fn load_member_state(
    state: &PoolsState,
    session_id: SessionId,
    member_id: &str,
) -> PoolMemberState {
    let key = pool_member_key(&state.sessions.redis_key_prefix, session_id, member_id);
    match state.sessions.ephemeral.get_snapshot(&key).await {
        Ok(Some(bytes)) => serde_json::from_slice(&bytes).unwrap_or_default(),
        _ => PoolMemberState::default(),
    }
}

async fn store_member_state(
    state: &PoolsState,
    session_id: SessionId,
    member_id: &str,
    value: &PoolMemberState,
) -> bool {
    let key = pool_member_key(&state.sessions.redis_key_prefix, session_id, member_id);
    let Ok(bytes) = serde_json::to_vec(value) else {
        return false;
    };
    // TTL of the live index: pruned with the snapshot (docs/11 §3.1).
    state
        .sessions
        .ephemeral
        .set_snapshot(&key, &bytes, std::time::Duration::from_secs(86_400))
        .await
        .is_ok()
}

/// Response of the pool member endpoints: the full `Member` plus the two
/// fields that only exist in a `POOL` session (OpenAPI `Member`).
#[derive(Debug, Clone, Serialize)]
pub struct PoolMemberResponse {
    #[serde(flatten)]
    pub member: gs_service::members::MemberResource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability: Option<String>,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub attributes: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct SetAvailabilityRequest {
    pub availability: String,
}

#[derive(Debug, Deserialize)]
pub struct SetAttributesRequest {
    pub attributes: std::collections::BTreeMap<String, String>,
}

/// Resolves the member id a caller may act on: an application with
/// `pools:manage` may act on any member; a device only on its own (`me`).
async fn resolve_target_member(
    state: &PoolsState,
    caller: &CallerPrincipal,
    session_id: SessionId,
    member_id_raw: &str,
    request_id: &str,
) -> Result<gs_service::gs_domain::ids::MemberId, Response> {
    match caller {
        CallerPrincipal::Application(_) => {
            gs_service::gs_domain::ids::MemberId::from_str(member_id_raw).map_err(|_| {
                problem(
                    ErrorCode::ValidationFailed,
                    request_id,
                    "malformed member id",
                )
            })
        }
        CallerPrincipal::Device(dev) => {
            let own = gs_service::members::get_member_for_device_me(
                &state.sessions.pool,
                dev.tenant.id,
                session_id,
                dev.subject.id,
            )
            .await
            .map_err(|_| problem(ErrorCode::NotFound, request_id, "member not found"))?;
            if member_id_raw != "me" && member_id_raw != own.id {
                return Err(problem(
                    ErrorCode::PermissionDenied,
                    request_id,
                    "a device may only change its own availability",
                ));
            }
            gs_service::gs_domain::ids::MemberId::from_str(&own.id)
                .map_err(|_| problem(ErrorCode::Internal, request_id, "malformed own member id"))
        }
    }
}

async fn set_availability_handler(
    State(state): State<PoolsState>,
    Path((session_id_raw, member_id_raw)): Path<(String, String)>,
    headers: HeaderMap,
    body: Result<Json<SetAvailabilityRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state.sessions, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };
    let tenant_id = match authorize(
        &state,
        &caller,
        &request_id,
        "pools:manage",
        Some("sharing:write"),
    )
    .await
    {
        Ok(t) => t,
        Err(resp) => return resp,
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
                "body must be a SetAvailabilityRequest",
            );
        }
    };
    if !matches!(body.availability.as_str(), "AVAILABLE" | "BUSY" | "PAUSED") {
        return problem(
            ErrorCode::ValidationFailed,
            &request_id,
            "invalid availability",
        );
    }
    if let Err(resp) = ensure_session(&state, tenant_id, session_id, &request_id).await {
        return resp;
    }
    let member_id =
        match resolve_target_member(&state, &caller, session_id, &member_id_raw, &request_id).await
        {
            Ok(id) => id,
            Err(resp) => return resp,
        };

    let mut member_state = load_member_state(&state, session_id, &member_id.to_string()).await;
    member_state.availability = Some(body.availability);
    if !store_member_state(&state, session_id, &member_id.to_string(), &member_state).await {
        return problem(
            ErrorCode::TemporarilyUnavailable,
            &request_id,
            "failed to store availability",
        );
    }
    respond_member(
        &state,
        tenant_id,
        session_id,
        member_id,
        member_state,
        &request_id,
    )
    .await
}

async fn set_attributes_handler(
    State(state): State<PoolsState>,
    Path((session_id_raw, member_id_raw)): Path<(String, String)>,
    headers: HeaderMap,
    body: Result<Json<SetAttributesRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state.sessions, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };
    // Attributes are application-only (`pools:manage`).
    let tenant_id = match authorize(&state, &caller, &request_id, "pools:manage", None).await {
        Ok(t) => t,
        Err(resp) => return resp,
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
                "body must be a SetAttributesRequest",
            );
        }
    };
    if body.attributes.len() > 16 {
        return problem(
            ErrorCode::ValidationFailed,
            &request_id,
            "too many attributes",
        );
    }
    for (k, v) in &body.attributes {
        if !valid_attribute_key(k) || v.len() > 64 {
            return problem(
                ErrorCode::ValidationFailed,
                &request_id,
                "attribute key must match ^[a-z0-9_]{1,32}$ and value <= 64",
            );
        }
    }
    if let Err(resp) = ensure_session(&state, tenant_id, session_id, &request_id).await {
        return resp;
    }
    let member_id =
        match resolve_target_member(&state, &caller, session_id, &member_id_raw, &request_id).await
        {
            Ok(id) => id,
            Err(resp) => return resp,
        };

    let mut member_state = load_member_state(&state, session_id, &member_id.to_string()).await;
    member_state.attributes = body.attributes;
    if !store_member_state(&state, session_id, &member_id.to_string(), &member_state).await {
        return problem(
            ErrorCode::TemporarilyUnavailable,
            &request_id,
            "failed to store attributes",
        );
    }
    respond_member(
        &state,
        tenant_id,
        session_id,
        member_id,
        member_state,
        &request_id,
    )
    .await
}

fn valid_attribute_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 32
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

async fn respond_member(
    state: &PoolsState,
    tenant_id: gs_service::gs_domain::ids::TenantId,
    session_id: SessionId,
    member_id: gs_service::gs_domain::ids::MemberId,
    member_state: PoolMemberState,
    request_id: &str,
) -> Response {
    match gs_service::members::get_member(&state.sessions.pool, tenant_id, session_id, member_id)
        .await
    {
        Ok(member) => json_ok(
            request_id,
            PoolMemberResponse {
                member,
                availability: member_state.availability,
                attributes: member_state.attributes,
            },
        ),
        Err(_) => problem(ErrorCode::NotFound, request_id, "member not found"),
    }
}

fn parse_attributes(raw: Option<&str>) -> Result<Vec<(String, String)>, ()> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for pair in raw.split('&') {
        let (k, v) = pair.split_once('=').ok_or(())?;
        if k.is_empty() || v.is_empty() || v.len() > 64 {
            return Err(());
        }
        out.push((k.to_string(), v.to_string()));
    }
    if out.len() > 3 {
        return Err(());
    }
    Ok(out)
}

async fn require_pools_allowed(
    state: &PoolsState,
    tenant_id: gs_service::gs_domain::ids::TenantId,
    request_id: &str,
) -> Option<Response> {
    let tenant = match TenantRepository::get_by_id(&state.sessions.pool, tenant_id).await {
        Ok(Some(t)) => t,
        Ok(None) => return Some(problem(ErrorCode::NotFound, request_id, "tenant not found")),
        Err(_) => {
            return Some(problem(
                ErrorCode::Internal,
                request_id,
                "failed to load tenant policy",
            ));
        }
    };
    match gs_service::gs_auth::TenantPolicy::from_json_value(tenant.policy.clone()) {
        Ok(policy) if policy.pools_allowed => None,
        Ok(_) => Some(problem(
            ErrorCode::FeatureDisabled,
            request_id,
            "pools are disabled for this tenant",
        )),
        Err(_) => Some(problem(
            ErrorCode::Internal,
            request_id,
            "invalid tenant policy",
        )),
    }
}

fn problem(code: ErrorCode, request_id: &str, detail: &str) -> Response {
    let problem = Problem::new(code, request_id).with_detail(detail);
    ProblemResponse::new(problem).into_response()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_attributes_accepts_up_to_three_pairs() {
        assert_eq!(
            parse_attributes(Some("vehicle_type=van&seats=4")),
            Ok(vec![
                ("vehicle_type".to_string(), "van".to_string()),
                ("seats".to_string(), "4".to_string())
            ])
        );
        assert_eq!(parse_attributes(None), Ok(Vec::new()));
    }

    #[test]
    fn parse_attributes_rejects_malformed_and_too_many() {
        assert!(parse_attributes(Some("nokeyvalue")).is_err());
        assert!(parse_attributes(Some("a=1&b=2&c=3&d=4")).is_err());
        assert!(parse_attributes(Some("=x")).is_err());
    }
}
