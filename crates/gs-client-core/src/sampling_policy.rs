#![forbid(unsafe_code)]

/// Remote sampling policy, clamped per ` sampling_policy.json` clamp_ranges.
#[derive(Debug, Clone, Copy)]
pub struct SamplingPolicy {
    pub interval_ms: u32,
    pub publish_max_silence_s: u32,
    pub wake_region_radius_m: u32,
    pub downgrade_confirm_s: u32,
    pub predict_horizon_ms: u32,
}

impl SamplingPolicy {
    pub fn defaults() -> Self {
        Self {
            interval_ms: 10000,
            publish_max_silence_s: 60,
            wake_region_radius_m: 150,
            downgrade_confirm_s: 120,
            predict_horizon_ms: 3000,
        }
    }
}

pub fn clamp_policy(p: &mut SamplingPolicy, clamped: &mut u32) {
    // ranges per sampling_policy.json clamp_ranges
    let mut c = 0u32;
    if p.interval_ms < 1000 {
        p.interval_ms = 1000;
        c += 1;
    }
    if p.interval_ms > 300_000 {
        p.interval_ms = 300_000;
        c += 1;
    }
    if p.publish_max_silence_s < 5 {
        p.publish_max_silence_s = 5;
        c += 1;
    }
    if p.publish_max_silence_s > 600 {
        p.publish_max_silence_s = 600;
        c += 1;
    }
    if p.wake_region_radius_m < 50 {
        p.wake_region_radius_m = 50;
        c += 1;
    }
    if p.wake_region_radius_m > 1000 {
        p.wake_region_radius_m = 1000;
        c += 1;
    }
    if p.downgrade_confirm_s < 30 {
        p.downgrade_confirm_s = 30;
        c += 1;
    }
    if p.downgrade_confirm_s > 3600 {
        p.downgrade_confirm_s = 3600;
        c += 1;
    }
    if p.predict_horizon_ms > 10000 {
        p.predict_horizon_ms = 10000;
        c += 1;
    }
    *clamped = c;
}
