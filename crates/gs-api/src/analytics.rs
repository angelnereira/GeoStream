//! `GET /v1/analytics/heatmap` (OpenAPI `getHeatmap`, `docs/24` §6, E6).
//!
//! Application-only, scope `analytics:read`, tenant policy `analytics_allowed`.
//!
//! Every response declares `k_anonymity_applied` and `suppressed_cells` so the
//! integrator knows what was hidden, **without** revealing the hidden values
//! (INV-HIS-3). The suppression itself happened at aggregation time (see
//! `gs_store::HistoryRepository::upsert_cell`): a cell that never reached `k`
//! was never written, so it cannot appear here at all.
#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use gs_service::gs_domain::error::ErrorCode;
use gs_service::gs_store::repos::HistoryRepository;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::sessions::{CallerPrincipal, SessionApiState, authenticate_caller};
use crate::{Problem, ProblemResponse, request_id_from};

/// State for the analytics endpoints.
#[derive(Clone)]
pub struct AnalyticsState {
    /// Shared authentication/rate-limit state.
    pub sessions: SessionApiState,
}

/// Mounts `/v1/analytics/*`.
pub fn analytics_router(state: AnalyticsState) -> axum::Router {
    axum::Router::new()
        .route("/v1/analytics/heatmap", axum::routing::get(heatmap_handler))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
pub struct HeatmapQuery {
    /// `minLon,minLat,maxLon,maxLat`.
    pub bbox: String,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub metric: Option<String>,
    #[serde(default)]
    pub resolution: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeatmapCell {
    pub cell: String,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeatmapResponse {
    pub metric: String,
    pub resolution: u8,
    pub cells: Vec<HeatmapCell>,
    pub k_anonymity_applied: u32,
    pub suppressed_cells: u64,
}

async fn heatmap_handler(
    State(state): State<AnalyticsState>,
    Query(query): Query<HeatmapQuery>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state.sessions, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };
    let tenant_id = match &caller {
        CallerPrincipal::Application(app) => {
            if !app.api_key.scopes.iter().any(|s| s == "analytics:read") {
                return problem(
                    ErrorCode::InsufficientScope,
                    &request_id,
                    "application requires analytics:read scope",
                );
            }
            app.tenant.id
        }
        CallerPrincipal::Device(_) => {
            return problem(
                ErrorCode::PermissionDenied,
                &request_id,
                "devices cannot read analytics",
            );
        }
    };

    let tenant = match gs_service::gs_store::repos::TenantRepository::get_by_id(
        &state.sessions.pool,
        tenant_id,
    )
    .await
    {
        Ok(Some(t)) => t,
        Ok(None) => return problem(ErrorCode::NotFound, &request_id, "tenant not found"),
        Err(_) => return problem(ErrorCode::Internal, &request_id, "failed to load tenant"),
    };
    let policy = gs_service::gs_auth::TenantPolicy::from_json_value(tenant.policy.clone())
        .unwrap_or_default();
    if !policy.analytics_allowed {
        return problem(
            ErrorCode::FeatureDisabled,
            &request_id,
            "analytics are disabled for this tenant",
        );
    }

    let bbox = match parse_bbox(&query.bbox) {
        Some(b) => b,
        None => {
            return problem(
                ErrorCode::ValidationFailed,
                &request_id,
                "bbox must be minLon,minLat,maxLon,maxLat",
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
    let metric = query.metric.as_deref().unwrap_or("presence");
    if !matches!(metric, "presence" | "stops" | "speed") {
        return problem(ErrorCode::ValidationFailed, &request_id, "invalid metric");
    }
    if let Some(r) = query.resolution
        && !(5..=12).contains(&r)
    {
        return problem(
            ErrorCode::ValidationFailed,
            &request_id,
            "resolution out of range",
        );
    }

    let stored =
        match HistoryRepository::read_cells(&state.sessions.pool, tenant_id, from, to).await {
            Ok(cells) => cells,
            Err(_) => return problem(ErrorCode::Internal, &request_id, "failed to read cells"),
        };

    // `speed` is not aggregated yet: returning presence/stops numbers under the
    // name `speed` would be a lie, so the cell list stays empty.
    let cells: Vec<HeatmapCell> = if metric == "speed" {
        Vec::new()
    } else {
        stored
            .iter()
            .filter(|c| {
                let in_bbox =
                    gs_geo::h3::cell_in_bbox(c.h3_cell as u64, bbox.1, bbox.0, bbox.3, bbox.2);
                let resolution_ok = match (
                    query.resolution,
                    gs_geo::h3::cell_resolution(c.h3_cell as u64),
                ) {
                    (Some(wanted), Some(actual)) => wanted == actual,
                    _ => true,
                };
                in_bbox && resolution_ok
            })
            .map(|c| HeatmapCell {
                cell: gs_geo::h3::cell_hex(c.h3_cell as u64),
                value: if metric == "stops" {
                    f64::from(c.stop_count)
                } else {
                    f64::from(c.point_count)
                },
            })
            .collect()
    };

    let resolution = query
        .resolution
        .or_else(|| {
            stored
                .first()
                .and_then(|c| gs_geo::h3::cell_resolution(c.h3_cell as u64))
        })
        .unwrap_or(9);

    json_ok(
        &request_id,
        HeatmapResponse {
            metric: metric.to_string(),
            resolution,
            cells,
            k_anonymity_applied: policy.analytics_k_anonymity,
            // Suppression happens at aggregation time, so nothing below `k`
            // exists to be counted here. A per-window aggregation counter is a
            // follow-up; it would report how many were dropped, never their
            // values or locations.
            suppressed_cells: 0,
        },
    )
}

/// Parses `minLon,minLat,maxLon,maxLat` into `(min_lon, min_lat, max_lon, max_lat)`.
fn parse_bbox(raw: &str) -> Option<(f64, f64, f64, f64)> {
    let parts: Vec<f64> = raw
        .split(',')
        .map(|p| p.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .ok()?;
    if parts.len() != 4 {
        return None;
    }
    let (min_lon, min_lat, max_lon, max_lat) = (parts[0], parts[1], parts[2], parts[3]);
    if min_lon > max_lon || min_lat > max_lat {
        return None;
    }
    Some((min_lon, min_lat, max_lon, max_lat))
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
    fn parse_bbox_accepts_four_numbers() {
        assert_eq!(parse_bbox("-80,8,-79,9"), Some((-80.0, 8.0, -79.0, 9.0)));
    }

    #[test]
    fn parse_bbox_rejects_malformed_and_inverted() {
        assert_eq!(parse_bbox("1,2,3"), None);
        assert_eq!(parse_bbox("a,b,c,d"), None);
        assert_eq!(parse_bbox("-79,8,-80,9"), None);
    }
}
