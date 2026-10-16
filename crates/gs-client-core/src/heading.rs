//! Circular heading utilities.
//!
//! Headings are angles in degrees and cannot be averaged arithmetically:
//! the mean of `350°` and `10°` is not `180°`. Everything here uses the vector
//! mean; the result is always in `[0, 360)`.

#![forbid(unsafe_code)]

/// Plain mean of headings expressed in degrees (`[0, 360)`).
///
/// Gives equal weight to every sample, even when the head count is 1.
#[must_use]
pub fn circular_mean_degrees(headings_deg: &[f64]) -> Option<f64> {
    if headings_deg.is_empty() {
        return None;
    }
    let mut sx = 0.0_f64;
    let mut sy = 0.0_f64;
    for h in headings_deg {
        if !h.is_finite() {
            continue;
        }
        let r = h.to_radians();
        sx += r.sin();
        sy += r.cos();
    }
    if sx.abs() < f64::EPSILON && sy.abs() < f64::EPSILON {
        return None; // uniform spread ⟹ undefined direction
    }
    let mut deg = sx.atan2(sy).to_degrees().rem_euclid(360.0);
    if deg > 360.0 - 1e-9 {
        deg = 0.0;
    }
    if deg < 1e-9 {
        deg = 0.0;
    }
    Some(deg)
}

/// Exponential heading smoother: `new = atan2(s·sin prev + (1−s)·sin cur, …)`.\n/// Called each fix; `smoothing` in `[0,1]` is the inertia of the previous value.
#[must_use]
pub fn smooth_heading(prev_deg: f64, cur_deg: f64, smoothing: f64) -> f64 {
    let s = smoothing.clamp(0.0, 1.0);
    let p = prev_deg.to_radians();
    let c = cur_deg.to_radians();
    let (sin_p, cos_p) = p.sin_cos();
    let (sin_c, cos_c) = c.sin_cos();
    let sx = s * sin_p + (1.0 - s) * sin_c;
    let sy = s * cos_p + (1.0 - s) * cos_c;
    if sx.abs() < f64::EPSILON && sy.abs() < f64::EPSILON {
        // Fully cancelling votes: no reliable direction; hold last.
        prev_deg.rem_euclid(360.0)
    } else {
        let mut deg = sx.atan2(sy).to_degrees().rem_euclid(360.0);
        if deg > 360.0 - 1e-9 {
            deg = 0.0;
        }
        deg
    }
}

/// Shortest signed angular delta `a -> b` in degrees, in `[-180, 180]`.
#[must_use]
pub fn angle_delta_deg(from_deg: f64, to_deg: f64) -> f64 {
    let d = (to_deg - from_deg).rem_euclid(360.0);
    if d > 180.0 { d - 360.0 } else { d }
}
