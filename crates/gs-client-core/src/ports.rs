//! Port types shared by every consumer of the core.
//!
//! Nothing here allocates on a hot path. The core never calls a platform or a
//! network: the values in `Input` come *from* the platform/runtime and the
//! values in `Output` go *to* it. That separation is the whole reason this crate
//! does not depend on `tokio`.
//!
//! Fields use milliseconds everywhere so the core is a pure function of
//! `now_ms`: there is no `SystemTime::now()` inside, which is what makes the
//! property tests deterministic under `20` §7.

#![forbid(unsafe_code)]

/// An opaque cross-restart identifier for an HTTP request issued by the core.
///\n/// The corresponding response comes back as `Input::HttpResponse`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId(pub u64);

/// An opaque identifier for a geofence/region.
pub type RegionId = u64;

/// Action the platform asks the core to perform on land (subject sharing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Subject sharing is off; keep the subscription only.
    Off,
    /// Subject sharing for a real-time trip (`REALTIME_TRIP`).
    RealtimeTrip,
    /// Subject sharing for an idle/always-on device (`REALTIME_IDLE`).
    RealtimeIdle,
    /// Subject sharing in the background (`BACKGROUND_TRACKING`).
    BackgroundTracking,
    /// Store history only, no live publication (`HISTORY_ONLY`).
    HistoryOnly,
}

impl Intent {
    /// A `LocationRequest` change has observable effects only when the intent
    /// actually changes. `Eq`-based deduplication is, by contrast, useless if
    /// every call allocates a fresh struct; here the enum carries nothing, so
    /// it compares for free.
    #[must_use]
    pub const fn is_active(self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// A fix delivered by the platform (FusedLocation / CoreLocation / geolocator).
/// All fields are already converted to SI units by the host adapter.
#[derive(Debug, Clone, Copy)]
pub struct PlatformFix {
    pub lat: f64,
    pub lon: f64,
    pub horizontal_accuracy_m: f64,
    pub altitude_m: Option<f64>,
    pub vertical_accuracy_m: Option<f64>,
    pub speed_mps: Option<f64>,
    pub speed_accuracy_mps: Option<f64>,
    pub heading_deg: Option<f64>,
    pub heading_accuracy_deg: Option<f64>,
    pub timestamp_ms: u64,
    /// `true` when the OS says the position is a mock AppOps report (`12` §11).
    pub is_mock: bool,
    /// `NETWORK`, `GPS` or `unknown`, as a raw string for logging-friendly use
    /// (`platform.source` of `09`); the core never inspects it beyond the
    /// publish gate.
    pub source: &'static str,
    /// Sampled by the platform, not obtained by one of our own reads.
    pub out_of_band: bool,
}

impl PlatformFix {
    /// Whether the fix passed the **local** quality gate that `12` §11 requires
    /// *before* publishing. The server still runs its own gate — this one only
    /// drops the unequivocal cases so the socket is not used for worthless
    /// bytes and the invalid sample is counted.
    #[must_use]
    pub fn passes_local_gate(&self) -> bool {
        // Rule 1 (12 §11): accuracy must be strictly positive and finite.
        if !self.horizontal_accuracy_m.is_finite() || self.horizontal_accuracy_m <= 0.0 {
            return false;
        }
        // Rule 2 (12 §11): coord finite — the server rejects NaN/infinite,
        // but dropping here avoids the trip. The core has no full `Position`
        // validator; that is the boundary's job (already done by the host in the
        // validated case). These are cheap defensive checks.
        if !self.lat.is_finite() || !self.lon.is_finite() {
            return false;
        }
        let accuracy_crit = self.horizontal_accuracy_m < f64::EPSILON;
        if accuracy_crit {
            return false;
        }
        true
    }
}

/// One update delivered by the OS activity recognition.
#[derive(Debug, Clone, Copy)]
pub struct PlatformActivity {
    /// A suggested `MotionState` (STILL/ON_FOOT/BICYCLING/IN_VEHICLE). The core
    /// decides whether to trust it using `confidence`.
    pub motion: u8,
    pub confidence: u8,
}

/// A network reachability report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetState {
    Disconnected,
    Connecting,
    ConnectedWifi,
    ConnectedMobile,
    Unknown,
}

/// A Bluetooth Low Energy advertising packet used for tag relay (E8).
#[derive(Debug, Clone)]
pub struct BleAdvertisement {
    pub payload: Vec<u8>,
    pub rssi_dbm: i8,
    pub observed_at_ms: u64,
}

/// An item the core can ask the platform to scan/geofence.
#[derive(Debug, Clone)]
pub struct LocationRequest {
    pub priority: u8, // 0 = no updates, higher = stronger
    pub interval_ms: u32,
    pub smallest_displacement_m: f32,
    pub background: bool,
    pub batch_deadline_ms: u32,
}

impl LocationRequest {
    /// Value that requests nothing: used to stop the platform location service.
    #[must_use]
    pub const fn off() -> Self {
        Self {
            priority: 0,
            interval_ms: u32::MAX,
            smallest_displacement_m: 0.0,
            background: false,
            batch_deadline_ms: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Region {
    pub id: RegionId,
    pub latitude: f64,
    pub longitude: f64,
    pub radius_m: f64,
}

#[derive(Debug, Clone)]
pub struct BleScanConfig {
    pub interval_ms: u32,
    pub duty_s: u32,
}

/// A REST request composed entirely within the core (FR-CORE-003). The runtime
/// executes it and returns `Input::HttpResponse`.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Put,
    Post,
    Delete,
}

#[derive(Debug, Clone)]
pub enum HttpResult {
    Ok { status: u16, body: Vec<u8> },
    Transport(String),
    TimedOut,
}

/// Inputs the platform/runtime feeds the core.
#[derive(Debug)]
pub enum Input {
    Command(Command),
    LocationFix(PlatformFix),
    Activity(PlatformActivity),
    RegionExited(RegionId),
    BleSighting(BleAdvertisement),
    TransportOpened,
    FrameReceived(Vec<u8>),
    TransportClosed(u16),
    HttpResponse(RequestId, HttpResult),
    Connectivity(NetState),
    AppVisibility(bool),
    Timer,
}

/// Outputs the core emits for the runtime to execute.
#[derive(Debug, Clone)]
pub enum Output {
    OpenTransport {
        url: String,
        subprotocols: Vec<String>,
    },
    SendFrame(Vec<u8>),
    CloseTransport(u16),
    Http(RequestId, HttpRequest),
    ConfigureLocation(LocationRequest),
    MonitorRegion(Region),
    StopMonitoringRegion(RegionId),
    ConfigureBleScan(Option<BleScanConfig>),
    Persist {
        key: String,
        value: Vec<u8>,
        secure: bool,
    },
    Emit(ClientEvent),
    ScheduleTimer(u64),
}

/// Something the core wants the application (via the SDK facade) to know about.
#[derive(Debug, Clone)]
pub enum ClientEvent {
    /// The connection state machine moved.
    ConnectionChanged(ConnectionState),
    /// A location was published.
    Published {
        published_seq: u64,
    },
    /// A fix was dropped locally before sending.
    FixDropped(DropReason),
    /// A close code with the reconnect policy attached (so the UI can show it).
    Reconnecting {
        state: ConnectionState,
        code: CloseCode,
    },
    /// The sampler changed its motion class or intent.
    SamplingChanged,
    /// Auth is about to expire: the runtime must renew the token and call
    /// `AuthRefresh` via an `Input`.
    AuthExpiring {
        at_ms: u64,
    },
    Heartbeat {
        client_time_ms: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    LowAccuracy,
    ExcessiveSpeed,
    InvalidCoordinate,
}

/// A WebSocket close code from `08` §8, plus `Unknown` and `LocalInitiated` for
/// values produced by this process itself (e.g. when we close the transport).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseCode {
    Normal,
    GoingAway,
    MessageTooBig,
    ProtocolError,
    UnsupportedProtocol,
    HelloTimeout,
    SlowConsumer,
    AuthExpired,
    Forbidden,
    IdleTimeout,
    Superseded,
    RateLimited,
    Internal,
    TryAgain,
    /// Something this core process itself produced, e.g. a local idle trigger.
    Local,
    /// Not one of the documented codes: we never treat it as a known case.
    Unknown(u16),
}

impl CloseCode {
    /// Translate the wire value (`Input::TransportClosed(u16)`) into a typed code.
    #[must_use]
    pub fn from_wire(code: u16) -> Self {
        match code {
            1000 => Self::Normal,
            1001 => Self::GoingAway,
            1009 => Self::MessageTooBig,
            4000 => Self::ProtocolError,
            4001 => Self::UnsupportedProtocol,
            4002 => Self::HelloTimeout,
            4005 => Self::SlowConsumer,
            4401 => Self::AuthExpired,
            4403 => Self::Forbidden,
            4408 => Self::IdleTimeout,
            4409 => Self::Superseded,
            4429 => Self::RateLimited,
            4500 => Self::Internal,
            4503 => Self::TryAgain,
            _ => Self::Unknown(code),
        }
    }

    #[must_use]
    pub fn to_wire(self) -> u16 {
        match self {
            Self::Normal => 1000,
            Self::GoingAway => 1001,
            Self::MessageTooBig => 1009,
            Self::ProtocolError => 4000,
            Self::UnsupportedProtocol => 4001,
            Self::HelloTimeout => 4002,
            Self::SlowConsumer => 4005,
            Self::AuthExpired => 4401,
            Self::Forbidden => 4403,
            Self::IdleTimeout => 4408,
            Self::Superseded => 4409,
            Self::RateLimited => 4429,
            Self::Internal => 4500,
            Self::TryAgain => 4503,
            // Honest corner case: the whole point of `Local` is that the whole
            // 1000 `NORMAL` close is a deliberate local decision, never an
            // artefact of processing.
            Self::Local => 1000,
            Self::Unknown(c) => c,
        }
    }

    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::UnsupportedProtocol | Self::Forbidden)
    }
}

/// The outcome of applying the `08` §8 close-code table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconnectAction {
    /// Immediate reconnect; no delay, no backoff reset needed.
    Immediate,
    /// Wait at least this many milliseconds.
    After(u64),
    /// Wait at least 60 s (`RATE_LIMITED`).
    RateLimited,
    /// Use the ordinary full-jitter backoff.
    Backoff,
    /// Authexpired: obtain a new token + ticket first, then reconnect.
    RefreshToken,
    /// No reconnecting at all (`UnsupportedProtocol`, `Forbidden`, `Superseded`).
    Stop,
}

impl ReconnectAction {
    /// A `bool` view: did the policy say to retry at all?
    #[must_use]
    pub const fn retries(self) -> bool {
        !matches!(self, Self::Stop)
    }
}

/// Apply the documented table to a close code, deciding what the runtime does
/// next. Pulled into the core so the table can be unit-tested without a server.
#[must_use]
/// Phase of the connection state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
}

pub fn close_action(code: CloseCode, notice_reconnect_after_ms: Option<u64>) -> ReconnectAction {
    match code {
        CloseCode::IdleTimeout => ReconnectAction::Immediate,
        CloseCode::AuthExpired => ReconnectAction::RefreshToken,
        CloseCode::UnsupportedProtocol => ReconnectAction::Stop,
        CloseCode::Forbidden => ReconnectAction::Stop,
        CloseCode::Superseded => ReconnectAction::Stop,
        CloseCode::RateLimited => ReconnectAction::RateLimited,
        CloseCode::TryAgain => ReconnectAction::After(notice_reconnect_after_ms.unwrap_or(30_000)),
        _ => ReconnectAction::Backoff,
    }
}

pub fn close_action_default(code: CloseCode) -> ReconnectAction {
    close_action(code, None)
}

#[derive(Debug, Clone)]
pub struct ReconnectPolicy {
    pub base_ms: u64,
    pub max_delay_ms: u64,
    pub attempt: u32,
    /// 60 s of stable connection resets the counter.
    pub stable_for_reset_ms: u64,
}

impl ReconnectPolicy {
    pub const BASE_MS: u64 = 500;
    pub const MAX_DELAY_MS: u64 = 30_000;
    pub const STABLE_RESET_MS: u64 = 60_000;

    #[must_use]
    pub fn new() -> Self {
        Self {
            base_ms: Self::BASE_MS,
            max_delay_ms: Self::MAX_DELAY_MS,
            attempt: 0,
            stable_for_reset_ms: Self::STABLE_RESET_MS,
        }
    }

    #[must_use]
    pub fn delay_ms(&self, rng_unit: f64) -> u64 {
        // Full jitter: random(0, min(max, base·2^attempt)).
        let mut ceiling = self
            .base_ms
            .saturating_mul(1u64.checked_shl(self.attempt).unwrap_or(u64::MAX));
        if ceiling > self.max_delay_ms {
            ceiling = self.max_delay_ms;
        }
        let unit = rng_unit.clamp(0.0, 1.0 - f64::EPSILON);
        (unit * (ceiling as f64)) as u64
    }

    pub fn note_connected(&mut self, _now_ms: u64) {
        self.attempt = 0;
    }

    pub fn note_failed(&mut self) {
        self.attempt = self.attempt.saturating_add(1);
    }
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self::new()
    }
}

/// A command produced by the platform/UI and consumed by the core.
#[derive(Debug, Clone)]
pub enum Command {
    Start,
    Stop,
    Watch { session_id: String },
    Unwatch { session_id: String },
    SetIntent(Intent),
    ForceReconnect,
}

/// What a `PlatformFix` update led the sampler to decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishDecision {
    Publish,
    Skip,
}

/// A splitmix generator for deterministic backoff jitter. The core carries one
/// so `delay_ms` is reproducible; the runtime overrides the strategy with
/// `rand` or the platform CSPRNG if it wants non-determinism.
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    #[must_use]
    pub fn next_unit(&mut self) -> f64 {
        self.state = self.state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111b9);
        z ^= z >> 31;
        (z as f64) / (u64::MAX as f64)
    }
}
