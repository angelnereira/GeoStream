//! Server-side trajectory engine (`gs-track`, E2).
//!
//! Pure math + state machines (22 §2). Integration with redis/CAS, fan-out,
//! `TrackEvent` transport and webhooks lives in `gs-service`; this crate is
//! the per-`(device, sequence)` kernel the ingest calls once.

#![forbid(unsafe_code)]
// Sólo mientras la interfaz de parámetros del motor se afina en E2/E6:
// el linter es estético, los umbrales de rendimiento son los de NFR-PERF-005.
#![allow(
    clippy::too_many_arguments,
    clippy::collapsible_if,
    clippy::needless_range_loop,
    clippy::identity_op
)]

mod kalman;
mod odometer;
mod plausibility;
mod stops;

pub use kalman::{KalmanCv, KalmanStatus};
pub use odometer::Odometer;
pub use plausibility::{PlausibilityFlags, motion_max_speed_mps};
pub use stops::{StopDetector, StopEvent, StopState};

use gs_geo::{LatLon, LocalFrame, haversine_distance};
use kalman::KalmanCv as K;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Stationary,
    Walking,
    Running,
    Cycling,
    Automotive,
    Unknown,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TrackConfig {
    pub accuracy_sigma_factor: f64,
    pub max_speed_mps: f64,
    pub max_accel_mps2: f64,
    pub low_accuracy_m: f64,
    pub frozen_fix_count: usize,
    pub reset_gap_s: f64,
    pub outlier_chi2: f64,
    pub outlier_reset_count: usize,
    pub process_noise_stationary_ms2: f64,
    pub process_noise_walking_ms2: f64,
    pub process_noise_running_ms2: f64,
    pub process_noise_cycling_ms2: f64,
    pub process_noise_automotive_ms2: f64,
    pub process_noise_unknown_ms2: f64,
    pub zupt_sigma_mps: f64,
    pub sigma_v0: f64,
    pub odometer_noise_factor: f64,
    pub track_stop_radius_m: f64,
    pub track_stop_min_duration_s: f64,
    pub track_stop_exit_confirm: usize,
    pub frame_reanchor_m: f64,
}

impl TrackConfig {
    pub fn defaults() -> Self {
        Self {
            accuracy_sigma_factor: 1.0,
            max_speed_mps: 70.0,
            max_accel_mps2: 8.0,
            low_accuracy_m: 100.0,
            frozen_fix_count: 5,
            reset_gap_s: 60.0,
            outlier_chi2: 13.82,
            outlier_reset_count: 3,
            process_noise_stationary_ms2: 0.05,
            process_noise_walking_ms2: 0.5,
            process_noise_running_ms2: 1.0,
            process_noise_cycling_ms2: 1.0,
            process_noise_automotive_ms2: 2.0,
            process_noise_unknown_ms2: 1.5,
            zupt_sigma_mps: 0.1,
            sigma_v0: 10.0,
            odometer_noise_factor: 1.0,
            track_stop_radius_m: 50.0,
            track_stop_min_duration_s: 120.0,
            track_stop_exit_confirm: 2,
            frame_reanchor_m: 10000.0,
        }
    }

    pub fn process_noise(&self, m: Motion) -> f64 {
        match m {
            Motion::Stationary => self.process_noise_stationary_ms2,
            Motion::Walking => self.process_noise_walking_ms2,
            Motion::Running => self.process_noise_running_ms2,
            Motion::Cycling => self.process_noise_cycling_ms2,
            Motion::Automotive => self.process_noise_automotive_ms2,
            Motion::Unknown => self.process_noise_unknown_ms2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackState {
    Init,
    Tracking,
    Coasting,
    Reset,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackResetReason {
    FirstFix,
    Gap,
    OutlierRun,
    FrameReanchor,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackEventKind {
    StopStarted,
    StopEnded,
    TrackingLost,
    TrackingResumed,
}

#[derive(Debug, Clone, Copy)]
pub struct TrackEstimate {
    pub position_lat: f64,
    pub position_lon: f64,
    pub position_std_m: f64,
    pub speed_mps: Option<f64>,
    pub heading_deg: Option<f64>,
    pub plausibility: f64,
    pub flags: PlausibilityFlags,
    pub track_state: TrackState,
    pub motion_state: Motion,
    pub odometer_m: f64,
}

#[derive(Debug)]
pub struct TrackUpdate {
    pub accepted: bool,
    pub reset_reason: Option<TrackResetReason>,
    pub estimate: Option<TrackEstimate>,
    pub events: Vec<TrackEventKind>,
}

#[derive(Debug, Clone, Copy)]
pub struct Sample {
    pub sequence: u64,
    pub sample_at_ms: u64,
    pub lat: f64,
    pub lon: f64,
    pub horizontal_accuracy_m: f64,
    pub speed_mps: Option<f64>,
    pub speed_accuracy_mps: Option<f64>,
    pub heading_deg: Option<f64>,
    pub is_mock: bool,
    pub source: &'static str,
    pub motion: Motion,
}

pub struct TrackStateMachine {
    pub config: TrackConfig,
    kalman: Option<K>,
    frame: Option<LocalFrame>,
    origin_lat: f64,
    origin_lon: f64,
    state: TrackState,
    prev_ms: Option<u64>,
    frozen_count: usize,
    prev_lat: f64,
    prev_lon: f64,
    prev_acc: f64,
    prev_speed: Option<f64>,
    outlier_run: usize,
    motion: Motion,
    pub odometer: Odometer,
    pub stops: StopDetector,
}

impl TrackStateMachine {
    pub fn new(config: TrackConfig) -> Self {
        let stops = StopDetector::new(
            config.track_stop_radius_m,
            config.track_stop_min_duration_s,
            config.track_stop_exit_confirm,
        );
        Self {
            config,
            kalman: None,
            frame: None,
            origin_lat: 0.0,
            origin_lon: 0.0,
            state: TrackState::Init,
            prev_ms: None,
            frozen_count: 0,
            prev_lat: 0.0,
            prev_lon: 0.0,
            prev_acc: 8.0,
            prev_speed: None,
            outlier_run: 0,
            motion: Motion::Unknown,
            odometer: Odometer::new(),
            stops,
        }
    }

    pub fn state(&self) -> TrackState {
        self.state
    }

    pub fn apply(&mut self, s: &Sample) -> TrackUpdate {
        // init frame if needed
        if self.frame.is_none() {
            self.frame = Some(LocalFrame::new(LatLon::new(s.lat, s.lon)));
            self.origin_lat = s.lat;
            self.origin_lon = s.lon;
        }
        let cfg = &self.config;
        // reanchor check
        let dist_from_origin = haversine_distance(
            LatLon::new(self.origin_lat, self.origin_lon),
            LatLon::new(s.lat, s.lon),
        );
        let mut reanchor = false;
        if dist_from_origin >= cfg.frame_reanchor_m {
            self.frame = Some(LocalFrame::new(LatLon::new(s.lat, s.lon)));
            self.origin_lat = s.lat;
            self.origin_lon = s.lon;
            reanchor = true;
        }
        let frame = self.frame.unwrap();
        let enu = frame.to_enu(LatLon::new(s.lat, s.lon));

        let mut flags = PlausibilityFlags::empty();

        let now = s.sample_at_ms;
        let prev_ms = self.prev_ms;
        let dt_s = match prev_ms {
            None => 0.0,
            Some(p) => (now - p) as f64 / 1000.0,
        };

        // frozen fix detection
        let same_pos_acc = self.kalman.is_some()
            && self.prev_lat == s.lat
            && self.prev_acc == s.horizontal_accuracy_m;
        if same_pos_acc && dt_s > 0.0 && s.motion != Motion::Stationary {
            self.frozen_count += 1;
        } else {
            self.frozen_count = 1;
        }
        if self.frozen_count >= cfg.frozen_fix_count {
            flags.frozen_fix = true;
        }

        // gap detection
        let reset_gap = dt_s > cfg.reset_gap_s;

        // distance to previous position in meters (haversine, proxy for enu prev)
        let dist_to_prev = if self.kalman.is_some() {
            haversine_distance(
                LatLon::new(self.prev_lat, self.prev_lon),
                LatLon::new(s.lat, s.lon),
            )
        } else {
            0.0
        };

        let speed_prev_for_accel = self.prev_speed;
        let sigma_prev = self.prev_acc.max(1e-6);
        // First sample handling plausibility
        plausibility::evaluate(
            cfg,
            &mut flags,
            dist_to_prev,
            dt_s,
            sigma_prev,
            s.horizontal_accuracy_m,
            speed_prev_for_accel,
            s.speed_mps,
            s.source,
            s.is_mock,
            s.horizontal_accuracy_m,
        );
        if reanchor {
            // only a reset reason, not a flag
        }

        // motion max speed patch: the evaluate used Unknown (max 70); better recompute EXCESSIVE_SPEED
        // with motion-specific v_max. Redo just that comparison:
        if dt_s > 0.0 && self.kalman.is_some() {
            let mv = motion_max_speed_mps(cfg, s.motion).min(cfg.max_speed_mps);
            let lhs = dist_to_prev
                - cfg.accuracy_sigma_factor * (self.prev_acc + s.horizontal_accuracy_m);
            let rhs = mv * dt_s;
            flags.excessive_speed = lhs > rhs;
        }

        // LOW_ACCURACY already set.

        // FIRST FIX
        if self.kalman.is_none() {
            self.kalman = Some(K::init(enu, cfg.sigma_v0));
            self.state = TrackState::Tracking;
            self.prev_ms = Some(now);
            self.prev_lat = s.lat;
            self.prev_lon = s.lon;
            self.prev_acc = s.horizontal_accuracy_m;
            self.prev_speed = s.speed_mps;
            self.motion = s.motion;
            let pos_std = cfg.accuracy_sigma_factor * s.horizontal_accuracy_m;
            let estimate = TrackEstimate {
                position_lat: s.lat,
                position_lon: s.lon,
                position_std_m: pos_std.max(1e-6),
                speed_mps: s.speed_mps,
                heading_deg: s.heading_deg,
                plausibility: flags.score(),
                flags,
                track_state: TrackState::Tracking,
                motion_state: s.motion,
                odometer_m: self.odometer.value(),
            };
            // odometer feed (#1)
            self.odometer
                .feed(s.lat, s.lon, pos_std, cfg.odometer_noise_factor);
            // stops
            self.stops
                .update(s.lat, s.lon, self.origin_lat, self.origin_lon, now);
            self.motion = s.motion;
            return TrackUpdate {
                accepted: true,
                reset_reason: Some(TrackResetReason::FirstFix),
                estimate: Some(estimate),
                events: self.take_events(),
            };
        }

        // reset gap?
        let do_reset = reset_gap;
        let mut reset_reason = if do_reset {
            Some(TrackResetReason::Gap)
        } else if reanchor {
            Some(TrackResetReason::FrameReanchor)
        } else {
            None
        };

        if do_reset {
            self.kalman = Some(K::init(enu, cfg.sigma_v0));
            self.state = TrackState::Tracking;
            self.outlier_run = 0;
        }

        // step otherwise (and when not reset)
        let mut outlier_jump = false;
        if !do_reset {
            let k = self.kalman.as_mut().unwrap();
            let acc = s.horizontal_accuracy_m;
            let chi2 = k.step(
                enu,
                s.speed_mps,
                s.heading_deg,
                s.speed_accuracy_mps,
                dt_s,
                cfg.process_noise(s.motion),
                acc,
                cfg.accuracy_sigma_factor,
                cfg.outlier_chi2,
                cfg.outlier_reset_count,
                &mut self.outlier_run,
                &mut outlier_jump,
            );
            let _ = chi2;
            if outlier_jump {
                flags.outlier_jump = true;
            }
            if self.outlier_run >= cfg.outlier_reset_count {
                // filter reset to new measurement
                let k = self.kalman.as_mut().unwrap();
                k.set_last(enu);
                self.state = TrackState::Tracking;
                self.outlier_run = 0;
                reset_reason = Some(TrackResetReason::OutlierRun);
            }
        } else {
            // initialising again
            self.outlier_run = 0;
        }

        // tracking lost if no measurement? leave tracking (coasting happens if measurement rejected but here measurement is accepted)

        // ZUPT
        if matches!(s.motion, Motion::Stationary) || self.stops.state() == StopState::Stopped {
            if let Some(k) = self.kalman.as_mut() {
                k.zupt(cfg.zupt_sigma_mps);
            }
        }

        // build estimate from kalman state
        let k = self.kalman.as_ref().unwrap();
        let (pe, pn, ve, vn) = k.state();
        // enu -> lat/lon using frame anchor
        let pos = frame.from_enu(pe, pn);
        // After a reset the filter has no velocity history: report `None` rather
        // than a fabricated 0 (TRK-RESET-001).
        let speed = if do_reset {
            None
        } else {
            Some((ve * ve + vn * vn).sqrt())
        };
        let heading = if ve.abs() + vn.abs() > 1e-9 {
            Some(ve.atan2(vn).to_degrees().rem_euclid(360.0))
        } else {
            None
        };
        let std = k.position_std();

        self.state = TrackState::Tracking;

        self.odometer
            .feed(pos.lat, pos.lon, std, cfg.odometer_noise_factor);
        // gap bridging: if reset_gap and track_odometer_bridge_gaps (not configurable here) -> bridge straight distance
        if do_reset && cfg.track_stop_min_duration_s >= 0.0 { /* placeholder */ }

        self.stops
            .update(pos.lat, pos.lon, self.origin_lat, self.origin_lon, now);

        // update prev
        self.prev_ms = Some(now);
        self.prev_lat = s.lat;
        self.prev_lon = s.lon;
        self.prev_acc = s.horizontal_accuracy_m;
        self.prev_speed = s.speed_mps;
        self.motion = s.motion;

        let estimate = TrackEstimate {
            position_lat: pos.lat,
            position_lon: pos.lon,
            position_std_m: std,
            speed_mps: speed,
            heading_deg: heading,
            plausibility: flags.score(),
            flags,
            track_state: self.state,
            motion_state: s.motion,
            odometer_m: self.odometer.value(),
        };

        TrackUpdate {
            accepted: true,
            reset_reason,
            estimate: Some(estimate),
            events: self.take_events(),
        }
    }

    pub fn take_events(&mut self) -> Vec<TrackEventKind> {
        self.stops
            .take_events()
            .into_iter()
            .map(|e| match e {
                stops::StopEvent::Started => TrackEventKind::StopStarted,
                stops::StopEvent::Ended => TrackEventKind::StopEnded,
            })
            .collect()
    }
}

// --- Checkpointing (docs/22 §2.1, docs/11 §3.1) ---------------------------
//
// The filter state is checkpointed so a reconnect on another instance resumes
// the trajectory instead of starting cold. The format is a small, versioned,
// little-endian blob stored under `{p}:trk:{device}`; it is internal to the
// engine (never a wire contract), so it carries its own version byte.

const CHECKPOINT_VERSION: u8 = 1;

fn put_f64(out: &mut Vec<u8>, v: f64) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_u8(out: &mut Vec<u8>, v: u8) {
    out.push(v);
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        if end > self.bytes.len() {
            return None;
        }
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Some(slice)
    }
    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
}

fn state_code(state: TrackState) -> u8 {
    match state {
        TrackState::Init => 0,
        TrackState::Tracking => 1,
        TrackState::Coasting => 2,
        TrackState::Reset => 3,
    }
}
fn state_from(code: u8) -> Option<TrackState> {
    match code {
        0 => Some(TrackState::Init),
        1 => Some(TrackState::Tracking),
        2 => Some(TrackState::Coasting),
        3 => Some(TrackState::Reset),
        _ => None,
    }
}
fn motion_code(m: Motion) -> u8 {
    match m {
        Motion::Stationary => 0,
        Motion::Walking => 1,
        Motion::Running => 2,
        Motion::Cycling => 3,
        Motion::Automotive => 4,
        Motion::Unknown => 5,
    }
}
fn motion_from(code: u8) -> Option<Motion> {
    match code {
        0 => Some(Motion::Stationary),
        1 => Some(Motion::Walking),
        2 => Some(Motion::Running),
        3 => Some(Motion::Cycling),
        4 => Some(Motion::Automotive),
        5 => Some(Motion::Unknown),
        _ => None,
    }
}

impl TrackStateMachine {
    /// Serializes the engine state for `{p}:trk:{device}`.
    #[must_use]
    pub fn encode_checkpoint(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(256);
        put_u8(&mut out, CHECKPOINT_VERSION);
        put_f64(&mut out, self.origin_lat);
        put_f64(&mut out, self.origin_lon);
        match &self.kalman {
            Some(k) => {
                put_u8(&mut out, 1);
                let (x, p) = k.raw();
                for v in x {
                    put_f64(&mut out, v);
                }
                for v in p {
                    put_f64(&mut out, v);
                }
            }
            None => put_u8(&mut out, 0),
        }
        put_u8(&mut out, state_code(self.state));
        match self.prev_ms {
            Some(ms) => {
                put_u8(&mut out, 1);
                put_u64(&mut out, ms);
            }
            None => put_u8(&mut out, 0),
        }
        put_u32(&mut out, self.frozen_count as u32);
        put_f64(&mut out, self.prev_lat);
        put_f64(&mut out, self.prev_lon);
        put_f64(&mut out, self.prev_acc);
        match self.prev_speed {
            Some(s) => {
                put_u8(&mut out, 1);
                put_f64(&mut out, s);
            }
            None => put_u8(&mut out, 0),
        }
        put_u32(&mut out, self.outlier_run as u32);
        put_u8(&mut out, motion_code(self.motion));
        // Odometer.
        put_u8(&mut out, u8::from(self.odometer.has_anchor));
        put_f64(&mut out, self.odometer.anchor_lat);
        put_f64(&mut out, self.odometer.anchor_lon);
        put_f64(&mut out, self.odometer.value_m);
        put_f64(&mut out, self.odometer.bridged_m);
        // Stop detector.
        put_u8(
            &mut out,
            match self.stops.state {
                stops::StopState::Candidate => 1,
                stops::StopState::Stopped => 2,
                stops::StopState::None => 0,
            },
        );
        put_f64(&mut out, self.stops.c_sum_lat);
        put_f64(&mut out, self.stops.c_sum_lon);
        put_u32(&mut out, self.stops.c_count as u32);
        match self.stops.c_started_ms {
            Some(ms) => {
                put_u8(&mut out, 1);
                put_u64(&mut out, ms);
            }
            None => put_u8(&mut out, 0),
        }
        put_u32(&mut out, self.stops.consecutive_outside as u32);
        out
    }

    /// Rebuilds the engine from a checkpoint. Returns `None` on any malformed
    /// input: a checkpoint that cannot be read is discarded, never trusted
    /// partially.
    #[must_use]
    pub fn decode_checkpoint(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader::new(bytes);
        if r.u8()? != CHECKPOINT_VERSION {
            return None;
        }
        let origin_lat = r.f64()?;
        let origin_lon = r.f64()?;
        let kalman = if r.u8()? == 1 {
            let mut x = [0.0f64; 4];
            for slot in &mut x {
                *slot = r.f64()?;
            }
            let mut p = [0.0f64; 16];
            for slot in &mut p {
                *slot = r.f64()?;
            }
            Some(K::from_raw(x, p))
        } else {
            None
        };
        let state = state_from(r.u8()?)?;
        let prev_ms = if r.u8()? == 1 { Some(r.u64()?) } else { None };
        let frozen_count = r.u32()? as usize;
        let prev_lat = r.f64()?;
        let prev_lon = r.f64()?;
        let prev_acc = r.f64()?;
        let prev_speed = if r.u8()? == 1 { Some(r.f64()?) } else { None };
        let outlier_run = r.u32()? as usize;
        let motion = motion_from(r.u8()?)?;

        let config = TrackConfig::defaults();
        let has_anchor = r.u8()? == 1;
        let anchor_lat = r.f64()?;
        let anchor_lon = r.f64()?;
        let value_m = r.f64()?;
        let bridged_m = r.f64()?;

        let stop_state = match r.u8()? {
            0 => stops::StopState::None,
            1 => stops::StopState::Candidate,
            2 => stops::StopState::Stopped,
            _ => return None,
        };
        let c_sum_lat = r.f64()?;
        let c_sum_lon = r.f64()?;
        let c_count = r.u32()? as usize;
        let c_started_ms = if r.u8()? == 1 { Some(r.u64()?) } else { None };
        let consecutive_outside = r.u32()? as usize;

        let frame = if kalman.is_some() {
            Some(LocalFrame::new(LatLon::new(origin_lat, origin_lon)))
        } else {
            None
        };

        let mut stops = StopDetector::new(
            config.track_stop_radius_m,
            config.track_stop_min_duration_s,
            config.track_stop_exit_confirm,
        );
        stops.state = stop_state;
        stops.c_sum_lat = c_sum_lat;
        stops.c_sum_lon = c_sum_lon;
        stops.c_count = c_count;
        stops.c_started_ms = c_started_ms;
        stops.consecutive_outside = consecutive_outside;

        Some(Self {
            config,
            kalman,
            frame,
            origin_lat,
            origin_lon,
            state,
            prev_ms,
            frozen_count,
            prev_lat,
            prev_lon,
            prev_acc,
            prev_speed,
            outlier_run,
            motion,
            odometer: Odometer {
                anchor_lat,
                anchor_lon,
                value_m,
                bridged_m,
                has_anchor,
            },
            stops,
        })
    }
}

#[cfg(test)]
mod checkpoint_tests {
    use super::*;

    fn sample(seq: u64, at_ms: u64, lat: f64) -> Sample {
        Sample {
            sequence: seq,
            sample_at_ms: at_ms,
            lat,
            lon: -79.5199,
            horizontal_accuracy_m: 8.0,
            speed_mps: Some(4.0),
            speed_accuracy_mps: None,
            heading_deg: Some(90.0),
            is_mock: false,
            source: "GPS",
            motion: Motion::Automotive,
        }
    }

    #[test]
    fn checkpoint_round_trip_preserves_the_estimate() {
        let mut engine = TrackStateMachine::new(TrackConfig::defaults());
        engine.apply(&sample(1, 0, 8.9824));
        engine.apply(&sample(2, 2000, 8.9830));
        let before = engine.apply(&sample(3, 4000, 8.9836));

        let blob = engine.encode_checkpoint();
        let mut restored = TrackStateMachine::decode_checkpoint(&blob).expect("decode");

        // The next sample on the restored engine must produce the same estimate
        // as the next sample on the original: continuity across instances.
        let s4 = sample(4, 6000, 8.9842);
        let a = engine.apply(&s4);
        let b = restored.apply(&s4);
        let ea = a.estimate.unwrap();
        let eb = b.estimate.unwrap();
        assert!((ea.position_lat - eb.position_lat).abs() < 1e-9);
        assert!((ea.position_lon - eb.position_lon).abs() < 1e-9);
        assert!((ea.odometer_m - eb.odometer_m).abs() < 1e-9);
        assert_eq!(ea.track_state, eb.track_state);
        let _ = before;
    }

    #[test]
    fn malformed_checkpoint_is_rejected_not_trusted() {
        assert!(TrackStateMachine::decode_checkpoint(&[]).is_none());
        assert!(TrackStateMachine::decode_checkpoint(&[99, 1, 2, 3]).is_none());
        let mut engine = TrackStateMachine::new(TrackConfig::defaults());
        engine.apply(&sample(1, 0, 8.9824));
        let blob = engine.encode_checkpoint();
        // Truncated blob: rejected.
        assert!(TrackStateMachine::decode_checkpoint(&blob[..blob.len() - 4]).is_none());
    }
}
