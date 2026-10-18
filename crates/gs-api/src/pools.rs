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

/// Pool query limits from the configuration.
#[derive(Debug, Clone, Copy)]
pub struct PoolsLimits {
    /// `pool_query_max_radius_m`.
    pub max_radius_m: f64,
    /// ETA speed used by `rank = eta` (DRIVING).
    pub eta_speed_mps: f64,
    /// Circuity factor of the own estimate.
    pub eta_circuity: f64,
}

impl Default for PoolsLimits {
    fn default() -> Self {
        Self {
            max_radius_m: DEFAULT_POOL_MAX_RADIUS_M,
            eta_speed_mps: 6.9,
            eta_circuity: gs_mobility::ETA_DEFAULT_CIRCUITY,
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
    // Application only, scope `pools:query`.
    let tenant_id = match &caller {
        CallerPrincipal::Application(app) => {
            if !app.api_key.scopes.iter().any(|s| s == "pools:query") {
                return problem(
                    ErrorCode::InsufficientScope,
                    &request_id,
                    "application requires pools:query scope",
                );
            }
            app.tenant.id
        }
        CallerPrincipal::Device(_) => {
            return problem(
                ErrorCode::PermissionDenied,
                &request_id,
                "devices cannot query a pool",
            );
        }
    };

    if let Some(resp) = require_pools_allowed(&state, tenant_id, &request_id).await {
        return resp;
    }

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

    // Parameter validation (OpenAPI).
    if !query.lat.is_finite()
        || !query.lon.is_finite()
        || !(-90.0..=90.0).contains(&query.lat)
        || !(-180.0..=180.0).contains(&query.lon)
    {
        return problem(ErrorCode::ValidationFailed, &request_id, "invalid lat/lon");
    }
    if !(query.radius_m.is_finite() && query.radius_m >= 1.0)
        || query.radius_m > state.limits.max_radius_m
    {
        return problem(
            ErrorCode::ValidationFailed,
            &request_id,
            "radius_m out of range",
        );
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
        Some(_) => {
            return problem(ErrorCode::ValidationFailed, &request_id, "invalid rank");
        }
    };

    // Attribute filter: up to 3 `k=v` pairs separated by `&`.
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

    // The session must belong to the tenant (404 otherwise).
    let session = match gs_service::gs_store::repos::SessionRepository::get_by_id(
        &state.sessions.pool,
        tenant_id,
        session_id,
    )
    .await
    {
        Ok(Some(session)) => session,
        Ok(None) => return problem(ErrorCode::NotFound, &request_id, "session not found"),
        Err(_) => {
            return problem(ErrorCode::Internal, &request_id, "failed to load session");
        }
    };
    let _ = session;

    // Read the live index from the snapshots of the active members.
    let page = match MembershipRepository::list_active_by_session(
        &state.sessions.pool,
        tenant_id,
        session_id,
        1000,
        None,
    )
    .await
    {
        Ok(page) => page,
        Err(_) => {
            return problem(ErrorCode::Internal, &request_id, "failed to load members");
        }
    };

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
        let Some(sample) = update.sample.as_ref() else {
            continue;
        };
        let Some(position) = sample.position.as_ref() else {
            continue;
        };
        let age_ms = update
            .received_at
            .as_ref()
            .and_then(|ts| u64::try_from(ts.seconds).ok())
            .map(|s| now_ms.saturating_sub(s * 1000))
            .unwrap_or(0);
        members.push(gs_pools::PoolMember {
            member_id: m.membership.id.to_string(),
            lat: position.latitude,
            lon: position.longitude,
            accuracy_m: position.horizontal_accuracy_m,
            availability: gs_pools::Availability::Available,
            attributes: Vec::new(),
            connected: true,
            age_ms,
        });
    }

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
    let data = found
        .into_iter()
        .map(|m| {
            let accuracy = members
                .iter()
                .find(|mm| mm.member_id == m.member_id)
                .map(|mm| mm.accuracy_m)
                .unwrap_or(0.0);
            NearbyMemberDto {
                member_id: m.member_id,
                distance_m: m.distance_m,
                age_ms: m.age_ms,
                availability: "AVAILABLE".to_string(),
                attributes: m.attributes.into_iter().collect(),
                position: PositionDto {
                    latitude: m.lat,
                    longitude: m.lon,
                    horizontal_accuracy_m: accuracy,
                },
                eta_s: m.eta_s,
                mode: m.mode.map(str::to_string),
            }
        })
        .collect();

    json_ok(&request_id, NearbyResponse { data })
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
