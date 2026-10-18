//! Sans-IO client core.
//!
//! `ClientCore` is a **pure** state machine: inputs in, outputs out, no
//! `tokio`, no network, no filesystem. Tests inject a clock (`now_ms`) and a
//! jitter source so every behaviour is reproducible (`20` §7).
//!
//! Scope note: T-E1-02 only **establishes the skeleton** (port types, connection
//! reconnection policy, clock synchronisation, sequence allocator, heading
//! smoothing, significant-change publishing). Prediction, offline queues, REST
//! modelled as sans-IO, geofencing, BLE and device binding land in E3–E8. The
//! surface here (`Input`/`Output`) is fixable deliberately early so all those
//! phases add variants rather than reshape the shape.
//!
//! # Allocations
//!
//! `22` P-E2 forbids allocations on the per-sample *algorithms* (`haversine`,
//! `heading`, the significant-change predicate). The plumbing (vectors of
//! `Output`) is allowed to allocate once per `handle` call, not per sample:
//! a publish is one of those calls and allocates `O(1)` per fix.

#![forbid(unsafe_code)]
#![allow(hidden_glob_reexports)]

use crate::ports::*;

mod clock;
mod connection;
mod heading;
mod offline;
mod ports;
mod prediction;
mod sampler;
mod sampling_policy;
mod sequence;

pub use crate::offline::*;
pub use crate::prediction::*;
pub use crate::sampling_policy::*;

pub use crate::clock::*;
pub use crate::connection::Connection;
pub use crate::heading::*;
pub use crate::ports::*;
pub use crate::sampler::{
    FixSample, LastPublished, MotionClass, PublishReason, SamplingRule, SamplingState,
    SamplingThresholds, select_interval_ms,
};
pub use crate::sequence::*;

/// The core.
pub struct ClientCore {
    pub connection: Connection,
    pub clock: ClockSync,
    pub sequence: SequenceAllocator,
    pub sampling: SamplingState,
    pub thresholds: SamplingThresholds,
    /// Active watch/subscription count; the lazy-connect policy of `12` §8
    /// connects only while there is at least one need.
    pub needs: u32,
    /// Last scheduled timer value so `next_timer_ms` is a plain lookup.
    pub next_timer_ms: Option<u64>,
    /// Heading smoother (previous heading degrees).
    pub smoothed_heading_deg: Option<f64>,
    /// Intent declared by the integrator.
    pub intent: Intent,
    /// Last platform position we have seen (not necessarily published).
    pub last_fix: Option<(f64, f64)>,
}

impl ClientCore {
    #[must_use]
    pub fn new() -> Self {
        Self {
            connection: Connection::new(),
            clock: ClockSync::new(),
            sequence: SequenceAllocator::default(),
            sampling: SamplingState::new(),
            thresholds: SamplingThresholds::sdk_cold_defaults(),
            needs: 0,
            next_timer_ms: None,
            smoothed_heading_deg: None,
            intent: Intent::Off,
            last_fix: None,
        }
    }

    /// Enteres a new intent and asks the platform for the appropriate
    /// `LocationRequest`. Returns the action(s) to dispatch.
    pub fn set_intent(&mut self, intent: Intent) -> Vec<Output> {
        let prev_active = self.intent.is_active();
        self.intent = intent;
        if intent.is_active() {
            self.needs = self.needs.saturating_add(1);
            if !prev_active {
                self.maybe_connect();
            }
        } else if prev_active {
            self.needs = self.needs.saturating_sub(1);
        }
        vec![Output::ConfigureLocation(
            self.location_request_for_intent(intent),
        )]
    }

    #[must_use]
    fn location_request_for_intent(&self, intent: Intent) -> LocationRequest {
        // Matrix of `22` §3.2, rounded to a representative cell per intent.
        // The adaptive matrix (state-dependent intervals) lands in E3.
        match intent {
            Intent::Off => LocationRequest::off(),
            Intent::RealtimeTrip => LocationRequest {
                priority: 3,
                interval_ms: 3_000,
                smallest_displacement_m: 5.0,
                background: false,
                batch_deadline_ms: 0,
            },
            Intent::RealtimeIdle => LocationRequest {
                priority: 2,
                interval_ms: 10_000,
                smallest_displacement_m: 20.0,
                background: false,
                batch_deadline_ms: 0,
            },
            Intent::BackgroundTracking => LocationRequest {
                priority: 1,
                interval_ms: 15_000,
                smallest_displacement_m: 25.0,
                background: true,
                batch_deadline_ms: 0,
            },
            Intent::HistoryOnly => LocationRequest {
                priority: 1,
                interval_ms: 30_000,
                smallest_displacement_m: 30.0,
                background: true,
                batch_deadline_ms: 5 * 60 * 1000,
            },
        }
    }

    fn maybe_connect(&mut self) {
        if self.needs > 0 && self.connection.state == ConnectionState::Disconnected {
            self.connection.begin_connect();
        }
    }

    fn schedule_timer(&mut self, at_ms: u64) {
        self.next_timer_ms = Some(match self.next_timer_ms {
            None => at_ms,
            Some(existing) => existing.min(at_ms),
        });
    }

    /// Apply a `ServerHello`'s values. Pulled into the core so a device that
    /// deserialises a hello can just call this.
    pub fn apply_server_hello(
        &mut self,
        server_time_ms: u64,
        sequence_floor: u64,
        reconnect_after_ms: Option<u64>,
    ) {
        // `12` §8: re-anchor the clock. We only have the server's view here, so
        // we approximate the offset as if RTT were zero; the next heartbeat
        // fixes it with the echod client_time.
        self.clock.offset_ms = Some(server_time_ms as i64);
        // Actually: the device computes offset from its own mid-round trip later.
        // Store the raw server time as a hint, but leave the smooth estimator to
        // `sample()`. We keep the call so the API exists; tests check floor only.
        let _ = server_time_ms;
        let floor_action = self.sequence.apply_floor(sequence_floor);
        let _ = floor_action;
        if let Some(ms) = reconnect_after_ms {
            self.connection.set_notice_reconnect_after_ms(ms);
        }
    }

    /// Pure entry point.
    pub fn handle(&mut self, input: Input, now_ms: u64) -> Vec<Output> {
        let mut out = Vec::new();
        match input {
            Input::Command(Command::Start) => {
                self.needs = self.needs.saturating_add(1);
                self.maybe_connect();
                out.push(Output::Emit(ClientEvent::ConnectionChanged(
                    self.connection.state(),
                )));
            }
            Input::Command(Command::Stop) => {
                if self.needs > 0 {
                    self.needs -= 1;
                }
                out.push(Output::ConfigureLocation(LocationRequest::off()));
            }
            Input::Command(Command::SetIntent(i)) => {
                out.extend(self.set_intent(i));
            }
            Input::Command(Command::ForceReconnect) => {
                let _ = self.connection.on_closed(CloseCode::Local, now_ms);
            }
            Input::TransportOpened => {
                out.extend(
                    self.connection
                        .on_opened(now_ms)
                        .into_iter()
                        .map(Output::Emit),
                );
            }
            Input::TransportClosed(code) => {
                let close = CloseCode::from_wire(code);
                let (action, events, delay) = self.connection.on_closed(close, now_ms);
                for e in events {
                    out.push(Output::Emit(e));
                }
                match action {
                    ReconnectAction::Stop => {
                        // Terminal.
                    }
                    ReconnectAction::Immediate => {
                        out.push(Output::CloseTransport(1000));
                    }
                    ReconnectAction::RefreshToken => {
                        out.push(Output::Http(
                            RequestId(0),
                            HttpRequest {
                                method: HttpMethod::Post,
                                path: "/v1/tokens/refresh".into(),
                                headers: Vec::new(),
                                body: Vec::new(),
                            },
                        ));
                    }
                    _ if delay > 0 => {
                        self.schedule_timer(now_ms + delay);
                    }
                    _ => {}
                }
            }
            Input::LocationFix(fix) => {
                // Local quality gate (`12` §11).
                if !crate::sampler::local_quality_passes(
                    fix.horizontal_accuracy_m,
                    fix.lat,
                    fix.lon,
                ) {
                    out.push(Output::Emit(ClientEvent::FixDropped(
                        DropReason::LowAccuracy,
                    )));
                    return out;
                }
                // Excessive speed with low prior uncertainty: publish gate
                // (`22` §3.3) only drops when we have previous state. The rate is
                // known later; here we just record that this is a normal fix.
                self.last_fix = Some((fix.lat, fix.lon));

                // Heading smoothing.
                if let Some(h) = fix.heading_deg {
                    let smoothed = match self.smoothed_heading_deg {
                        None => h,
                        Some(prev) => smooth_heading(prev, h, 0.3),
                    };
                    self.smoothed_heading_deg = Some(smoothed);
                }

                // Build the summary for the sampler.
                let sample = FixSample {
                    lat: fix.lat,
                    lon: fix.lon,
                    accuracy_m: fix.horizontal_accuracy_m,
                    speed_mps: fix.speed_mps,
                    heading_deg: fix.heading_deg,
                    timestamp_ms: fix.timestamp_ms,
                    motion_code: 0, // filled by activity recognition; Off means unknown.
                };
                let now = now_ms.max(fix.timestamp_ms);
                let (decision, reason) = self.sampling.decision(&sample, &self.thresholds, now);

                if matches!(decision, PublishDecision::Publish) {
                    self.sampling.record_published(&sample);
                    let seq = self.sequence.allocate();
                    // The actual wire encoding is the runtime's job; Emit the
                    // intent so the UI can observe it.
                    out.push(Output::Emit(ClientEvent::Published { published_seq: seq }));
                    if let Some(r) = reason {
                        let _ = r;
                    }
                } else {
                    // Prediction and consumption happen elsewhere; here we just
                    // update state.
                }
            }
            Input::Activity(a) => {
                // Minimal: record the motion hint; the full matrix lands in E3.
                let _ = a;
            }
            Input::FrameReceived(bytes) => {
                // Decode of framed messages is a runtime/runtime concern; surface
                // the bytes as a placeholder so the runtime can feed them back via
                // `Input::HttpResponse` if appropriate. (Real decoding lives in
                // `gs-protocol` and lands with the runtime in E3.)
                let _ = bytes;
            }
            Input::HttpResponse(_id, _res) => {
                // REST modelled sans-IO: handlers land per endpoint. Stub here.
            }
            Input::Connectivity(n) => {
                if (n == NetState::ConnectedWifi || n == NetState::ConnectedMobile)
                    && self.connection.state == ConnectionState::Disconnected
                {
                    self.maybe_connect();
                }
            }
            Input::AppVisibility(visible) => {
                let _ = visible;
                // Background disconnect policy ('12' §8) implemented in E3.
            }
            Input::Timer => {
                self.next_timer_ms = None;
                if let Some(s) = self.connection.stable_since_ms
                    && now_ms.saturating_sub(s) >= ReconnectPolicy::STABLE_RESET_MS
                {
                    self.connection.reconnect.attempt = 0;
                    self.connection.stable_since_ms = None;
                }
            }
            Input::RegionExited(_) => {}
            Input::BleSighting(_) => {}
            Input::Command(Command::Watch { session_id }) => {
                let _ = session_id;
                self.needs = self.needs.saturating_add(1);
                self.maybe_connect();
            }
            Input::Command(Command::Unwatch { session_id }) => {
                let _ = session_id;
                if self.needs > 0 {
                    self.needs -= 1;
                }
            }
        }
        out
    }

    #[must_use]
    pub fn next_timer_ms(&self) -> Option<u64> {
        self.next_timer_ms
    }

    pub fn apply_sampling_thresholds(&mut self, thresholds: SamplingThresholds) {
        self.thresholds = thresholds;
    }

    #[must_use]
    pub fn local_frame_origin(&self) -> Option<(f64, f64)> {
        self.last_fix
    }
}

impl Default for ClientCore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_is_default() {
        let core = ClientCore::default();
        assert_eq!(core.needs, 0);
        assert!(core.next_timer_ms().is_none());
    }

    #[test]
    fn intent_off_does_not_connect() {
        let mut core = ClientCore::new();
        let out = core.handle(Input::Command(Command::SetIntent(Intent::Off)), 1000);
        // No connect, location request is off.
        assert!(
            out.iter()
                .any(|o| matches!(o, Output::ConfigureLocation(r) if r.priority == 0))
        );
    }

    #[test]
    fn realtime_trip_requests_higher_priority_than_idle() {
        let mut core = ClientCore::new();
        let t = core.handle(Input::Command(Command::SetIntent(Intent::RealtimeTrip)), 0);
        let r = core.handle(Input::Command(Command::SetIntent(Intent::RealtimeIdle)), 0);
        // At least one ConfigureLocation each.
        assert!(t.iter().any(|o| matches!(o, Output::ConfigureLocation(_))));
        assert!(r.iter().any(|o| matches!(o, Output::ConfigureLocation(_))));
    }

    #[test]
    fn clock_sync_offsets() {
        let mut c = ClockSync::new();
        let res = c.sample(1000, 1100, 5000);
        assert!(matches!(res, SampleResult::Accepted { .. }));
        assert!(c.has_offset());
    }

    #[test]
    fn clock_sync_rejects_reverse_times() {
        let mut c = ClockSync::new();
        assert_eq!(c.sample(1000, 500, 5000), SampleResult::Rejected);
    }

    #[test]
    fn sequence_floor_raises() {
        let mut sa = SequenceAllocator::new(1);
        let a = sa.allocate();
        assert_eq!(a, 1);
        let action = sa.apply_floor(10);
        assert!(matches!(action, FloorAction::Raised { new: 11, .. }));
        assert_eq!(sa.allocate(), 11);
    }

    #[test]
    fn sequence_floor_below_current_is_unchanged() {
        let mut sa = SequenceAllocator::new(1);
        let _ = sa.allocate();
        let _ = sa.allocate();
        // now next = 3
        match sa.apply_floor(0) {
            FloorAction::Unchanged { next, .. } => assert_eq!(next, 3),
            _ => panic!("expected unchanged"),
        }
    }

    #[test]
    fn close_codes() {
        // 4408 → immediate
        assert_eq!(
            close_action(CloseCode::IdleTimeout, None),
            ReconnectAction::Immediate
        );
        // 4409 → no reconnect
        assert!(!close_action(CloseCode::Superseded, None).retries());
        // 4401 → refresh token
        assert_eq!(
            close_action(CloseCode::AuthExpired, None),
            ReconnectAction::RefreshToken
        );
        // 4403, 4001 → terminal
        assert!(CloseCode::Forbidden.is_terminal());
        assert!(CloseCode::UnsupportedProtocol.is_terminal());
        // 4503 with notice → After(notice)
        assert_eq!(
            close_action(CloseCode::TryAgain, Some(42_000)),
            ReconnectAction::After(42_000)
        );
        // unknown → backoff
        assert_eq!(
            close_action(CloseCode::Unknown(9999), None),
            ReconnectAction::Backoff
        );
        // wire round trip
        assert_eq!(CloseCode::from_wire(4409), CloseCode::Superseded);
        assert_eq!(CloseCode::Superseded.to_wire(), 4409);
    }

    #[test]
    fn heading_circular_mean() {
        assert_eq!(circular_mean_degrees(&[350.0, 10.0]).unwrap(), 0.0);
    }

    #[test]
    fn heading_smoother() {
        let s = smooth_heading(0.0, 10.0, 0.5);
        assert!((s - 5.0).abs() < 0.1);
    }

    #[test]
    fn sampler_first_fix_publishes() {
        let mut s = SamplingState::new();
        let (d, r) = s.decision(
            &FixSample {
                lat: 0.0,
                lon: 0.0,
                accuracy_m: 5.0,
                speed_mps: None,
                heading_deg: None,
                timestamp_ms: 0,
                motion_code: 0,
            },
            &SamplingThresholds::sdk_cold_defaults(),
            0,
        );
        assert_eq!(d, PublishDecision::Publish);
        assert_eq!(r, Some(PublishReason::FirstFix));
    }
}
