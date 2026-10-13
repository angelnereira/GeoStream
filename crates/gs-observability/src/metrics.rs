//! Metric and label catalog with the restrictions of docs/16_OPERATIONS.md §4.2
//! (NFR-OBS-001, NFR-OBS-002).
//!
//! Every metric name and every permitted label value lives here, so that a call site cannot
//! invent a label that would leak a session, member, device, subject or tenant id.
#![forbid(unsafe_code)]

use std::fmt;

/// Build metric (always 1).
pub const BUILD_INFO: &str = "gs_build_info";
/// REST request counter.
pub const HTTP_REQUESTS_TOTAL: &str = "gs_http_requests_total";
/// REST request duration histogram.
pub const HTTP_REQUEST_DURATION_SECONDS: &str = "gs_http_request_duration_seconds";
/// Authentication failure counter.
pub const AUTH_FAILURES_TOTAL: &str = "gs_auth_failures_total";
/// Rate limited request counter.
pub const RATE_LIMITED_TOTAL: &str = "gs_rate_limited_total";
/// Open stream connections.
pub const STREAM_CONNECTIONS: &str = "gs_stream_connections";
/// Stream connections opened since the start.
pub const STREAM_CONNECTIONS_OPENED_TOTAL: &str = "gs_stream_connections_opened_total";
/// Stream connections closed since the start.
pub const STREAM_CONNECTIONS_CLOSED_TOTAL: &str = "gs_stream_connections_closed_total";
/// Rejected stream upgrades.
pub const STREAM_UPGRADE_REJECTED_TOTAL: &str = "gs_stream_upgrade_rejected_total";
/// Open stream subscriptions.
pub const STREAM_SUBSCRIPTIONS: &str = "gs_stream_subscriptions";
/// Stream messages received.
pub const STREAM_MESSAGES_IN_TOTAL: &str = "gs_stream_messages_in_total";
/// Stream messages sent.
pub const STREAM_MESSAGES_OUT_TOTAL: &str = "gs_stream_messages_out_total";
/// Location samples by acceptance result.
pub const LOCATION_SAMPLES_TOTAL: &str = "gs_location_samples_total";
/// Rejected location samples by field.
pub const LOCATION_REJECTED_TOTAL: &str = "gs_location_rejected_total";
/// Stream delivery latency.
pub const STREAM_DELIVERY_LATENCY_SECONDS: &str = "gs_stream_delivery_latency_seconds";
/// Coalesced location updates.
pub const STREAM_COALESCED_TOTAL: &str = "gs_stream_coalesced_total";
/// Applied sequence floors.
pub const STREAM_SEQUENCE_FLOOR_APPLIED_TOTAL: &str = "gs_stream_sequence_floor_applied_total";
/// Fan-out deliveries of `LocationUpdate`.
pub const FANOUT_DELIVERIES_TOTAL: &str = "gs_fanout_deliveries_total";
/// Revocation propagation delay.
pub const REVOCATION_PROPAGATION_SECONDS: &str = "gs_revocation_propagation_seconds";
/// Authorization cache events.
pub const AUTHZ_CACHE_EVENTS_TOTAL: &str = "gs_authz_cache_events_total";
/// Bus connection state.
pub const BUS_CONNECTED: &str = "gs_bus_connected";
/// Bus messages by direction and topic kind.
pub const BUS_MESSAGES_TOTAL: &str = "gs_bus_messages_total";
/// Bus reconnections.
pub const BUS_RECONNECTS_TOTAL: &str = "gs_bus_reconnects_total";
/// Ephemeral store command duration.
pub const EPHEMERAL_COMMAND_DURATION_SECONDS: &str = "gs_ephemeral_command_duration_seconds";
/// Ephemeral store errors.
pub const EPHEMERAL_ERRORS_TOTAL: &str = "gs_ephemeral_errors_total";
/// Failed post-commit effects.
pub const EPHEMERAL_EFFECT_FAILURES_TOTAL: &str = "gs_ephemeral_effect_failures_total";
/// Database query duration.
pub const DB_QUERY_DURATION_SECONDS: &str = "gs_db_query_duration_seconds";
/// Database pool connections by state.
pub const DB_POOL_CONNECTIONS: &str = "gs_db_pool_connections";
/// Active sessions (worker).
pub const SESSIONS_ACTIVE: &str = "gs_sessions_active";
/// Worker job duration.
pub const WORKER_JOB_DURATION_SECONDS: &str = "gs_worker_job_duration_seconds";
/// Worker job processed rows.
pub const WORKER_JOB_ROWS_TOTAL: &str = "gs_worker_job_rows_total";
/// Webhook delivery results.
pub const WEBHOOK_DELIVERIES_TOTAL: &str = "gs_webhook_deliveries_total";
/// Webhook backlog.
pub const WEBHOOK_BACKLOG: &str = "gs_webhook_backlog";
/// Geofence evaluations.
pub const GEOFENCE_EVALUATIONS_TOTAL: &str = "gs_geofence_evaluations_total";
/// Dropped geofence evaluations.
pub const GEOFENCE_EVALUATIONS_DROPPED_TOTAL: &str = "gs_geofence_evaluations_dropped_total";
/// Geofence transitions.
pub const GEOFENCE_TRANSITIONS_TOTAL: &str = "gs_geofence_transitions_total";

/// Kind of principal: only these values may appear in labels and logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrincipalKind {
    /// Application API key.
    Application,
    /// Device token.
    Device,
    /// Operator of the CLI.
    Operator,
}

impl PrincipalKind {
    /// Canonical label value.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Application => "application",
            Self::Device => "device",
            Self::Operator => "operator",
        }
    }
}

impl fmt::Display for PrincipalKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// HTTP status class allowed in labels: `2xx`, `3xx`, `4xx`, `5xx`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusClass {
    /// 2xx.
    Success,
    /// 3xx.
    Redirect,
    /// 4xx.
    ClientError,
    /// 5xx.
    ServerError,
}

impl StatusClass {
    /// Classifies an HTTP status code.
    pub const fn of(status: u16) -> Self {
        match status {
            200..=399 => Self::Success,
            400..=499 => Self::ClientError,
            _ => Self::ServerError,
        }
    }

    /// Canonical label value.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "2xx",
            Self::Redirect | Self::ClientError => "4xx",
            Self::ServerError => "5xx",
        }
    }
}

impl fmt::Display for StatusClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Rate limit scope (docs/16 §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitScope {
    /// REST with device token.
    RestDevice,
    /// REST with application key.
    RestApplication,
    /// Invitation redemption.
    InviteRedeem,
    /// Per IP authentication.
    AuthIp,
    /// Stream publishing.
    Publish,
    /// Inbound stream messages.
    StreamInbound,
    /// Per IP stream upgrade.
    UpgradeIp,
}

impl RateLimitScope {
    /// Canonical label value.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::RestDevice => "rest_device",
            Self::RestApplication => "rest_app",
            Self::InviteRedeem => "invite_redeem",
            Self::AuthIp => "auth_ip",
            Self::Publish => "publish",
            Self::StreamInbound => "stream_inbound",
            Self::UpgradeIp => "upgrade_ip",
        }
    }
}

impl fmt::Display for RateLimitScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Result of a location sample (docs/16 §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleResult {
    /// Accepted.
    Accepted,
    /// Duplicate sequence.
    IgnoredDuplicate,
    /// Obsolete sequence.
    IgnoredStale,
    /// Validation failed.
    Rejected,
}

impl SampleResult {
    /// Canonical label value.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::IgnoredDuplicate => "ignored_duplicate",
            Self::IgnoredStale => "ignored_stale",
            Self::Rejected => "rejected",
        }
    }
}

impl fmt::Display for SampleResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Topic kind of the bus (docs/16 §4.2, docs/04 §6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopicKind {
    /// Location updates.
    Loc,
    /// Control events.
    Ctl,
    /// Device events.
    Dev,
    /// Global invalidation.
    Global,
}

impl TopicKind {
    /// Canonical label value.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Loc => "loc",
            Self::Ctl => "ctl",
            Self::Dev => "dev",
            Self::Global => "global",
        }
    }
}

impl fmt::Display for TopicKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Authorization cache event (docs/16 §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthzCacheEvent {
    /// Entry found and valid.
    Hit,
    /// Entry absent.
    Miss,
    /// Entry revalidated by TTL.
    Revalidate,
    /// Entry reloaded from PostgreSQL.
    Reload,
    /// Cache flushed.
    Flush,
}

impl AuthzCacheEvent {
    /// Canonical label value.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Revalidate => "revalidate",
            Self::Reload => "reload",
            Self::Flush => "flush",
        }
    }
}

impl fmt::Display for AuthzCacheEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Metric labels of one call site: only catalogued names and values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Labels {
    entries: Vec<(&'static str, String)>,
}

impl Labels {
    /// Empty label set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a label. The name must belong to the catalog of docs/16 §4.2.
    pub fn with(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.entries.push((name, value.into()));
        self
    }

    /// Adds a `principal_kind` label.
    pub fn principal(self, kind: PrincipalKind) -> Self {
        self.with("principal_kind", kind.as_str())
    }

    /// Adds a `route` label with the path template, never the concrete id.
    pub fn route(self, template: impl Into<String>) -> Self {
        self.with("route", template)
    }

    /// Adds a `method` label.
    pub fn method(self, method: impl Into<String>) -> Self {
        self.with("method", method)
    }

    /// Adds a `status_class` label.
    pub fn status_class(self, status: u16) -> Self {
        self.with("status_class", StatusClass::of(status).as_str())
    }

    /// Adds a `result` label.
    pub fn result(self, result: SampleResult) -> Self {
        self.with("result", result.as_str())
    }

    /// Adds a `scope` label.
    pub fn scope(self, scope: RateLimitScope) -> Self {
        self.with("scope", scope.as_str())
    }

    /// Adds a `code` label.
    pub fn code(self, code: impl Into<String>) -> Self {
        self.with("code", code)
    }

    /// Adds a `reason` label.
    pub fn reason(self, reason: impl Into<String>) -> Self {
        self.with("reason", reason)
    }

    /// Adds a `type` label.
    pub fn message_type(self, message_type: impl Into<String>) -> Self {
        self.with("type", message_type)
    }

    /// Adds a `topic_kind` label.
    pub fn topic_kind(self, kind: TopicKind) -> Self {
        self.with("topic_kind", kind.as_str())
    }

    /// Adds a `direction` label.
    pub fn direction(self, direction: impl Into<String>) -> Self {
        self.with("direction", direction)
    }

    /// Adds an `event` label.
    pub fn event(self, event: AuthzCacheEvent) -> Self {
        self.with("event", event.as_str())
    }

    /// Adds an `op` label.
    pub fn op(self, op: impl Into<String>) -> Self {
        self.with("op", op)
    }

    /// Adds a `field` label with a bounded path of docs/09 §3.1.
    pub fn field(self, field: impl Into<String>) -> Self {
        self.with("field", field)
    }

    /// Adds an `effect` label.
    pub fn effect(self, effect: impl Into<String>) -> Self {
        self.with("effect", effect)
    }

    /// Adds a `job` label.
    pub fn job(self, job: impl Into<String>) -> Self {
        self.with("job", job)
    }

    /// Adds a `state` label.
    pub fn state(self, state: impl Into<String>) -> Self {
        self.with("state", state)
    }

    /// Adds a `query` label with the logical name of a bounded set.
    pub fn query(self, query: impl Into<String>) -> Self {
        self.with("query", query)
    }

    /// Adds a `transition` label.
    pub fn transition(self, transition: impl Into<String>) -> Self {
        self.with("transition", transition)
    }

    /// Adds a `kind` label.
    pub fn kind(self, kind: impl Into<String>) -> Self {
        self.with("kind", kind)
    }

    /// Returns the labels as `(name, value)` pairs.
    pub fn entries(&self) -> &[(&'static str, String)] {
        &self.entries
    }

    /// Converts the label set into the `metrics` crate representation.
    pub fn to_metric_labels(&self) -> Vec<metrics::Label> {
        self.entries
            .iter()
            .map(|(name, value)| metrics::Label::new(*name, value.clone()))
            .collect()
    }
}

/// Label names permitted by docs/16 §4.2.
pub const ALLOWED_LABEL_NAMES: &[&str] = &[
    "version",
    "commit",
    "route",
    "method",
    "status_class",
    "principal_kind",
    "reason",
    "scope",
    "code",
    "type",
    "result",
    "field",
    "kind",
    "direction",
    "topic_kind",
    "event",
    "op",
    "effect",
    "job",
    "state",
    "query",
    "transition",
];

/// Rejects any label name outside the catalog of docs/16 §4.2 (NFR-OBS-002).
pub fn is_allowed_label(name: &str) -> bool {
    ALLOWED_LABEL_NAMES.contains(&name)
}

/// Increments a counter with the given labels.
pub fn increment_counter(name: &'static str, labels: &Labels) {
    let metric_labels = labels.to_metric_labels();
    if metric_labels.is_empty() {
        metrics::counter!(name).increment(1);
    } else {
        metrics::counter!(name, metric_labels).increment(1);
    }
}

/// Sets a gauge.
pub fn set_gauge(name: &'static str, value: f64, labels: &Labels) {
    let metric_labels = labels.to_metric_labels();
    if metric_labels.is_empty() {
        metrics::gauge!(name).set(value);
    } else {
        metrics::gauge!(name, metric_labels).set(value);
    }
}

/// Observes a duration in seconds.
pub fn observe_duration(name: &'static str, seconds: f64, labels: &Labels) {
    let metric_labels = labels.to_metric_labels();
    if metric_labels.is_empty() {
        metrics::histogram!(name).record(seconds);
    } else {
        metrics::histogram!(name, metric_labels).record(seconds);
    }
}

/// Registers the build information gauge.
pub fn record_build_info(version: &str, commit: &str) {
    let labels = Labels::new()
        .with("version", version.to_string())
        .with("commit", commit.to_string());
    set_gauge(BUILD_INFO, 1.0, &labels);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_label_catalog_rejects_unknown_names() {
        // covers: NFR-OBS-002, docs/16 §4.2
        for allowed in ALLOWED_LABEL_NAMES {
            assert!(is_allowed_label(allowed), "{allowed} must be permitted");
        }
        for forbidden in [
            "session_id",
            "member_id",
            "device_id",
            "subject_id",
            "tenant_id",
            "latitude",
        ] {
            assert!(
                !is_allowed_label(forbidden),
                "{forbidden} must never be a metric label"
            );
        }
    }

    #[test]
    fn test_principal_kind_values_are_bounded() {
        // covers: NFR-OBS-002, docs/16 §4.2
        let values = [
            PrincipalKind::Application,
            PrincipalKind::Device,
            PrincipalKind::Operator,
        ]
        .iter()
        .map(|kind| kind.as_str())
        .collect::<Vec<_>>();
        assert_eq!(values, ["application", "device", "operator"]);
    }

    #[test]
    fn test_status_class_groups_results() {
        // covers: NFR-OBS-002, docs/16 §4.2
        assert_eq!(StatusClass::of(200).as_str(), "2xx");
        assert_eq!(StatusClass::of(302).as_str(), "2xx");
        assert_eq!(StatusClass::of(404).as_str(), "4xx");
        assert_eq!(StatusClass::of(503).as_str(), "5xx");
    }

    #[test]
    fn test_rate_limit_and_topic_values_are_bounded() {
        // covers: NFR-OBS-002, docs/16 §4.2
        let scopes = [
            RateLimitScope::RestDevice,
            RateLimitScope::RestApplication,
            RateLimitScope::InviteRedeem,
            RateLimitScope::AuthIp,
            RateLimitScope::Publish,
            RateLimitScope::StreamInbound,
            RateLimitScope::UpgradeIp,
        ]
        .iter()
        .map(|scope| scope.as_str())
        .collect::<Vec<_>>();
        assert_eq!(
            scopes,
            [
                "rest_device",
                "rest_app",
                "invite_redeem",
                "auth_ip",
                "publish",
                "stream_inbound",
                "upgrade_ip"
            ]
        );

        assert_eq!(TopicKind::Global.as_str(), "global");
        assert_eq!(SampleResult::IgnoredStale.as_str(), "ignored_stale");
        assert_eq!(AuthzCacheEvent::Reload.as_str(), "reload");
    }

    #[test]
    fn test_metric_helpers_record_without_a_global_recorder() {
        // covers: NFR-OBS-001
        // Without a recorder installed the calls are no-ops, which keeps call sites simple.
        let labels = Labels::new()
            .principal(PrincipalKind::Device)
            .result(SampleResult::Accepted);
        increment_counter(LOCATION_SAMPLES_TOTAL, &labels);
        set_gauge(
            STREAM_CONNECTIONS,
            3.0,
            &Labels::new().principal(PrincipalKind::Application),
        );
        observe_duration(STREAM_DELIVERY_LATENCY_SECONDS, 0.012, &Labels::new());
        record_build_info("0.0.0", "unknown");
    }
}
