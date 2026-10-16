//! Visual dead-reckoning (`22` §3.6).
//!
//! PREDICTED positions never leave the client: returned for painting a marker,
//! never published, stored or logged.

#![forbid(unsafe_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderKind {
    Measured,
    Filtered,
    Predicted,
}

#[derive(Debug, Clone, Copy)]
pub struct PredictionPolicy {
    pub predict_horizon_ms: u64,
    pub predict_blend_ms: u64,
    pub predict_speed_decay_s: f64,
    pub predict_min_speed_mps: f64,
    pub predict_min_plausibility: f64,
    pub predict_uncertainty_growth_mps: f64,
}

impl PredictionPolicy {
    pub fn defaults() -> Self {
        Self {
            predict_horizon_ms: 3000,
            predict_blend_ms: 500,
            predict_speed_decay_s: 2.0,
            predict_min_speed_mps: 1.0,
            predict_min_plausibility: 0.5,
            predict_uncertainty_growth_mps: 2.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LastMeasured {
    pub lat: f64,
    pub lon: f64,
    pub ts_ms: u64,
    pub speed_mps: f64,
    pub heading_deg: f64,
    pub accuracy_m: f64,
    pub plausibility: f64,
    pub stationary: bool,
    pub stale: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct RenderPosition {
    pub lat: f64,
    pub lon: f64,
    pub kind: RenderKind,
    pub confidence_radius_m: f64,
}

/// Paint decision. Returns `None` only when no measured position is available.
pub fn render_position(
    now_ms: u64,
    last: LastMeasured,
    policy: PredictionPolicy,
) -> RenderPosition {
    let age = now_ms.saturating_sub(last.ts_ms);
    if age == 0 || age > policy.predict_horizon_ms {
        return RenderPosition {
            lat: last.lat,
            lon: last.lon,
            kind: RenderKind::Measured,
            confidence_radius_m: last.accuracy_m,
        };
    }
    if last.stationary
        || last.stale
        || last.plausibility < policy.predict_min_plausibility
        || last.speed_mps < policy.predict_min_speed_mps
    {
        return RenderPosition {
            lat: last.lat,
            lon: last.lon,
            kind: RenderKind::Filtered,
            confidence_radius_m: last.accuracy_m,
        };
    }
    let tau_ms = (policy.predict_speed_decay_s * 1000.0).max(1.0);
    let factor = 1.0 - (-(age as f64) / tau_ms).exp();
    let distance_m = last.speed_mps * (age as f64 / 1000.0) * factor;
    let p = gs_geo::destination(
        gs_geo::LatLon::new(last.lat, last.lon),
        last.heading_deg,
        distance_m,
    );
    let confidence =
        last.accuracy_m + policy.predict_uncertainty_growth_mps * (age as f64 / 1000.0);
    RenderPosition {
        lat: p.lat,
        lon: p.lon,
        kind: RenderKind::Predicted,
        confidence_radius_m: confidence,
    }
}
