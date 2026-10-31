//! Domain types and pure validation for location samples and positions.
//!
//! Governed by docs/09_VALIDATION_ERRORS.md §3 and docs/05_DOMAIN_MODEL.md §9.
//! All coordinate types implement redacted `Debug` and `Display` (docs/10_SECURITY_PRIVACY.md §8).
#![forbid(unsafe_code)]

use std::fmt;
use std::str::FromStr;
use time::OffsetDateTime;

use crate::ids::{DeviceId, MemberId, SessionId};

/// Maximum allowed sequence number: 2^53 - 1 (RFC 7946 / JS Number.MAX_SAFE_INTEGER).
pub const MAX_SEQUENCE: u64 = 9_007_199_254_740_991;

/// Minimum timestamp seconds in Protobuf / W3C range (-62135596800 = 0001-01-01T00:00:00Z).
pub const MIN_TIMESTAMP_SECONDS: i64 = -62_135_596_800;

/// Maximum timestamp seconds in Protobuf / W3C range (253402300799 = 9999-12-31T23:59:59Z).
pub const MAX_TIMESTAMP_SECONDS: i64 = 253_402_300_799;

// ============================================================================
// Enums
// ============================================================================

/// Origin / provider of a location measurement.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum LocationSource {
    #[default]
    #[serde(rename = "UNSPECIFIED", alias = "LOCATION_SOURCE_UNSPECIFIED")]
    Unspecified,
    #[serde(rename = "GNSS", alias = "LOCATION_SOURCE_GNSS")]
    Gnss,
    #[serde(rename = "NETWORK", alias = "LOCATION_SOURCE_NETWORK")]
    Network,
    #[serde(rename = "FUSED", alias = "LOCATION_SOURCE_FUSED")]
    Fused,
    #[serde(rename = "MANUAL", alias = "LOCATION_SOURCE_MANUAL")]
    Manual,
    #[serde(rename = "EXTERNAL_DEVICE", alias = "LOCATION_SOURCE_EXTERNAL_DEVICE")]
    ExternalDevice,
}

impl fmt::Display for LocationSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unspecified => write!(f, "UNSPECIFIED"),
            Self::Gnss => write!(f, "GNSS"),
            Self::Network => write!(f, "NETWORK"),
            Self::Fused => write!(f, "FUSED"),
            Self::Manual => write!(f, "MANUAL"),
            Self::ExternalDevice => write!(f, "EXTERNAL_DEVICE"),
        }
    }
}

impl FromStr for LocationSource {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "GNSS" | "LOCATION_SOURCE_GNSS" => Ok(Self::Gnss),
            "NETWORK" | "LOCATION_SOURCE_NETWORK" => Ok(Self::Network),
            "FUSED" | "LOCATION_SOURCE_FUSED" => Ok(Self::Fused),
            "MANUAL" | "LOCATION_SOURCE_MANUAL" => Ok(Self::Manual),
            "EXTERNAL_DEVICE" | "LOCATION_SOURCE_EXTERNAL_DEVICE" => Ok(Self::ExternalDevice),
            "UNSPECIFIED" | "LOCATION_SOURCE_UNSPECIFIED" => Ok(Self::Unspecified),
            _ => Ok(Self::Unspecified),
        }
    }
}

/// Nature of location sample. Derived samples are never MEASURED.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum LocationKind {
    #[default]
    #[serde(rename = "UNSPECIFIED", alias = "LOCATION_KIND_UNSPECIFIED")]
    Unspecified,
    #[serde(rename = "MEASURED", alias = "LOCATION_KIND_MEASURED")]
    Measured,
    #[serde(rename = "FILTERED", alias = "LOCATION_KIND_FILTERED")]
    Filtered,
    #[serde(rename = "INTERPOLATED", alias = "LOCATION_KIND_INTERPOLATED")]
    Interpolated,
    #[serde(rename = "PREDICTED", alias = "LOCATION_KIND_PREDICTED")]
    Predicted,
    #[serde(rename = "MAP_MATCHED", alias = "LOCATION_KIND_MAP_MATCHED")]
    MapMatched,
}

impl fmt::Display for LocationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unspecified => write!(f, "UNSPECIFIED"),
            Self::Measured => write!(f, "MEASURED"),
            Self::Filtered => write!(f, "FILTERED"),
            Self::Interpolated => write!(f, "INTERPOLATED"),
            Self::Predicted => write!(f, "PREDICTED"),
            Self::MapMatched => write!(f, "MAP_MATCHED"),
        }
    }
}

impl FromStr for LocationKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "MEASURED" | "LOCATION_KIND_MEASURED" => Ok(Self::Measured),
            "FILTERED" | "LOCATION_KIND_FILTERED" => Ok(Self::Filtered),
            "INTERPOLATED" | "LOCATION_KIND_INTERPOLATED" => Ok(Self::Interpolated),
            "PREDICTED" | "LOCATION_KIND_PREDICTED" => Ok(Self::Predicted),
            "MAP_MATCHED" | "LOCATION_KIND_MAP_MATCHED" => Ok(Self::MapMatched),
            "UNSPECIFIED" | "LOCATION_KIND_UNSPECIFIED" => Ok(Self::Unspecified),
            _ => Ok(Self::Unspecified),
        }
    }
}

/// Motion activity estimated by platform sensors.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum MotionActivity {
    #[default]
    #[serde(rename = "UNSPECIFIED", alias = "MOTION_ACTIVITY_UNSPECIFIED")]
    Unspecified,
    #[serde(rename = "STATIONARY", alias = "MOTION_ACTIVITY_STATIONARY")]
    Stationary,
    #[serde(rename = "WALKING", alias = "MOTION_ACTIVITY_WALKING")]
    Walking,
    #[serde(rename = "RUNNING", alias = "MOTION_ACTIVITY_RUNNING")]
    Running,
    #[serde(rename = "CYCLING", alias = "MOTION_ACTIVITY_CYCLING")]
    Cycling,
    #[serde(rename = "AUTOMOTIVE", alias = "MOTION_ACTIVITY_AUTOMOTIVE")]
    Automotive,
}

impl fmt::Display for MotionActivity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unspecified => write!(f, "UNSPECIFIED"),
            Self::Stationary => write!(f, "STATIONARY"),
            Self::Walking => write!(f, "WALKING"),
            Self::Running => write!(f, "RUNNING"),
            Self::Cycling => write!(f, "CYCLING"),
            Self::Automotive => write!(f, "AUTOMOTIVE"),
        }
    }
}

impl FromStr for MotionActivity {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "STATIONARY" | "MOTION_ACTIVITY_STATIONARY" => Ok(Self::Stationary),
            "WALKING" | "MOTION_ACTIVITY_WALKING" => Ok(Self::Walking),
            "RUNNING" | "MOTION_ACTIVITY_RUNNING" => Ok(Self::Running),
            "CYCLING" | "MOTION_ACTIVITY_CYCLING" => Ok(Self::Cycling),
            "AUTOMOTIVE" | "MOTION_ACTIVITY_AUTOMOTIVE" => Ok(Self::Automotive),
            "UNSPECIFIED" | "MOTION_ACTIVITY_UNSPECIFIED" => Ok(Self::Unspecified),
            _ => Ok(Self::Unspecified),
        }
    }
}

/// Evaluation result status for a location sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SampleStatus {
    #[serde(rename = "ACCEPTED", alias = "SAMPLE_STATUS_ACCEPTED")]
    Accepted,
    #[serde(
        rename = "IGNORED_DUPLICATE",
        alias = "SAMPLE_STATUS_IGNORED_DUPLICATE"
    )]
    IgnoredDuplicate,
    #[serde(rename = "IGNORED_STALE", alias = "SAMPLE_STATUS_IGNORED_STALE")]
    IgnoredStale,
    #[serde(rename = "REJECTED", alias = "SAMPLE_STATUS_REJECTED")]
    Rejected,
}

impl fmt::Display for SampleStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Accepted => write!(f, "ACCEPTED"),
            Self::IgnoredDuplicate => write!(f, "IGNORED_DUPLICATE"),
            Self::IgnoredStale => write!(f, "IGNORED_STALE"),
            Self::Rejected => write!(f, "REJECTED"),
        }
    }
}

// ============================================================================
// Policy & Error Types
// ============================================================================

/// Effective policy configuration for location sample validation.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LocationPolicy {
    pub max_horizontal_accuracy_m: f64,
    pub reject_null_island: bool,
    pub max_future_skew_ms: i64,
    pub max_event_age_ms: i64,
    pub max_samples_per_publish: u32,
    /// Regla 17 (DR-20261031-02): rechazar `quality.is_mock = true`.
    #[serde(default)]
    pub reject_mock_samples: bool,
    /// Regla 18 (DR-20261031-02): rechazar `quality.source = NETWORK`.
    #[serde(default)]
    pub reject_network_only: bool,
}

impl Default for LocationPolicy {
    fn default() -> Self {
        Self {
            max_horizontal_accuracy_m: 50_000.0,
            reject_null_island: true,
            max_future_skew_ms: 5_000,
            max_event_age_ms: 120_000,
            max_samples_per_publish: 50,
            reject_mock_samples: false,
            reject_network_only: false,
        }
    }
}

/// Validation error produced during sample or position validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid location sample at field '{field}': {error_code}")]
pub struct LocationValidationError {
    pub error_code: &'static str,
    pub field: &'static str,
}

impl LocationValidationError {
    pub const fn new(field: &'static str) -> Self {
        Self {
            error_code: "INVALID_LOCATION",
            field,
        }
    }
}

/// Result of evaluating a single sample during publication ingestion.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SampleResult {
    pub sequence: u64,
    pub status: SampleStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

impl SampleResult {
    pub fn accepted(sequence: u64) -> Self {
        Self {
            sequence,
            status: SampleStatus::Accepted,
            error_code: None,
            field: None,
        }
    }

    pub fn ignored_duplicate(sequence: u64) -> Self {
        Self {
            sequence,
            status: SampleStatus::IgnoredDuplicate,
            error_code: None,
            field: None,
        }
    }

    pub fn ignored_stale(sequence: u64) -> Self {
        Self {
            sequence,
            status: SampleStatus::IgnoredStale,
            error_code: None,
            field: None,
        }
    }

    pub fn rejected(
        sequence: u64,
        error_code: impl Into<String>,
        field: impl Into<String>,
    ) -> Self {
        Self {
            sequence,
            status: SampleStatus::Rejected,
            error_code: Some(error_code.into()),
            field: Some(field.into()),
        }
    }
}

// ============================================================================
// Domain Position (Redacted Coordinates)
// ============================================================================

/// Validated geographic position in WGS 84.
///
/// Coordinates are private and access is provided via getters.
/// Both `Debug` and `Display` are redacted to prevent leaking coordinates in logs.
#[derive(Clone, PartialEq)]
pub struct Position {
    latitude: f64,
    longitude: f64,
    horizontal_accuracy_m: f64,
    altitude_m: Option<f64>,
    vertical_accuracy_m: Option<f64>,
    speed_mps: Option<f64>,
    speed_accuracy_mps: Option<f64>,
    heading_deg: Option<f64>,
    heading_accuracy_deg: Option<f64>,
}

impl Position {
    /// Validates and constructs a new `Position` according to domain rules 4–13.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        latitude: f64,
        longitude: f64,
        horizontal_accuracy_m: f64,
        altitude_m: Option<f64>,
        vertical_accuracy_m: Option<f64>,
        speed_mps: Option<f64>,
        speed_accuracy_mps: Option<f64>,
        heading_deg: Option<f64>,
        heading_accuracy_deg: Option<f64>,
        policy: &LocationPolicy,
    ) -> Result<Self, LocationValidationError> {
        // Rule 4: latitude finite and [-90, 90]
        if !latitude.is_finite() || !(-90.0..=90.0).contains(&latitude) {
            return Err(LocationValidationError::new("position.latitude"));
        }
        // Rule 5: longitude finite and [-180, 180]
        if !longitude.is_finite() || !(-180.0..=180.0).contains(&longitude) {
            return Err(LocationValidationError::new("position.longitude"));
        }
        // Rule 6: null island exact match if policy dictates
        if policy.reject_null_island && latitude == 0.0 && longitude == 0.0 {
            return Err(LocationValidationError::new("position"));
        }
        // Rule 7: horizontal_accuracy_m finite, > 0, <= policy max
        if !horizontal_accuracy_m.is_finite()
            || horizontal_accuracy_m <= 0.0
            || horizontal_accuracy_m > policy.max_horizontal_accuracy_m
        {
            return Err(LocationValidationError::new(
                "position.horizontal_accuracy_m",
            ));
        }
        // Rule 8: altitude_m optional finite [-1000, 50000]
        if let Some(alt) = altitude_m
            && (!alt.is_finite() || !(-1000.0..=50000.0).contains(&alt))
        {
            return Err(LocationValidationError::new("position.altitude_m"));
        }
        // Rule 9: vertical_accuracy_m optional finite (0, 10000]
        if let Some(va) = vertical_accuracy_m
            && (!va.is_finite() || va <= 0.0 || va > 10_000.0)
        {
            return Err(LocationValidationError::new("position.vertical_accuracy_m"));
        }
        // Rule 10: speed_mps optional finite [0, 300]
        if let Some(speed) = speed_mps
            && (!speed.is_finite() || !(0.0..=300.0).contains(&speed))
        {
            return Err(LocationValidationError::new("position.speed_mps"));
        }
        // Rule 11: speed_accuracy_mps optional finite (0, 300]
        if let Some(sa) = speed_accuracy_mps
            && (!sa.is_finite() || sa <= 0.0 || sa > 300.0)
        {
            return Err(LocationValidationError::new("position.speed_accuracy_mps"));
        }
        // Rule 12: heading_deg optional finite [0, 360)
        if let Some(hd) = heading_deg
            && (!hd.is_finite() || hd < 0.0 || hd >= 360.0)
        {
            return Err(LocationValidationError::new("position.heading_deg"));
        }
        // Rule 13: heading_accuracy_deg optional finite (0, 180]
        if let Some(ha) = heading_accuracy_deg
            && (!ha.is_finite() || ha <= 0.0 || ha > 180.0)
        {
            return Err(LocationValidationError::new(
                "position.heading_accuracy_deg",
            ));
        }

        Ok(Self {
            latitude,
            longitude,
            horizontal_accuracy_m,
            altitude_m,
            vertical_accuracy_m,
            speed_mps,
            speed_accuracy_mps,
            heading_deg,
            heading_accuracy_deg,
        })
    }

    pub fn latitude(&self) -> f64 {
        self.latitude
    }

    pub fn longitude(&self) -> f64 {
        self.longitude
    }

    pub fn horizontal_accuracy_m(&self) -> f64 {
        self.horizontal_accuracy_m
    }

    pub fn altitude_m(&self) -> Option<f64> {
        self.altitude_m
    }

    pub fn vertical_accuracy_m(&self) -> Option<f64> {
        self.vertical_accuracy_m
    }

    pub fn speed_mps(&self) -> Option<f64> {
        self.speed_mps
    }

    pub fn speed_accuracy_mps(&self) -> Option<f64> {
        self.speed_accuracy_mps
    }

    pub fn heading_deg(&self) -> Option<f64> {
        self.heading_deg
    }

    pub fn heading_accuracy_deg(&self) -> Option<f64> {
        self.heading_accuracy_deg
    }
}

impl fmt::Debug for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Position { <redacted> }")
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Position { <redacted> }")
    }
}

// ============================================================================
// LocationQuality, LocationSample, AcceptedLocation
// ============================================================================

/// Metadata describing measurement quality.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct LocationQuality {
    pub source: LocationSource,
    pub kind: LocationKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_mock: Option<bool>,
    pub motion: MotionActivity,
}

impl LocationQuality {
    pub fn new(
        source: LocationSource,
        kind: LocationKind,
        is_mock: Option<bool>,
        motion: MotionActivity,
    ) -> Result<Self, LocationValidationError> {
        if kind == LocationKind::Unspecified {
            return Err(LocationValidationError::new("quality.kind"));
        }
        Ok(Self {
            source,
            kind,
            is_mock,
            motion,
        })
    }
}

/// A validated location sample published by a device.
#[derive(Clone, PartialEq)]
pub struct LocationSample {
    pub sequence: u64,
    pub captured_at: OffsetDateTime,
    pub position: Position,
    pub quality: LocationQuality,
}

impl fmt::Debug for LocationSample {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocationSample")
            .field("sequence", &self.sequence)
            .field("captured_at", &self.captured_at)
            .field("position", &"<redacted>")
            .field("quality", &self.quality)
            .finish()
    }
}

impl fmt::Display for LocationSample {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "LocationSample(seq={}, captured_at={}, <redacted>)",
            self.sequence, self.captured_at
        )
    }
}

/// A location accepted by the server and assigned a received timestamp.
#[derive(Clone, PartialEq)]
pub struct AcceptedLocation {
    pub session_id: SessionId,
    pub member_id: MemberId,
    pub device_id: DeviceId,
    pub sample: LocationSample,
    pub received_at: OffsetDateTime,
}

impl fmt::Debug for AcceptedLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AcceptedLocation")
            .field("session_id", &self.session_id)
            .field("member_id", &self.member_id)
            .field("device_id", &self.device_id)
            .field("sample", &self.sample)
            .field("received_at", &self.received_at)
            .finish()
    }
}

impl fmt::Display for AcceptedLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "AcceptedLocation(session={}, member={}, device={}, seq={}, <redacted>)",
            self.session_id, self.member_id, self.device_id, self.sample.sequence
        )
    }
}

// ============================================================================
// Raw (Unvalidated) Input Types for Validation Pipeline
// ============================================================================

/// Unvalidated raw location sample as received from wire or JSON.
#[derive(Debug, Clone, Default)]
pub struct RawLocationSample {
    pub sequence: Option<u64>,
    pub sequence_invalid: bool,
    pub captured_at: Option<OffsetDateTime>,
    pub captured_at_invalid: bool,
    pub position: Option<RawPosition>,
    pub quality: Option<RawQuality>,
}

/// Unvalidated raw position fields.
#[derive(Debug, Clone, Default)]
pub struct RawPosition {
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub horizontal_accuracy_m: Option<f64>,
    pub altitude_m: Option<f64>,
    pub vertical_accuracy_m: Option<f64>,
    pub speed_mps: Option<f64>,
    pub speed_accuracy_mps: Option<f64>,
    pub heading_deg: Option<f64>,
    pub heading_accuracy_deg: Option<f64>,
}

/// Unvalidated raw quality fields.
#[derive(Debug, Clone, Default)]
pub struct RawQuality {
    pub source: Option<LocationSource>,
    pub kind: Option<LocationKind>,
    pub is_mock: Option<bool>,
    pub motion: Option<MotionActivity>,
}

impl RawLocationSample {
    /// Parses an unvalidated sample from a JSON value (REST / vector representation).
    pub fn from_json(val: &serde_json::Value) -> Self {
        let (sequence, sequence_invalid) = match val.get("sequence") {
            None | Some(serde_json::Value::Null) => (None, false),
            Some(serde_json::Value::Number(n)) => {
                if let Some(u) = n.as_u64() {
                    (Some(u), false)
                } else {
                    (None, true)
                }
            }
            Some(serde_json::Value::String(s)) => match s.parse::<u64>() {
                Ok(u) => (Some(u), false),
                Err(_) => (None, true),
            },
            _ => (None, true),
        };

        let (captured_at, captured_at_invalid) = match val.get("captured_at") {
            None | Some(serde_json::Value::Null) => (None, false),
            Some(serde_json::Value::String(s)) => {
                match OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339) {
                    Ok(dt) => {
                        let secs = dt.unix_timestamp();
                        let nanos = dt.nanosecond();
                        if (MIN_TIMESTAMP_SECONDS..=MAX_TIMESTAMP_SECONDS).contains(&secs)
                            && nanos < 1_000_000_000
                        {
                            (Some(dt), false)
                        } else {
                            (None, true)
                        }
                    }
                    Err(_) => (None, true),
                }
            }
            _ => (None, true),
        };

        let position = match val.get("position") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::Object(pos)) => Some(RawPosition {
                latitude: parse_flexible_float(pos.get("latitude")),
                longitude: parse_flexible_float(pos.get("longitude")),
                horizontal_accuracy_m: parse_flexible_float(pos.get("horizontal_accuracy_m")),
                altitude_m: parse_flexible_float(pos.get("altitude_m")),
                vertical_accuracy_m: parse_flexible_float(pos.get("vertical_accuracy_m")),
                speed_mps: parse_flexible_float(pos.get("speed_mps")),
                speed_accuracy_mps: parse_flexible_float(pos.get("speed_accuracy_mps")),
                heading_deg: parse_flexible_float(pos.get("heading_deg")),
                heading_accuracy_deg: parse_flexible_float(pos.get("heading_accuracy_deg")),
            }),
            _ => None,
        };

        let quality = match val.get("quality") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::Object(q)) => {
                let source = match q.get("source") {
                    None | Some(serde_json::Value::Null) => Some(LocationSource::Unspecified),
                    Some(serde_json::Value::String(s)) => LocationSource::from_str(s).ok(),
                    _ => Some(LocationSource::Unspecified),
                };

                let kind = match q.get("kind") {
                    None | Some(serde_json::Value::Null) => None,
                    Some(serde_json::Value::String(s)) => LocationKind::from_str(s).ok(),
                    _ => None,
                };

                let is_mock = match q.get("is_mock") {
                    Some(serde_json::Value::Bool(b)) => Some(*b),
                    _ => None,
                };

                let motion = match q.get("motion") {
                    None | Some(serde_json::Value::Null) => Some(MotionActivity::Unspecified),
                    Some(serde_json::Value::String(s)) => MotionActivity::from_str(s).ok(),
                    _ => Some(MotionActivity::Unspecified),
                };

                Some(RawQuality {
                    source,
                    kind,
                    is_mock,
                    motion,
                })
            }
            _ => None,
        };

        Self {
            sequence,
            sequence_invalid,
            captured_at,
            captured_at_invalid,
            position,
            quality,
        }
    }
}

fn parse_flexible_float(val: Option<&serde_json::Value>) -> Option<f64> {
    match val? {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => match s.as_str() {
            "NaN" => Some(f64::NAN),
            "Infinity" | "+Infinity" => Some(f64::INFINITY),
            "-Infinity" => Some(f64::NEG_INFINITY),
            other => other.parse::<f64>().ok().or(Some(f64::NAN)),
        },
        serde_json::Value::Null => None,
        _ => Some(f64::NAN),
    }
}

// ============================================================================
// Pure Validation Function (Rules 1 to 16)
// ============================================================================

/// Pure and deterministic validation of a location sample according to docs/09 §3.1.
///
/// Returns `Ok(LocationSample)` if all 16 rules pass in exact order.
/// Returns `Err(LocationValidationError)` on the first rule failure.
pub fn validate_sample(
    sample: &RawLocationSample,
    policy: &LocationPolicy,
    now: OffsetDateTime,
) -> Result<LocationSample, LocationValidationError> {
    // 1. sequence: obligatory, integer in [1, 9007199254740991] (2^53 - 1)
    if sample.sequence_invalid {
        return Err(LocationValidationError::new("sequence"));
    }
    let sequence = match sample.sequence {
        Some(seq) if (1..=MAX_SEQUENCE).contains(&seq) => seq,
        _ => return Err(LocationValidationError::new("sequence")),
    };

    // 2. captured_at: obligatory, valid Timestamp seconds in [-62135596800, 253402300799], nanos in [0, 999999999]
    if sample.captured_at_invalid || sample.captured_at.is_none() {
        return Err(LocationValidationError::new("captured_at"));
    }
    let captured_at = sample.captured_at.unwrap();

    // 3. position: obligatory, present
    let raw_pos = match &sample.position {
        Some(p) => p,
        None => return Err(LocationValidationError::new("position")),
    };

    // 4. position.latitude: obligatory, finite, [-90, 90]
    let latitude = match raw_pos.latitude {
        Some(lat) if lat.is_finite() && (-90.0..=90.0).contains(&lat) => lat,
        _ => return Err(LocationValidationError::new("position.latitude")),
    };

    // 5. position.longitude: obligatory, finite, [-180, 180]
    let longitude = match raw_pos.longitude {
        Some(lon) if lon.is_finite() && (-180.0..=180.0).contains(&lon) => lon,
        _ => return Err(LocationValidationError::new("position.longitude")),
    };

    // 6. position null island: if reject_null_island, reject if latitude == 0.0 && longitude == 0.0
    if policy.reject_null_island && latitude == 0.0 && longitude == 0.0 {
        return Err(LocationValidationError::new("position"));
    }

    // 7. position.horizontal_accuracy_m: obligatory, finite, > 0, <= max_horizontal_accuracy_m
    let horizontal_accuracy_m = match raw_pos.horizontal_accuracy_m {
        Some(acc) if acc.is_finite() && acc > 0.0 && acc <= policy.max_horizontal_accuracy_m => acc,
        _ => {
            return Err(LocationValidationError::new(
                "position.horizontal_accuracy_m",
            ));
        }
    };

    // 8. position.altitude_m: optional, finite, [-1000, 50000]
    if let Some(alt) = raw_pos.altitude_m
        && (!alt.is_finite() || !(-1000.0..=50000.0).contains(&alt))
    {
        return Err(LocationValidationError::new("position.altitude_m"));
    }

    // 9. position.vertical_accuracy_m: optional, finite, (0, 10000]
    if let Some(va) = raw_pos.vertical_accuracy_m
        && (!va.is_finite() || va <= 0.0 || va > 10_000.0)
    {
        return Err(LocationValidationError::new("position.vertical_accuracy_m"));
    }

    // 10. position.speed_mps: optional, finite, [0, 300]
    if let Some(speed) = raw_pos.speed_mps
        && (!speed.is_finite() || !(0.0..=300.0).contains(&speed))
    {
        return Err(LocationValidationError::new("position.speed_mps"));
    }

    // 11. position.speed_accuracy_mps: optional, finite, (0, 300]
    if let Some(sa) = raw_pos.speed_accuracy_mps
        && (!sa.is_finite() || sa <= 0.0 || sa > 300.0)
    {
        return Err(LocationValidationError::new("position.speed_accuracy_mps"));
    }

    // 12. position.heading_deg: optional, finite, [0, 360)
    if let Some(hd) = raw_pos.heading_deg
        && (!hd.is_finite() || hd < 0.0 || hd >= 360.0)
    {
        return Err(LocationValidationError::new("position.heading_deg"));
    }

    // 13. position.heading_accuracy_deg: optional, finite, (0, 180]
    if let Some(ha) = raw_pos.heading_accuracy_deg
        && (!ha.is_finite() || ha <= 0.0 || ha > 180.0)
    {
        return Err(LocationValidationError::new(
            "position.heading_accuracy_deg",
        ));
    }

    // 14. quality: obligatory, present
    let raw_quality = match &sample.quality {
        Some(q) => q,
        None => return Err(LocationValidationError::new("quality")),
    };

    // 15. quality.kind: obligatory, known value != UNSPECIFIED
    let kind = match raw_quality.kind {
        Some(k) if k != LocationKind::Unspecified => k,
        _ => return Err(LocationValidationError::new("quality.kind")),
    };

    // 16. captured_at <= now + GS_LOCATION_MAX_FUTURE_SKEW_MS
    let max_future_skew = time::Duration::milliseconds(policy.max_future_skew_ms);
    let max_future = match now.checked_add(max_future_skew) {
        Some(t) => t,
        None => return Err(LocationValidationError::new("captured_at")),
    };
    if captured_at > max_future {
        return Err(LocationValidationError::new("captured_at"));
    }

    let position = Position {
        latitude,
        longitude,
        horizontal_accuracy_m,
        altitude_m: raw_pos.altitude_m,
        vertical_accuracy_m: raw_pos.vertical_accuracy_m,
        speed_mps: raw_pos.speed_mps,
        speed_accuracy_mps: raw_pos.speed_accuracy_mps,
        heading_deg: raw_pos.heading_deg,
        heading_accuracy_deg: raw_pos.heading_accuracy_deg,
    };

    // 17. quality.is_mock (política opcional del tenant, DR-20261031-02)
    if policy.reject_mock_samples && raw_quality.is_mock == Some(true) {
        return Err(LocationValidationError::new("quality.is_mock"));
    }

    // 18. quality.source = NETWORK (política opcional del tenant, DR-20261031-02)
    let source = raw_quality.source.unwrap_or(LocationSource::Unspecified);
    if policy.reject_network_only && source == LocationSource::Network {
        return Err(LocationValidationError::new("quality.source"));
    }

    let quality = LocationQuality {
        source,
        kind,
        is_mock: raw_quality.is_mock,
        motion: raw_quality.motion.unwrap_or(MotionActivity::Unspecified),
    };

    Ok(LocationSample {
        sequence,
        captured_at,
        position,
        quality,
    })
}

/// Evaluates a single sample through the acceptance algorithm (validation, age, sequence order).
pub fn evaluate_sample(
    sample: &RawLocationSample,
    policy: &LocationPolicy,
    now: OffsetDateTime,
    last_sequence: Option<u64>,
) -> SampleResult {
    // 2. Pure validation (§3.1, rules 1–16)
    let validated = match validate_sample(sample, policy, now) {
        Ok(s) => s,
        Err(e) => {
            return SampleResult::rejected(sample.sequence.unwrap_or(0), e.error_code, e.field);
        }
    };

    // 3. Stale check (§3.2, captured_at < now - max_event_age)
    let max_age = time::Duration::milliseconds(policy.max_event_age_ms);
    let min_allowed = match now.checked_sub(max_age) {
        Some(t) => t,
        None => now,
    };
    if validated.captured_at < min_allowed {
        return SampleResult::ignored_stale(validated.sequence);
    }

    // 4. Sequence ordering
    match last_sequence {
        None => SampleResult::accepted(validated.sequence),
        Some(prev) if validated.sequence == prev => {
            SampleResult::ignored_duplicate(validated.sequence)
        }
        Some(prev) if validated.sequence < prev => SampleResult::ignored_stale(validated.sequence),
        Some(_) => SampleResult::accepted(validated.sequence),
    }
}

// ============================================================================
// Sequence Ordering Model & Batch Acceptance Pipeline (FR-LOC-003, docs/09 §4)
// ============================================================================

/// Result of an atomic sequence compare-and-set operation (docs/09 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CasResult {
    Accepted,
    Duplicate,
    Stale,
}

impl fmt::Display for CasResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Accepted => write!(f, "ACCEPTED"),
            Self::Duplicate => write!(f, "IGNORED_DUPLICATE"),
            Self::Stale => write!(f, "IGNORED_STALE"),
        }
    }
}

/// In-memory sequence store implementing the CAS ordering model for simulated and local test stores.
#[derive(Debug, Clone, Default)]
pub struct InMemorySequenceStore {
    last_sequence: Option<u64>,
}

impl InMemorySequenceStore {
    pub fn new(initial: Option<u64>) -> Self {
        Self {
            last_sequence: initial,
        }
    }

    pub fn current_sequence(&self) -> Option<u64> {
        self.last_sequence
    }

    /// Atomic compare-and-set according to docs/09 §4:
    /// - If no previous sequence exists -> update and Accepted
    /// - If seq > prev -> update and Accepted
    /// - If seq == prev -> Duplicate (no update)
    /// - If seq < prev -> Stale (no update)
    pub fn compare_and_set(&mut self, seq: u64) -> CasResult {
        match self.last_sequence {
            None => {
                self.last_sequence = Some(seq);
                CasResult::Accepted
            }
            Some(prev) if seq == prev => CasResult::Duplicate,
            Some(prev) if seq < prev => CasResult::Stale,
            Some(_) => {
                self.last_sequence = Some(seq);
                CasResult::Accepted
            }
        }
    }
}

/// Precondition errors that reject an entire batch before per-sample processing (docs/08 §6.4).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, thiserror::Error,
)]
pub enum BatchPreconditionError {
    #[error("VALIDATION_FAILED")]
    #[serde(rename = "VALIDATION_FAILED")]
    ValidationFailed,
    #[error("PAYLOAD_TOO_LARGE")]
    #[serde(rename = "PAYLOAD_TOO_LARGE")]
    PayloadTooLarge,
}

/// Outcome of processing a batch of location samples.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BatchEvaluationResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_error: Option<BatchPreconditionError>,
    pub results: Vec<SampleResult>,
    pub final_last_sequence: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_sequence: Option<u64>,
}

/// Processes a batch of location samples through preconditions, pure validation,
/// age checking, and CAS sequence ordering according to docs/09 §4.
pub fn process_sample_batch(
    samples: &[RawLocationSample],
    policy: &LocationPolicy,
    now: OffsetDateTime,
    store: &mut InMemorySequenceStore,
) -> BatchEvaluationResult {
    let current_seq = store.current_sequence().unwrap_or(0);

    // 1. Batch preconditions (docs/08 §6.4, docs/09 §4)
    if samples.is_empty() {
        return BatchEvaluationResult {
            request_error: Some(BatchPreconditionError::ValidationFailed),
            results: Vec::new(),
            final_last_sequence: current_seq,
            snapshot_sequence: None,
        };
    }

    if samples.len() > policy.max_samples_per_publish as usize {
        return BatchEvaluationResult {
            request_error: Some(BatchPreconditionError::PayloadTooLarge),
            results: Vec::new(),
            final_last_sequence: current_seq,
            snapshot_sequence: None,
        };
    }

    // Check strictly increasing sequence numbers within the batch
    for window in samples.windows(2) {
        match (window[0].sequence, window[1].sequence) {
            (Some(a), Some(b)) if b > a => {}
            _ => {
                return BatchEvaluationResult {
                    request_error: Some(BatchPreconditionError::ValidationFailed),
                    results: Vec::new(),
                    final_last_sequence: current_seq,
                    snapshot_sequence: None,
                };
            }
        }
    }

    let mut results = Vec::with_capacity(samples.len());
    let mut last_accepted = None;

    for s in samples {
        // 2. Pure validation (§3.1, rules 1–16)
        let validated = match validate_sample(s, policy, now) {
            Ok(v) => v,
            Err(e) => {
                results.push(SampleResult::rejected(
                    s.sequence.unwrap_or(0),
                    e.error_code,
                    e.field,
                ));
                continue;
            }
        };

        // 3. Age check (§3.2, captured_at < now - max_event_age)
        let max_age = time::Duration::milliseconds(policy.max_event_age_ms);
        let min_allowed = match now.checked_sub(max_age) {
            Some(t) => t,
            None => now,
        };
        if validated.captured_at < min_allowed {
            results.push(SampleResult::ignored_stale(validated.sequence));
            continue;
        }

        // 4. Order / CAS atomic check in sequence store
        match store.compare_and_set(validated.sequence) {
            CasResult::Duplicate => {
                results.push(SampleResult::ignored_duplicate(validated.sequence));
            }
            CasResult::Stale => {
                results.push(SampleResult::ignored_stale(validated.sequence));
            }
            CasResult::Accepted => {
                results.push(SampleResult::accepted(validated.sequence));
                last_accepted = Some(validated.sequence);
            }
        }
    }

    BatchEvaluationResult {
        request_error: None,
        results,
        final_last_sequence: store.current_sequence().unwrap_or(current_seq),
        snapshot_sequence: last_accepted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enums_display_and_parse() {
        assert_eq!(LocationSource::Gnss.to_string(), "GNSS");
        assert_eq!(LocationSource::from_str("GNSS"), Ok(LocationSource::Gnss));
        assert_eq!(
            LocationSource::from_str("LOCATION_SOURCE_NETWORK"),
            Ok(LocationSource::Network)
        );
        assert_eq!(
            LocationSource::from_str("UNKNOWN"),
            Ok(LocationSource::Unspecified)
        );

        assert_eq!(LocationKind::Measured.to_string(), "MEASURED");
        assert_eq!(
            LocationKind::from_str("MEASURED"),
            Ok(LocationKind::Measured)
        );
        assert_eq!(
            LocationKind::from_str("LOCATION_KIND_FILTERED"),
            Ok(LocationKind::Filtered)
        );
        assert_eq!(
            LocationKind::from_str("UNKNOWN"),
            Ok(LocationKind::Unspecified)
        );

        assert_eq!(MotionActivity::Automotive.to_string(), "AUTOMOTIVE");
        assert_eq!(
            MotionActivity::from_str("AUTOMOTIVE"),
            Ok(MotionActivity::Automotive)
        );
        assert_eq!(
            MotionActivity::from_str("MOTION_ACTIVITY_CYCLING"),
            Ok(MotionActivity::Cycling)
        );
        assert_eq!(
            MotionActivity::from_str("UNKNOWN"),
            Ok(MotionActivity::Unspecified)
        );

        assert_eq!(SampleStatus::Accepted.to_string(), "ACCEPTED");
        assert_eq!(
            SampleStatus::IgnoredDuplicate.to_string(),
            "IGNORED_DUPLICATE"
        );
        assert_eq!(SampleStatus::IgnoredStale.to_string(), "IGNORED_STALE");
        assert_eq!(SampleStatus::Rejected.to_string(), "REJECTED");
    }

    #[test]
    fn test_location_quality_validation() {
        let err = LocationQuality::new(
            LocationSource::Gnss,
            LocationKind::Unspecified,
            None,
            MotionActivity::Stationary,
        );
        assert_eq!(err, Err(LocationValidationError::new("quality.kind")));

        let ok = LocationQuality::new(
            LocationSource::Gnss,
            LocationKind::Measured,
            Some(false),
            MotionActivity::Walking,
        );
        assert!(ok.is_ok());
    }

    #[test]
    fn test_position_getters() {
        let policy = LocationPolicy::default();
        let pos = Position::new(
            10.0,
            20.0,
            5.0,
            Some(100.0),
            Some(2.5),
            Some(15.0),
            Some(1.0),
            Some(180.0),
            Some(10.0),
            &policy,
        )
        .expect("valid position");

        assert_eq!(pos.latitude(), 10.0);
        assert_eq!(pos.longitude(), 20.0);
        assert_eq!(pos.horizontal_accuracy_m(), 5.0);
        assert_eq!(pos.altitude_m(), Some(100.0));
        assert_eq!(pos.vertical_accuracy_m(), Some(2.5));
        assert_eq!(pos.speed_mps(), Some(15.0));
        assert_eq!(pos.speed_accuracy_mps(), Some(1.0));
        assert_eq!(pos.heading_deg(), Some(180.0));
        assert_eq!(pos.heading_accuracy_deg(), Some(10.0));
    }

    #[test]
    fn test_sample_and_accepted_location_redaction() {
        let policy = LocationPolicy::default();
        let pos = Position::new(8.0, -79.0, 5.0, None, None, None, None, None, None, &policy)
            .expect("valid position");
        let quality = LocationQuality::new(
            LocationSource::Gnss,
            LocationKind::Measured,
            None,
            MotionActivity::Stationary,
        )
        .expect("valid quality");

        let sample = LocationSample {
            sequence: 42,
            captured_at: OffsetDateTime::UNIX_EPOCH,
            position: pos,
            quality,
        };

        let sample_debug = format!("{sample:?}");
        let sample_display = format!("{sample}");
        assert!(sample_debug.contains("<redacted>"));
        assert!(!sample_debug.contains("8.0"));
        assert!(!sample_debug.contains("-79.0"));
        assert!(sample_display.contains("<redacted>"));
        assert!(!sample_display.contains("8.0"));
        assert!(!sample_display.contains("-79.0"));

        let accepted = AcceptedLocation {
            session_id: SessionId::new(),
            member_id: MemberId::new(),
            device_id: DeviceId::new(),
            sample,
            received_at: OffsetDateTime::UNIX_EPOCH,
        };

        let accepted_debug = format!("{accepted:?}");
        let accepted_display = format!("{accepted}");
        assert!(accepted_debug.contains("<redacted>"));
        assert!(!accepted_debug.contains("8.0"));
        assert!(!accepted_debug.contains("-79.0"));
        assert!(accepted_display.contains("<redacted>"));
        assert!(!accepted_display.contains("8.0"));
        assert!(!accepted_display.contains("-79.0"));
    }

    #[test]
    fn test_in_memory_sequence_store_cas() {
        let mut store = InMemorySequenceStore::default();
        assert_eq!(store.current_sequence(), None);

        // First sequence accepted
        assert_eq!(store.compare_and_set(1), CasResult::Accepted);
        assert_eq!(store.current_sequence(), Some(1));

        // Duplicate
        assert_eq!(store.compare_and_set(1), CasResult::Duplicate);
        assert_eq!(store.current_sequence(), Some(1));

        // Monotonic advance
        assert_eq!(store.compare_and_set(5), CasResult::Accepted);
        assert_eq!(store.current_sequence(), Some(5));

        // Stale (seq < current)
        assert_eq!(store.compare_and_set(4), CasResult::Stale);
        assert_eq!(store.compare_and_set(1), CasResult::Stale);
        assert_eq!(store.current_sequence(), Some(5));

        assert_eq!(CasResult::Accepted.to_string(), "ACCEPTED");
        assert_eq!(CasResult::Duplicate.to_string(), "IGNORED_DUPLICATE");
        assert_eq!(CasResult::Stale.to_string(), "IGNORED_STALE");
    }

    #[test]
    fn test_batch_precondition_error_display() {
        assert_eq!(
            BatchPreconditionError::ValidationFailed.to_string(),
            "VALIDATION_FAILED"
        );
        assert_eq!(
            BatchPreconditionError::PayloadTooLarge.to_string(),
            "PAYLOAD_TOO_LARGE"
        );
    }
}
