//! Stream configuration: timers, limits and thresholds (docs/17 §2.6 and the tenant policy
//! of docs/17 §4, FR-STR-001, FR-STR-005).
//!
//! Defaults mirror `17` §2.6; the effective per-connection limits that the server reports in
//! `ServerHello` also take the tenant policy into account (docs/08 §6.1).
#![forbid(unsafe_code)]

use std::time::Duration;

use gs_service::gs_auth::TenantPolicy;

/// Default of `GS_STREAM_MAX_CONNECTIONS` (docs/17 §2.6).
pub const DEFAULT_MAX_CONNECTIONS: u32 = 20_000;
/// Default of `GS_STREAM_MAX_FRAME_BYTES` (docs/17 §2.6).
pub const DEFAULT_MAX_FRAME_BYTES: usize = 16_384;
/// Default of `GS_STREAM_HELLO_TIMEOUT_MS` (docs/17 §2.6).
pub const DEFAULT_HELLO_TIMEOUT_MS: u64 = 5_000;
/// Default of `GS_STREAM_HEARTBEAT_INTERVAL_MS` (docs/17 §2.6).
pub const DEFAULT_HEARTBEAT_INTERVAL_MS: u64 = 20_000;
/// Default of `GS_STREAM_IDLE_TIMEOUT_MS` (docs/17 §2.6).
pub const DEFAULT_IDLE_TIMEOUT_MS: u64 = 60_000;
/// Default of `GS_STREAM_MAX_SUBSCRIPTIONS` (docs/17 §2.6).
pub const DEFAULT_MAX_SUBSCRIPTIONS: u32 = 20;
/// Default of `GS_STREAM_MAX_SUBSCRIPTIONS_APP` (docs/17 §2.6).
pub const DEFAULT_MAX_SUBSCRIPTIONS_APP: u32 = 1_000;
/// Default of `GS_STREAM_MAX_PROTOCOL_ERRORS` (docs/17 §2.6).
pub const DEFAULT_MAX_PROTOCOL_ERRORS: u32 = 10;
/// Default of `GS_STREAM_MAX_INBOUND_MSGS_PER_S` (docs/17 §2.6).
pub const DEFAULT_MAX_INBOUND_MSGS_PER_S: u32 = 20;
/// Default of `GS_STREAM_ABUSE_WINDOW_S` (docs/17 §2.6).
pub const DEFAULT_ABUSE_WINDOW_S: u64 = 10;
/// Default of `GS_STREAM_UPGRADE_RPM_PER_IP` (docs/17 §2.6).
pub const DEFAULT_UPGRADE_RPM_PER_IP: u32 = 120;
/// Default of `GS_LOCATION_MAX_EVENT_AGE_MS` (docs/17 §2.7).
pub const DEFAULT_MAX_EVENT_AGE_MS: u64 = 120_000;
/// Default of `GS_AUTHZ_CACHE_TTL_MS` (docs/17 §2.2, ADR-0009).
pub const DEFAULT_AUTHZ_CACHE_TTL_MS: u64 = 5_000;
/// Default of `min_publish_interval_ms` (docs/17 §4).
pub const DEFAULT_MIN_PUBLISH_INTERVAL_MS: u32 = 1_000;

/// Server side stream parameters (docs/17 §2.6).
///
/// Only what a connection needs to open, stay alive and close is here; the parameters of
/// publishing, presence and drainage arrive with their own tasks (T-F4-04..T-F4-08).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamConfig {
    /// Whether `gs.v1.json` is offered (`GS_STREAM_JSON_ENABLED`).
    pub json_subprotocol_enabled: bool,
    /// Connections per instance (`GS_STREAM_MAX_CONNECTIONS`).
    pub max_connections: u32,
    /// Largest inbound frame (`GS_STREAM_MAX_FRAME_BYTES`).
    pub max_frame_bytes: usize,
    /// Time to send `ClientHello` (`GS_STREAM_HELLO_TIMEOUT_MS`).
    pub hello_timeout: Duration,
    /// Heartbeat interval reported in `limits` (`GS_STREAM_HEARTBEAT_INTERVAL_MS`).
    pub heartbeat_interval: Duration,
    /// Inactivity limit (`GS_STREAM_IDLE_TIMEOUT_MS`).
    pub idle_timeout: Duration,
    /// Subscriptions of a device connection (`GS_STREAM_MAX_SUBSCRIPTIONS`).
    pub max_subscriptions: u32,
    /// Subscriptions of an application connection (`GS_STREAM_MAX_SUBSCRIPTIONS_APP`).
    pub max_subscriptions_app: u32,
    /// Protocol errors tolerated before closing (`GS_STREAM_MAX_PROTOCOL_ERRORS`).
    pub max_protocol_errors: u32,
    /// Inbound messages per second per connection (`GS_STREAM_MAX_INBOUND_MSGS_PER_S`).
    pub max_inbound_msgs_per_s: u32,
    /// Sustained excess that triggers 4429 (`GS_STREAM_ABUSE_WINDOW_S`).
    pub abuse_window: Duration,
    /// Maximum age of an accepted `captured_at` (`GS_LOCATION_MAX_EVENT_AGE_MS`).
    pub max_event_age: Duration,
    /// Maximum age of a cached authorization decision (`GS_AUTHZ_CACHE_TTL_MS`, ADR-0009).
    pub authz_cache_ttl: Duration,
}

impl Default for StreamConfig {
    /// Defaults of docs/17 §2.6 and §2.7.
    fn default() -> Self {
        Self {
            json_subprotocol_enabled: true,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            max_frame_bytes: DEFAULT_MAX_FRAME_BYTES,
            hello_timeout: Duration::from_millis(DEFAULT_HELLO_TIMEOUT_MS),
            heartbeat_interval: Duration::from_millis(DEFAULT_HEARTBEAT_INTERVAL_MS),
            idle_timeout: Duration::from_millis(DEFAULT_IDLE_TIMEOUT_MS),
            max_subscriptions: DEFAULT_MAX_SUBSCRIPTIONS,
            max_subscriptions_app: DEFAULT_MAX_SUBSCRIPTIONS_APP,
            max_protocol_errors: DEFAULT_MAX_PROTOCOL_ERRORS,
            max_inbound_msgs_per_s: DEFAULT_MAX_INBOUND_MSGS_PER_S,
            abuse_window: Duration::from_secs(DEFAULT_ABUSE_WINDOW_S),
            max_event_age: Duration::from_millis(DEFAULT_MAX_EVENT_AGE_MS),
            authz_cache_ttl: Duration::from_millis(DEFAULT_AUTHZ_CACHE_TTL_MS),
        }
    }
}

/// Violation of a documented range found in [`StreamConfig`] (docs/17 §2.6).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StreamConfigError {
    /// `GS_STREAM_MAX_FRAME_BYTES` outside 1024..=1048576.
    #[error("max_frame_bytes outside the documented range")]
    MaxFrameBytes,
    /// `GS_STREAM_IDLE_TIMEOUT_MS` below twice the heartbeat interval.
    #[error("idle_timeout must be at least twice the heartbeat interval")]
    IdleTimeout,
    /// `GS_STREAM_MAX_CONNECTIONS` is zero.
    #[error("max_connections must be at least one")]
    MaxConnections,
    /// `GS_STREAM_MAX_PROTOCOL_ERRORS` is zero.
    #[error("max_protocol_errors must be at least one")]
    MaxProtocolErrors,
    /// `GS_STREAM_MAX_INBOUND_MSGS_PER_S` is zero.
    #[error("max_inbound_msgs_per_s must be at least one")]
    MaxInboundMsgs,
}

impl StreamConfig {
    /// Validates the cross-field rules of docs/17 §2.6 (`idle_timeout` ≥ 2 × heartbeat).
    pub fn validate(&self) -> Result<(), StreamConfigError> {
        if self.max_connections == 0 {
            return Err(StreamConfigError::MaxConnections);
        }
        if self.max_protocol_errors == 0 {
            return Err(StreamConfigError::MaxProtocolErrors);
        }
        if self.max_inbound_msgs_per_s == 0 {
            return Err(StreamConfigError::MaxInboundMsgs);
        }
        if !(1024..=1_048_576).contains(&self.max_frame_bytes) {
            return Err(StreamConfigError::MaxFrameBytes);
        }
        if self.idle_timeout < self.heartbeat_interval.saturating_mul(2) {
            return Err(StreamConfigError::IdleTimeout);
        }
        Ok(())
    }
}

/// Tenant policy values that shape the limits reported to a connection (docs/17 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TenantStreamLimits {
    /// `min_publish_interval_ms`.
    pub min_publish_interval_ms: u64,
    /// `max_samples_per_publish`.
    pub max_samples_per_publish: u32,
}

impl From<&TenantPolicy> for TenantStreamLimits {
    fn from(policy: &TenantPolicy) -> Self {
        Self {
            min_publish_interval_ms: policy.min_publish_interval_ms,
            max_samples_per_publish: policy.max_samples_per_publish,
        }
    }
}

/// Effective limits of one connection, as reported in `ServerHello.limits` (docs/08 §6.1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StreamLimits {
    /// `GS_STREAM_MAX_FRAME_BYTES`.
    pub max_frame_bytes: u32,
    /// `GS_STREAM_MAX_SUBSCRIPTIONS` or `GS_STREAM_MAX_SUBSCRIPTIONS_APP`.
    pub max_subscriptions: u32,
    /// `min_publish_interval_ms` of the tenant policy.
    pub min_publish_interval_ms: u32,
    /// `max_samples_per_publish` of the tenant policy.
    pub max_samples_per_publish: u32,
    /// `GS_STREAM_HEARTBEAT_INTERVAL_MS`.
    pub heartbeat_interval_ms: u32,
    /// `GS_STREAM_IDLE_TIMEOUT_MS`.
    pub idle_timeout_ms: u32,
    /// `GS_LOCATION_MAX_EVENT_AGE_MS`.
    pub max_event_age_ms: u32,
}

impl StreamConfig {
    /// Builds the effective limits of a connection (docs/08 §6.1).
    pub fn effective_limits(&self, tenant: &TenantStreamLimits, application: bool) -> StreamLimits {
        // `min_publish_interval_ms` is a `uint32` on the wire and 200..=60000 in the policy
        // (docs/17 §4), so the narrowing cannot lose a value inside the documented range.
        let min_publish_interval_ms = u32::try_from(tenant.min_publish_interval_ms)
            .unwrap_or(crate::config::DEFAULT_MIN_PUBLISH_INTERVAL_MS);
        StreamLimits {
            max_frame_bytes: self.max_frame_bytes as u32,
            max_subscriptions: if application {
                self.max_subscriptions_app
            } else {
                self.max_subscriptions
            },
            min_publish_interval_ms,
            max_samples_per_publish: tenant.max_samples_per_publish,
            heartbeat_interval_ms: self.heartbeat_interval.as_millis() as u32,
            idle_timeout_ms: self.idle_timeout.as_millis() as u32,
            max_event_age_ms: self.max_event_age.as_millis() as u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults_match_docs_17_section_2_6() {
        // covers: docs/17 §2.6, docs/17 §2.7
        let config = StreamConfig::default();
        config.validate().expect("defaults must be valid");
        assert_eq!(config.max_connections, 20_000);
        assert_eq!(config.max_frame_bytes, 16_384);
        assert_eq!(config.hello_timeout, Duration::from_millis(5_000));
        assert_eq!(config.heartbeat_interval, Duration::from_millis(20_000));
        assert_eq!(config.idle_timeout, Duration::from_millis(60_000));
        assert_eq!(config.max_subscriptions, 20);
        assert_eq!(config.max_subscriptions_app, 1_000);
        assert_eq!(config.max_protocol_errors, 10);
        assert_eq!(config.max_event_age, Duration::from_millis(120_000));
        assert_eq!(config.authz_cache_ttl, Duration::from_millis(5_000));
    }

    #[test]
    fn test_idle_timeout_must_be_twice_the_heartbeat() {
        // covers: docs/17 §2.6
        let mut config = StreamConfig {
            idle_timeout: Duration::from_millis(30_000),
            ..StreamConfig::default()
        };
        assert_eq!(
            config.validate(),
            Err(StreamConfigError::IdleTimeout),
            "idle timeout below 2 x heartbeat must be rejected"
        );
        config.idle_timeout = Duration::from_millis(40_000);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_frame_size_range_is_enforced() {
        // covers: docs/17 §2.6
        let config = StreamConfig {
            max_frame_bytes: 512,
            ..StreamConfig::default()
        };
        assert_eq!(config.validate(), Err(StreamConfigError::MaxFrameBytes));
        let config = StreamConfig {
            max_frame_bytes: 2_097_152,
            ..StreamConfig::default()
        };
        assert_eq!(config.validate(), Err(StreamConfigError::MaxFrameBytes));
    }

    #[test]
    fn test_effective_limits_distinguish_principal_kinds() {
        // covers: docs/08 §6.1
        let config = StreamConfig::default();
        let tenant = TenantStreamLimits {
            min_publish_interval_ms: 1_000,
            max_samples_per_publish: 5,
        };
        let device = config.effective_limits(&tenant, false);
        let application = config.effective_limits(&tenant, true);
        assert_eq!(device.max_subscriptions, 20);
        assert_eq!(application.max_subscriptions, 1_000);
        assert_eq!(device.max_frame_bytes, 16_384);
        assert_eq!(device.min_publish_interval_ms, 1_000);
        assert_eq!(device.max_samples_per_publish, 5);
        assert_eq!(device.heartbeat_interval_ms, 20_000);
        assert_eq!(device.idle_timeout_ms, 60_000);
        assert_eq!(device.max_event_age_ms, 120_000);
    }
}
