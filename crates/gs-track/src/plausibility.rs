//! Puerta de plausibilidad (22 §2.2).

#![forbid(unsafe_code)]

use crate::TrackConfig;

#[derive(Debug, Clone, Copy, Default)]
pub struct PlausibilityFlags {
    pub excessive_speed: bool,
    pub outlier_jump: bool,
    pub mock_flagged: bool,
    pub frozen_fix: bool,
    pub implausible_accel: bool,
    pub low_accuracy: bool,
    pub network_only: bool,
}

impl PlausibilityFlags {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn to_vec(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.excessive_speed {
            out.push("EXCESSIVE_SPEED");
        }
        if self.outlier_jump {
            out.push("OUTLIER_JUMP");
        }
        if self.mock_flagged {
            out.push("MOCK_FLAGGED");
        }
        if self.frozen_fix {
            out.push("FROZEN_FIX");
        }
        if self.implausible_accel {
            out.push("IMPLAUSIBLE_ACCEL");
        }
        if self.low_accuracy {
            out.push("LOW_ACCURACY");
        }
        if self.network_only {
            out.push("NETWORK_ONLY");
        }
        out
    }

    /// Puntuación 0..=1 con las penalizaciones de `17` §2.11.
    pub fn score(&self) -> f64 {
        let mut p = 1.0_f64;
        if self.excessive_speed {
            p -= 0.8;
        }
        if self.outlier_jump {
            p -= 0.6;
        }
        if self.mock_flagged {
            p -= 0.5;
        }
        if self.frozen_fix {
            p -= 0.5;
        }
        if self.implausible_accel {
            p -= 0.4;
        }
        if self.low_accuracy {
            p -= 0.2;
        }
        if self.network_only {
            p -= 0.1;
        }
        p.max(0.0)
    }
}

/// Valores de velocidad maxima por motion. Stub.
pub fn motion_max_speed_mps(_cfg: &TrackConfig, motion: crate::Motion) -> f64 {
    let base = 70.0_f64;
    let m: f64 = match motion {
        crate::Motion::Stationary => 2.0,
        crate::Motion::Walking => 2.0,
        crate::Motion::Running => 8.0,
        crate::Motion::Cycling => 12.0,
        crate::Motion::Automotive => 70.0,
        crate::Motion::Unknown => 70.0,
    };
    m.min(base)
}

/// Evaluador de plausibilidad. Llena `flags` y deja los campos no modificados
/// (el caller ajusta frozen_fix/outlier con su estado).
pub fn evaluate(
    cfg: &TrackConfig,
    flags: &mut PlausibilityFlags,
    dist_to_prev_m: f64,
    dt_s: f64,
    sigma_prev: f64,
    sigma_new: f64,
    prev_speed: Option<f64>,
    new_speed: Option<f64>,
    source: &'static str,
    is_mock: bool,
    horizontal_accuracy_m: f64,
) {
    if !(horizontal_accuracy_m.is_finite()) || horizontal_accuracy_m <= 0.0 { /* no */ }
    // EXCESSIVE_SPEED
    if dt_s > 0.0 {
        let k = cfg.accuracy_sigma_factor;
        let v_max_motion = motion_max_speed_mps(cfg, crate::Motion::Unknown);
        let v_max = cfg.max_speed_mps.min(v_max_motion);
        let lhs = dist_to_prev_m - k * (sigma_prev + sigma_new);
        let rhs = v_max * dt_s;
        if lhs > rhs {
            flags.excessive_speed = true;
        }
    }
    // IMPLAUSIBLE_ACCEL
    if let (Some(sn), Some(sp)) = (new_speed, prev_speed) {
        if dt_s > 0.0 {
            let margin = 1.0; // speed_accuracy_mps not in function signature; minimal
            if (sn - sp).abs() / dt_s > cfg.max_accel_mps2 + margin {
                flags.implausible_accel = true;
            }
        }
    }
    // LOW_ACCURACY
    if horizontal_accuracy_m > cfg.low_accuracy_m {
        flags.low_accuracy = true;
    }
    // MOCK
    if is_mock {
        flags.mock_flagged = true;
    }
    // NETWORK_ONLY: source == "NETWORK"
    if source == "NETWORK" {
        flags.network_only = true;
    }
}
