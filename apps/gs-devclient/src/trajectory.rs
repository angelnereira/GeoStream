//! Synthetic trajectory generator for development and testing.
#![forbid(unsafe_code)]

use std::f64::consts::PI;
use gs_protocol::pbjson_types::Timestamp;
use gs_protocol::{
    LocationKind, LocationQuality, LocationSample, LocationSource, MotionActivity, Position,
};

/// Type of synthetic trajectory to generate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrajectoryKind {
    Circle,
    Linear,
    Sine,
}

impl std::str::FromStr for TrajectoryKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "circle" => Ok(Self::Circle),
            "linear" => Ok(Self::Linear),
            "sine" => Ok(Self::Sine),
            other => Err(format!(
                "Unknown trajectory kind '{other}'. Allowed: circle, linear, sine"
            )),
        }
    }
}

/// Generator of sequential synthetic location samples.
pub struct TrajectoryGenerator {
    kind: TrajectoryKind,
    center_lat: f64,
    center_lon: f64,
    speed_mps: f64,
    step_secs: f64,
    radius_m: f64,
    current_seq: u64,
    current_step: u64,
    motion: MotionActivity,
}

impl TrajectoryGenerator {
    pub fn new(
        kind: TrajectoryKind,
        center_lat: f64,
        center_lon: f64,
        speed_mps: f64,
        interval_ms: u64,
    ) -> Self {
        Self {
            kind,
            center_lat,
            center_lon,
            speed_mps,
            step_secs: (interval_ms as f64) / 1000.0,
            radius_m: 400.0,
            current_seq: 1,
            current_step: 0,
            motion: MotionActivity::Automotive,
        }
    }

    pub fn with_start_seq(mut self, seq: u64) -> Self {
        self.current_seq = seq;
        self
    }

    pub fn with_radius(mut self, radius_m: f64) -> Self {
        self.radius_m = radius_m;
        self
    }

    pub fn with_motion(mut self, motion: MotionActivity) -> Self {
        self.motion = motion;
        self
    }

    /// Generates the next sequential sample.
    pub fn next_sample(&mut self) -> LocationSample {
        let seq = self.current_seq;
        self.current_seq += 1;

        let (lat, lon, heading) = match self.kind {
            TrajectoryKind::Circle => {
                let circumference = 2.0 * PI * self.radius_m;
                let period_secs = circumference / self.speed_mps.max(1.0);
                let t = (self.current_step as f64 * self.step_secs) % period_secs;
                let theta = (t / period_secs) * 2.0 * PI;

                let d_lat = (self.radius_m * theta.sin()) / 111_139.0;
                let lat_rad = self.center_lat.to_radians();
                let d_lon = (self.radius_m * theta.cos()) / (111_139.0 * lat_rad.cos().max(0.001));

                let lat = self.center_lat + d_lat;
                let lon = self.center_lon + d_lon;
                let heading = ((theta.to_degrees() + 90.0) % 360.0 + 360.0) % 360.0;
                (lat, lon, heading)
            }
            TrajectoryKind::Linear => {
                let distance = self.current_step as f64 * self.step_secs * self.speed_mps;
                let bearing_rad = 45.0_f64.to_radians();
                let d_lat = (distance * bearing_rad.cos()) / 111_139.0;
                let lat_rad = self.center_lat.to_radians();
                let d_lon = (distance * bearing_rad.sin()) / (111_139.0 * lat_rad.cos().max(0.001));

                (self.center_lat + d_lat, self.center_lon + d_lon, 45.0)
            }
            TrajectoryKind::Sine => {
                let distance_north = self.current_step as f64 * self.step_secs * self.speed_mps;
                let d_lat = distance_north / 111_139.0;
                let wave_freq = 0.05;
                let d_east = (self.current_step as f64 * wave_freq).sin() * 150.0;
                let lat_rad = (self.center_lat + d_lat).to_radians();
                let d_lon = d_east / (111_139.0 * lat_rad.cos().max(0.001));

                let heading = 10.0 + (self.current_step as f64 * wave_freq).cos() * 20.0;
                (
                    self.center_lat + d_lat,
                    self.center_lon + d_lon,
                    (heading % 360.0 + 360.0) % 360.0,
                )
            }
        };

        self.current_step += 1;

        let now = time::OffsetDateTime::now_utc();
        let timestamp = Timestamp {
            seconds: now.unix_timestamp(),
            nanos: now.nanosecond() as i32,
        };

        LocationSample {
            sequence: seq,
            captured_at: Some(timestamp),
            position: Some(Position {
                latitude: (lat * 1_000_000.0).round() / 1_000_000.0,
                longitude: (lon * 1_000_000.0).round() / 1_000_000.0,
                horizontal_accuracy_m: 5.0,
                altitude_m: Some(105.0),
                vertical_accuracy_m: None,
                speed_mps: Some((self.speed_mps * 10.0).round() / 10.0),
                speed_accuracy_mps: None,
                heading_deg: Some((heading * 10.0).round() / 10.0),
                heading_accuracy_deg: None,
            }),
            quality: Some(LocationQuality {
                source: LocationSource::Gnss as i32,
                kind: LocationKind::Measured as i32,
                is_mock: Some(false),
                motion: self.motion as i32,
            }),
        }
    }
}
