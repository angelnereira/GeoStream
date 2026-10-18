//! Significant-change publishing, per `22` §3.3.
//!
//! The server has the filter; the device decides **when** to publish. The rules
//! here are exactly `22` §3.3: every condition is a *minimum* threshold on the
//! *difference* between the current fix and the last one published. A distance
//! shorter than `publish_min_distance_m` does not publish by itself, *unless* one
//! of the other four conditions holds.
//!
//! All thresholds are parameters, never constants. Per `22` §3.4 they come from
//! the server's `SamplingPolicy` (`ServerHello.sampling`) and are clamped against\n//! the hard bounds of `17` before use. Before the first connection the SDK
//! defaults are used. This is why the significant-change function takes a
//! `Thresholds` argument instead of reading globals.

#![forbid(unsafe_code)]

use crate::ports::PublishDecision;
use gs_geo::{LatLon, haversine_distance};

/// Thresholds coming from the server policy. They are clamped by the runtime
/// (T-30) before reaching here; this type makes no assumptions about range.
#[derive(Debug, Clone, Copy)]
pub struct SamplingThresholds {
    pub publish_min_distance_m: f64,
    pub publish_accuracy_factor: f64,
    pub publish_min_course_change_deg: f64,
    pub publish_turn_min_speed_mps: f64,
    pub publish_min_speed_change_mps: f64,
    pub publish_max_silence_ms: u64,
    pub min_publish_interval_ms: u64,
}

impl SamplingThresholds {
    /// Values taken from the SDK before the first `ServerHello`. They are **not**
    /// the server defaults (those live in `17` and arrive on connection) — they
    /// exist only to keep a cold device from flooding before it has ever spoken.
    #[must_use]
    pub fn sdk_cold_defaults() -> Self {
        Self {
            publish_min_distance_m: 5.0,
            publish_accuracy_factor: 1.0,
            publish_min_course_change_deg: 20.0,
            publish_turn_min_speed_mps: 3.0,
            publish_min_speed_change_mps: 2.0,
            publish_max_silence_ms: 60_000,
            min_publish_interval_ms: 1_000,
        }
    }
}

/// Motion class as used by the adaptive sampler (`22` §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionClass {
    Stationary,
    OnFoot,
    Cycling,
    Vehicle,
    Unknown,
}

impl MotionClass {
    /// Only a genuinely moving class can select a speed/accuracy zone rule.
    #[must_use]
    pub const fn is_moving(self) -> bool {
        matches!(self, Self::OnFoot | Self::Cycling | Self::Vehicle)
    }
}

/// A zone rule from `SamplingPolicy.sampling_rules`.
#[derive(Debug, Clone, Copy)]
pub struct SamplingRule {
    pub zone: &'static str,
    pub max_speed_mps: f64,
    pub max_accuracy_m: f64,
    pub interval_ms: u32,
}

impl SamplingRule {
    /// A rule with both maxima at zero is the *open* catch-all: it matches
    /// anything and guarantees publication never stops (`SMP-INT-004/005`).
    #[must_use]
    pub fn is_open(self) -> bool {
        self.max_speed_mps <= 0.0 && self.max_accuracy_m <= 0.0
    }
}

/// Selects the sampling interval (ms) for a fix, per `22` §3.2 and the
/// `sampling_rules` of the policy.
///
/// Rules are evaluated in order and the **first match wins**. `motion` is
/// `None` when the platform did not report it (the class is then inferred from
/// speed) and `Some(Unknown)` when it reported "unknown" or "stationary" —
/// both mean "not moving", which selects the open rule.
///
/// The function never returns "stop": if no rule matches and there is no open
/// rule, the last rule's interval is used, because a sampler that stops
/// publishing is worse than one that publishes too often.
#[must_use]
pub fn select_interval_ms(
    rules: &[SamplingRule],
    speed_mps: f64,
    accuracy_m: f64,
    motion: Option<MotionClass>,
) -> u32 {
    if rules.is_empty() {
        return 0;
    }
    let moving = match motion {
        // Not reported: infer from speed.
        None => speed_mps > 0.0,
        // Reported and not a moving class: the open rule applies.
        Some(m) => m.is_moving(),
    };
    for rule in rules {
        if rule.is_open() {
            return rule.interval_ms;
        }
        if moving && speed_mps <= rule.max_speed_mps && accuracy_m <= rule.max_accuracy_m {
            return rule.interval_ms;
        }
    }
    rules[rules.len() - 1].interval_ms
}

/// A compact summary of the fix the sampler needs. The caller (`ClientCore`)
/// builds it from a `PlatformFix`; the sampler does not read platform fields
/// directly, which keeps this function testable in isolation.
#[derive(Debug, Clone, Copy)]
pub struct FixSample {
    pub lat: f64,
    pub lon: f64,
    pub accuracy_m: f64,
    pub speed_mps: Option<f64>,
    pub heading_deg: Option<f64>,
    pub timestamp_ms: u64,
    pub motion_code: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishReason {
    FirstFix,
    SignificantDistance,
    CourseChange,
    SpeedChange,
    MotionTransition,
    SilenceTimeout,
}

/// Per `12` §11: whether a fix is even allowed to enter the publishing
/// pipeline. The sampler is *after* the local gate; this helper re-expresses the
/// gate for tests that don't build a `PlatformFix`.
#[must_use]
pub fn local_quality_passes(accuracy_m: f64, lat: f64, lon: f64) -> bool {
    accuracy_m.is_finite() && accuracy_m > 0.0 && lat.is_finite() && lon.is_finite()
}

/// Recorded state of the last fix that was actually published.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LastPublished {
    pub lat: f64,
    pub lon: f64,
    pub ts_ms: u64,
    pub speed_mps: Option<f64>,
    pub heading_deg: Option<f64>,
    pub motion_code: u8,
    pub accuracy_m: f64,
}

#[derive(Debug, Default)]
pub struct SamplingState {
    pub last: Option<LastPublished>,
    /// Counts consecutive publishes below the min interval; for diagnostics.
    pub publishes_skipped_for_rate: u64,
    pub samples_seen: u64,
    pub samples_published: u64,
}

impl SamplingState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            last: None,
            publishes_skipped_for_rate: 0,
            samples_seen: 0,
            samples_published: 0,
        }
    }

    pub fn reset(&mut self) {
        self.last = None;
        self.publishes_skipped_for_rate = 0;
        self.samples_seen = 0;
        self.samples_published = 0;
    }

    /// Pure decision for the fix. `now_ms` is the clock at which the fix is
    /// evaluated and must be monotonic so the silence rule compares correctly.
    ///
    /// `decision` does **not** update `last`: the caller commits via
    /// `record_published` only after the fix has actually been sent. A failed
    /// send (which the core cannot observe) then cannot desynchronise the state.
    #[must_use]
    #[allow(clippy::collapsible_if)]
    pub fn decision(
        &mut self,
        fix: &FixSample,
        thresholds: &SamplingThresholds,
        now_ms: u64,
    ) -> (PublishDecision, Option<PublishReason>) {
        let Some(last) = &self.last else {
            // Primer fix: se publica siempre que pase la puerta de calidad
            // (que `ClientCore` ya aplicó antes de llamar aquí).
            return (PublishDecision::Publish, Some(PublishReason::FirstFix));
        };

        let dt_ms = now_ms.saturating_sub(last.ts_ms);

        // Distancia a la última publicada.
        let dist = haversine_distance(
            LatLon::new(last.lat, last.lon),
            LatLon::new(fix.lat, fix.lon),
        );

        // Distancia significativa: dist ≥ max(MIN, FACTOR·accuracy).
        let threshold_dist = thresholds
            .publish_min_distance_m
            .max(thresholds.publish_accuracy_factor * last.accuracy_m);
        let significant_distance = dist >= threshold_dist;

        let course_change = fix.heading_deg.zip(last.heading_deg).is_some_and(|(a, b)| {
            let delta = crate::heading::angle_delta_deg(b, a).abs();
            delta >= thresholds.publish_min_course_change_deg
                && fix.speed_mps.unwrap_or(0.0) >= thresholds.publish_turn_min_speed_mps
        });

        let speed_change = match (fix.speed_mps, last.speed_mps) {
            (Some(a), Some(b)) => (a - b).abs() >= thresholds.publish_min_speed_change_mps,
            _ => false,
        };

        let motion_transition = fix.motion_code != last.motion_code;

        let silence_timeout = dt_ms >= thresholds.publish_max_silence_ms;

        let reason = if significant_distance {
            Some(PublishReason::SignificantDistance)
        } else if course_change {
            Some(PublishReason::CourseChange)
        } else if speed_change {
            Some(PublishReason::SpeedChange)
        } else if motion_transition {
            Some(PublishReason::MotionTransition)
        } else if silence_timeout {
            Some(PublishReason::SilenceTimeout)
        } else {
            None
        };

        // The floor always applies: never publish more often than the server
        // allows, even if a significant change is detected.
        if dt_ms < thresholds.min_publish_interval_ms {
            if reason.is_some() {
                self.publishes_skipped_for_rate += 1;
            }
            return (PublishDecision::Skip, None);
        }

        match reason {
            Some(r) => (PublishDecision::Publish, Some(r)),
            None => (PublishDecision::Skip, None),
        }
    }

    pub fn record_published(&mut self, fix: &FixSample) {
        self.last = Some(LastPublished {
            lat: fix.lat,
            lon: fix.lon,
            ts_ms: fix.timestamp_ms,
            speed_mps: fix.speed_mps,
            heading_deg: fix.heading_deg,
            motion_code: fix.motion_code,
            accuracy_m: fix.accuracy_m,
        });
        self.samples_published += 1;
    }
}

#[cfg(test)]
mod rule_tests {
    use super::*;

    /// Rules from `contracts/test-vectors/sampling_policy.json`.
    const RULES: &[SamplingRule] = &[
        SamplingRule {
            zone: "URBAN",
            max_speed_mps: 8.0,
            max_accuracy_m: 25.0,
            interval_ms: 1000,
        },
        SamplingRule {
            zone: "HIGHWAY",
            max_speed_mps: 40.0,
            max_accuracy_m: 50.0,
            interval_ms: 3000,
        },
        SamplingRule {
            zone: "RURAL",
            max_speed_mps: 0.0,
            max_accuracy_m: 0.0,
            interval_ms: 5000,
        },
    ];

    #[test]
    fn smp_int_001_urban_by_speed() {
        assert_eq!(select_interval_ms(RULES, 4.0, 8.0, None), 1000);
    }

    #[test]
    fn smp_int_002_highway_not_urban() {
        assert_eq!(select_interval_ms(RULES, 25.0, 8.0, None), 3000);
    }

    #[test]
    fn smp_int_003_poor_accuracy_falls_to_open() {
        assert_eq!(select_interval_ms(RULES, 4.0, 120.0, None), 5000);
    }

    #[test]
    fn smp_int_004_stationary_uses_open_rule() {
        assert_eq!(
            select_interval_ms(RULES, 0.0, 5.0, Some(MotionClass::Stationary)),
            5000
        );
    }

    #[test]
    fn smp_int_005_unknown_motion_never_stops_publishing() {
        let interval = select_interval_ms(RULES, 12.0, 8.0, Some(MotionClass::Unknown));
        assert_eq!(interval, 5000);
        assert!(interval > 0, "publishing must never stop");
    }

    #[test]
    fn moving_class_enables_zone_rules() {
        assert_eq!(
            select_interval_ms(RULES, 12.0, 8.0, Some(MotionClass::Vehicle)),
            3000
        );
        assert_eq!(
            select_interval_ms(RULES, 1.0, 8.0, Some(MotionClass::OnFoot)),
            1000
        );
    }

    #[test]
    fn empty_rules_are_zero_not_a_panic() {
        assert_eq!(select_interval_ms(&[], 1.0, 1.0, None), 0);
    }
}
