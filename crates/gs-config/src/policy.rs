//! Tenant policy model and validations (docs/17 §4, FR-ADM-002).
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

use crate::error::PolicyError;

pub const POLICY_KEYS: &[&str] = &[
    "default_session_ttl_seconds",
    "max_session_ttl_seconds",
    "default_max_members_one_to_many",
    "default_max_members_many_to_many",
    "max_members_cap",
    "snapshot_ttl_default_seconds",
    "snapshot_ttl_max_seconds",
    "stale_after_seconds_default",
    "min_publish_interval_ms",
    "publish_burst",
    "max_samples_per_publish",
    "max_samples_per_publish_http",
    "max_horizontal_accuracy_m",
    "reject_null_island",
    "max_active_sessions_per_subject",
    "max_devices_per_subject",
    "invitation_max_ttl_seconds",
    "invitation_max_uses",
    "max_webhooks_per_tenant",
    "max_geofences_per_tenant",
    "max_geofences_per_session",
    "geofence_confirm_samples",
    "geofence_exit_buffer_m",
    "geofence_max_accuracy_m",
    "history_allowed",
    "tools_allowed",
    "pools_allowed",
    "edge_allowed",
    "analytics_allowed",
    "analytics_k_anonymity",
];

/// Tenant policy stored in `tenants.policy` (JSONB) with normative defaults and ranges (docs/17 §4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TenantPolicy {
    pub default_session_ttl_seconds: u64,
    pub max_session_ttl_seconds: u64,
    pub default_max_members_one_to_many: u32,
    pub default_max_members_many_to_many: u32,
    pub max_members_cap: u32,
    pub snapshot_ttl_default_seconds: u64,
    pub snapshot_ttl_max_seconds: u64,
    pub stale_after_seconds_default: u64,
    pub min_publish_interval_ms: u64,
    pub publish_burst: u32,
    pub max_samples_per_publish: u32,
    pub max_samples_per_publish_http: u32,
    pub max_horizontal_accuracy_m: f64,
    pub reject_null_island: bool,
    pub max_active_sessions_per_subject: u32,
    pub max_devices_per_subject: u32,
    pub invitation_max_ttl_seconds: u64,
    pub invitation_max_uses: u32,
    pub max_webhooks_per_tenant: u32,
    pub max_geofences_per_tenant: u32,
    pub max_geofences_per_session: u32,
    pub geofence_confirm_samples: u32,
    pub geofence_exit_buffer_m: f64,
    pub geofence_max_accuracy_m: f64,
    pub history_allowed: bool,
    /// `POST /v1/tools/*` (`23` §4–§7). Off by default (`403 FEATURE_DISABLED`).
    pub tools_allowed: bool,
    /// `POOL` sessions and `nearby`/`rings`/`density` (`23`). Off by default.
    pub pools_allowed: bool,
    /// Tags, beacon reports and LPWAN (`25`). Off by default.
    pub edge_allowed: bool,
    /// k-anonymous aggregates (`24` §6). Requires `history_allowed`.
    pub analytics_allowed: bool,
    /// The `k` applied to every aggregate (`24` §6).
    pub analytics_k_anonymity: u32,
}

impl Default for TenantPolicy {
    fn default() -> Self {
        Self {
            default_session_ttl_seconds: 86400,
            max_session_ttl_seconds: 604800,
            default_max_members_one_to_many: 100,
            default_max_members_many_to_many: 50,
            max_members_cap: 1000,
            snapshot_ttl_default_seconds: 600,
            snapshot_ttl_max_seconds: 86400,
            stale_after_seconds_default: 60,
            min_publish_interval_ms: 1000,
            publish_burst: 5,
            max_samples_per_publish: 1,
            max_samples_per_publish_http: 50,
            max_horizontal_accuracy_m: 50000.0,
            reject_null_island: true,
            max_active_sessions_per_subject: 50,
            max_devices_per_subject: 10,
            invitation_max_ttl_seconds: 604800,
            invitation_max_uses: 100,
            max_webhooks_per_tenant: 10,
            max_geofences_per_tenant: 10000,
            max_geofences_per_session: 100,
            geofence_confirm_samples: 2,
            geofence_exit_buffer_m: 20.0,
            geofence_max_accuracy_m: 100.0,
            history_allowed: false,
            tools_allowed: false,
            pools_allowed: false,
            edge_allowed: false,
            analytics_allowed: false,
            analytics_k_anonymity: 5,
        }
    }
}

impl TenantPolicy {
    /// Validates all ranges and cross-validations of the tenant policy.
    pub fn validate(&self) -> Result<(), PolicyError> {
        // Individual ranges
        if !(3600..=31536000).contains(&self.max_session_ttl_seconds) {
            return Err(PolicyError::OutOfRange {
                key: "max_session_ttl_seconds".to_string(),
                message: format!(
                    "value {} outside range 3600..=31536000",
                    self.max_session_ttl_seconds
                ),
            });
        }

        if self.default_session_ttl_seconds < 60
            || self.default_session_ttl_seconds > self.max_session_ttl_seconds
        {
            return Err(PolicyError::OutOfRange {
                key: "default_session_ttl_seconds".to_string(),
                message: format!(
                    "value {} outside range 60..={}",
                    self.default_session_ttl_seconds, self.max_session_ttl_seconds
                ),
            });
        }

        if !(2..=10000).contains(&self.max_members_cap) {
            return Err(PolicyError::OutOfRange {
                key: "max_members_cap".to_string(),
                message: format!("value {} outside range 2..=10000", self.max_members_cap),
            });
        }

        if self.default_max_members_one_to_many < 2
            || self.default_max_members_one_to_many > self.max_members_cap
        {
            return Err(PolicyError::OutOfRange {
                key: "default_max_members_one_to_many".to_string(),
                message: format!(
                    "value {} outside range 2..={}",
                    self.default_max_members_one_to_many, self.max_members_cap
                ),
            });
        }

        if self.default_max_members_many_to_many < 2
            || self.default_max_members_many_to_many > self.max_members_cap
        {
            return Err(PolicyError::OutOfRange {
                key: "default_max_members_many_to_many".to_string(),
                message: format!(
                    "value {} outside range 2..={}",
                    self.default_max_members_many_to_many, self.max_members_cap
                ),
            });
        }

        if self.snapshot_ttl_max_seconds > 86400 {
            return Err(PolicyError::OutOfRange {
                key: "snapshot_ttl_max_seconds".to_string(),
                message: format!(
                    "value {} outside range 0..=86400",
                    self.snapshot_ttl_max_seconds
                ),
            });
        }

        if self.snapshot_ttl_default_seconds > self.snapshot_ttl_max_seconds {
            return Err(PolicyError::OutOfRange {
                key: "snapshot_ttl_default_seconds".to_string(),
                message: format!(
                    "value {} outside range 0..={}",
                    self.snapshot_ttl_default_seconds, self.snapshot_ttl_max_seconds
                ),
            });
        }

        if !(5..=3600).contains(&self.stale_after_seconds_default) {
            return Err(PolicyError::OutOfRange {
                key: "stale_after_seconds_default".to_string(),
                message: format!(
                    "value {} outside range 5..=3600",
                    self.stale_after_seconds_default
                ),
            });
        }

        if !(200..=60000).contains(&self.min_publish_interval_ms) {
            return Err(PolicyError::OutOfRange {
                key: "min_publish_interval_ms".to_string(),
                message: format!(
                    "value {} outside range 200..=60000",
                    self.min_publish_interval_ms
                ),
            });
        }

        if !(1..=20).contains(&self.publish_burst) {
            return Err(PolicyError::OutOfRange {
                key: "publish_burst".to_string(),
                message: format!("value {} outside range 1..=20", self.publish_burst),
            });
        }

        if !(1..=50).contains(&self.max_samples_per_publish) {
            return Err(PolicyError::OutOfRange {
                key: "max_samples_per_publish".to_string(),
                message: format!(
                    "value {} outside range 1..=50",
                    self.max_samples_per_publish
                ),
            });
        }

        if !(1..=50).contains(&self.max_samples_per_publish_http) {
            return Err(PolicyError::OutOfRange {
                key: "max_samples_per_publish_http".to_string(),
                message: format!(
                    "value {} outside range 1..=50",
                    self.max_samples_per_publish_http
                ),
            });
        }

        if !(100.0..=100000.0).contains(&self.max_horizontal_accuracy_m) {
            return Err(PolicyError::OutOfRange {
                key: "max_horizontal_accuracy_m".to_string(),
                message: format!(
                    "value {} outside range 100.0..=100000.0",
                    self.max_horizontal_accuracy_m
                ),
            });
        }

        if !(1..=1000).contains(&self.max_active_sessions_per_subject) {
            return Err(PolicyError::OutOfRange {
                key: "max_active_sessions_per_subject".to_string(),
                message: format!(
                    "value {} outside range 1..=1000",
                    self.max_active_sessions_per_subject
                ),
            });
        }

        if !(1..=100).contains(&self.max_devices_per_subject) {
            return Err(PolicyError::OutOfRange {
                key: "max_devices_per_subject".to_string(),
                message: format!(
                    "value {} outside range 1..=100",
                    self.max_devices_per_subject
                ),
            });
        }

        if !(60..=2592000).contains(&self.invitation_max_ttl_seconds) {
            return Err(PolicyError::OutOfRange {
                key: "invitation_max_ttl_seconds".to_string(),
                message: format!(
                    "value {} outside range 60..=2592000",
                    self.invitation_max_ttl_seconds
                ),
            });
        }

        if !(1..=10000).contains(&self.invitation_max_uses) {
            return Err(PolicyError::OutOfRange {
                key: "invitation_max_uses".to_string(),
                message: format!("value {} outside range 1..=10000", self.invitation_max_uses),
            });
        }

        if !(1..=100).contains(&self.max_webhooks_per_tenant) {
            return Err(PolicyError::OutOfRange {
                key: "max_webhooks_per_tenant".to_string(),
                message: format!(
                    "value {} outside range 1..=100",
                    self.max_webhooks_per_tenant
                ),
            });
        }

        if self.max_geofences_per_tenant > 100000 {
            return Err(PolicyError::OutOfRange {
                key: "max_geofences_per_tenant".to_string(),
                message: format!(
                    "value {} outside range 0..=100000",
                    self.max_geofences_per_tenant
                ),
            });
        }

        if self.max_geofences_per_session > 1000 {
            return Err(PolicyError::OutOfRange {
                key: "max_geofences_per_session".to_string(),
                message: format!(
                    "value {} outside range 0..=1000",
                    self.max_geofences_per_session
                ),
            });
        }

        if !(1..=10).contains(&self.geofence_confirm_samples) {
            return Err(PolicyError::OutOfRange {
                key: "geofence_confirm_samples".to_string(),
                message: format!(
                    "value {} outside range 1..=10",
                    self.geofence_confirm_samples
                ),
            });
        }

        if !(0.0..=500.0).contains(&self.geofence_exit_buffer_m) {
            return Err(PolicyError::OutOfRange {
                key: "geofence_exit_buffer_m".to_string(),
                message: format!(
                    "value {} outside range 0.0..=500.0",
                    self.geofence_exit_buffer_m
                ),
            });
        }

        if !(10.0..=1000.0).contains(&self.geofence_max_accuracy_m) {
            return Err(PolicyError::OutOfRange {
                key: "geofence_max_accuracy_m".to_string(),
                message: format!(
                    "value {} outside range 10.0..=1000.0",
                    self.geofence_max_accuracy_m
                ),
            });
        }

        // Cross validations
        if self.default_session_ttl_seconds > self.max_session_ttl_seconds {
            return Err(PolicyError::CrossValidation {
                key1: "default_session_ttl_seconds".to_string(),
                key2: "max_session_ttl_seconds".to_string(),
                message: format!(
                    "default_session_ttl_seconds ({}) must be <= max_session_ttl_seconds ({})",
                    self.default_session_ttl_seconds, self.max_session_ttl_seconds
                ),
            });
        }

        if self.default_max_members_one_to_many > self.max_members_cap {
            return Err(PolicyError::CrossValidation {
                key1: "default_max_members_one_to_many".to_string(),
                key2: "max_members_cap".to_string(),
                message: format!(
                    "default_max_members_one_to_many ({}) must be <= max_members_cap ({})",
                    self.default_max_members_one_to_many, self.max_members_cap
                ),
            });
        }

        if self.default_max_members_many_to_many > self.max_members_cap {
            return Err(PolicyError::CrossValidation {
                key1: "default_max_members_many_to_many".to_string(),
                key2: "max_members_cap".to_string(),
                message: format!(
                    "default_max_members_many_to_many ({}) must be <= max_members_cap ({})",
                    self.default_max_members_many_to_many, self.max_members_cap
                ),
            });
        }

        if self.snapshot_ttl_default_seconds > self.snapshot_ttl_max_seconds {
            return Err(PolicyError::CrossValidation {
                key1: "snapshot_ttl_default_seconds".to_string(),
                key2: "snapshot_ttl_max_seconds".to_string(),
                message: format!(
                    "snapshot_ttl_default_seconds ({}) must be <= snapshot_ttl_max_seconds ({})",
                    self.snapshot_ttl_default_seconds, self.snapshot_ttl_max_seconds
                ),
            });
        }

        if !(2..=100).contains(&self.analytics_k_anonymity) {
            return Err(PolicyError::OutOfRange {
                key: "analytics_k_anonymity".to_string(),
                message: format!("value {} outside range 2..=100", self.analytics_k_anonymity),
            });
        }

        // `analytics_allowed = true` requires `history_allowed = true`: there are
        // no aggregates without history to aggregate (`17` §4, cross-validation).
        if self.analytics_allowed && !self.history_allowed {
            return Err(PolicyError::CrossValidation {
                key1: "analytics_allowed".to_string(),
                key2: "history_allowed".to_string(),
                message: "analytics_allowed = true requires history_allowed = true".to_string(),
            });
        }

        Ok(())
    }

    /// Parses a JSON value into a `TenantPolicy`, merging with defaults, rejecting unknown keys,
    /// and validating all ranges and cross-validations.
    pub fn from_json_value(value: Value) -> Result<Self, PolicyError> {
        let obj = match value {
            Value::Object(map) => map,
            other => {
                return Err(PolicyError::JsonError(format!(
                    "expected JSON object for policy, got {other:?}"
                )));
            }
        };

        let known_keys: HashSet<&'static str> = POLICY_KEYS.iter().copied().collect();

        // Check for unknown keys (17 §4: "Las claves desconocidas se rechazan")
        for key in obj.keys() {
            if !known_keys.contains(key.as_str()) {
                return Err(PolicyError::UnknownKey(key.clone()));
            }
        }

        let mut policy = Self::default();

        if let Some(v) = obj.get("default_session_ttl_seconds") {
            policy.default_session_ttl_seconds =
                v.as_u64().ok_or_else(|| PolicyError::InvalidType {
                    key: "default_session_ttl_seconds".to_string(),
                    message: "expected positive integer".to_string(),
                })?;
        }

        if let Some(v) = obj.get("max_session_ttl_seconds") {
            policy.max_session_ttl_seconds =
                v.as_u64().ok_or_else(|| PolicyError::InvalidType {
                    key: "max_session_ttl_seconds".to_string(),
                    message: "expected positive integer".to_string(),
                })?;
        }

        if let Some(v) = obj.get("default_max_members_one_to_many") {
            policy.default_max_members_one_to_many = v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| PolicyError::InvalidType {
                    key: "default_max_members_one_to_many".to_string(),
                    message: "expected positive 32-bit integer".to_string(),
                })?;
        }

        if let Some(v) = obj.get("default_max_members_many_to_many") {
            policy.default_max_members_many_to_many = v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| PolicyError::InvalidType {
                    key: "default_max_members_many_to_many".to_string(),
                    message: "expected positive 32-bit integer".to_string(),
                })?;
        }

        if let Some(v) = obj.get("max_members_cap") {
            policy.max_members_cap =
                v.as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(|| PolicyError::InvalidType {
                        key: "max_members_cap".to_string(),
                        message: "expected positive 32-bit integer".to_string(),
                    })?;
        }

        if let Some(v) = obj.get("snapshot_ttl_default_seconds") {
            policy.snapshot_ttl_default_seconds =
                v.as_u64().ok_or_else(|| PolicyError::InvalidType {
                    key: "snapshot_ttl_default_seconds".to_string(),
                    message: "expected non-negative integer".to_string(),
                })?;
        }

        if let Some(v) = obj.get("snapshot_ttl_max_seconds") {
            policy.snapshot_ttl_max_seconds =
                v.as_u64().ok_or_else(|| PolicyError::InvalidType {
                    key: "snapshot_ttl_max_seconds".to_string(),
                    message: "expected non-negative integer".to_string(),
                })?;
        }

        if let Some(v) = obj.get("stale_after_seconds_default") {
            policy.stale_after_seconds_default =
                v.as_u64().ok_or_else(|| PolicyError::InvalidType {
                    key: "stale_after_seconds_default".to_string(),
                    message: "expected positive integer".to_string(),
                })?;
        }

        if let Some(v) = obj.get("min_publish_interval_ms") {
            policy.min_publish_interval_ms =
                v.as_u64().ok_or_else(|| PolicyError::InvalidType {
                    key: "min_publish_interval_ms".to_string(),
                    message: "expected positive integer".to_string(),
                })?;
        }

        if let Some(v) = obj.get("publish_burst") {
            policy.publish_burst =
                v.as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(|| PolicyError::InvalidType {
                        key: "publish_burst".to_string(),
                        message: "expected positive 32-bit integer".to_string(),
                    })?;
        }

        if let Some(v) = obj.get("max_samples_per_publish") {
            policy.max_samples_per_publish = v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| PolicyError::InvalidType {
                    key: "max_samples_per_publish".to_string(),
                    message: "expected positive 32-bit integer".to_string(),
                })?;
        }

        if let Some(v) = obj.get("max_samples_per_publish_http") {
            policy.max_samples_per_publish_http = v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| PolicyError::InvalidType {
                    key: "max_samples_per_publish_http".to_string(),
                    message: "expected positive 32-bit integer".to_string(),
                })?;
        }

        if let Some(v) = obj.get("max_horizontal_accuracy_m") {
            policy.max_horizontal_accuracy_m =
                v.as_f64().ok_or_else(|| PolicyError::InvalidType {
                    key: "max_horizontal_accuracy_m".to_string(),
                    message: "expected float number".to_string(),
                })?;
        }

        if let Some(v) = obj.get("reject_null_island") {
            policy.reject_null_island = v.as_bool().ok_or_else(|| PolicyError::InvalidType {
                key: "reject_null_island".to_string(),
                message: "expected boolean".to_string(),
            })?;
        }

        if let Some(v) = obj.get("max_active_sessions_per_subject") {
            policy.max_active_sessions_per_subject = v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| PolicyError::InvalidType {
                    key: "max_active_sessions_per_subject".to_string(),
                    message: "expected positive 32-bit integer".to_string(),
                })?;
        }

        if let Some(v) = obj.get("max_devices_per_subject") {
            policy.max_devices_per_subject = v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| PolicyError::InvalidType {
                    key: "max_devices_per_subject".to_string(),
                    message: "expected positive 32-bit integer".to_string(),
                })?;
        }

        if let Some(v) = obj.get("invitation_max_ttl_seconds") {
            policy.invitation_max_ttl_seconds =
                v.as_u64().ok_or_else(|| PolicyError::InvalidType {
                    key: "invitation_max_ttl_seconds".to_string(),
                    message: "expected positive integer".to_string(),
                })?;
        }

        if let Some(v) = obj.get("invitation_max_uses") {
            policy.invitation_max_uses = v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| PolicyError::InvalidType {
                    key: "invitation_max_uses".to_string(),
                    message: "expected positive 32-bit integer".to_string(),
                })?;
        }

        if let Some(v) = obj.get("max_webhooks_per_tenant") {
            policy.max_webhooks_per_tenant = v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| PolicyError::InvalidType {
                    key: "max_webhooks_per_tenant".to_string(),
                    message: "expected positive 32-bit integer".to_string(),
                })?;
        }

        if let Some(v) = obj.get("max_geofences_per_tenant") {
            policy.max_geofences_per_tenant = v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| PolicyError::InvalidType {
                    key: "max_geofences_per_tenant".to_string(),
                    message: "expected non-negative 32-bit integer".to_string(),
                })?;
        }

        if let Some(v) = obj.get("max_geofences_per_session") {
            policy.max_geofences_per_session = v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| PolicyError::InvalidType {
                    key: "max_geofences_per_session".to_string(),
                    message: "expected non-negative 32-bit integer".to_string(),
                })?;
        }

        if let Some(v) = obj.get("geofence_confirm_samples") {
            policy.geofence_confirm_samples = v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| PolicyError::InvalidType {
                    key: "geofence_confirm_samples".to_string(),
                    message: "expected positive 32-bit integer".to_string(),
                })?;
        }

        if let Some(v) = obj.get("geofence_exit_buffer_m") {
            policy.geofence_exit_buffer_m = v.as_f64().ok_or_else(|| PolicyError::InvalidType {
                key: "geofence_exit_buffer_m".to_string(),
                message: "expected float number".to_string(),
            })?;
        }

        if let Some(v) = obj.get("geofence_max_accuracy_m") {
            policy.geofence_max_accuracy_m =
                v.as_f64().ok_or_else(|| PolicyError::InvalidType {
                    key: "geofence_max_accuracy_m".to_string(),
                    message: "expected float number".to_string(),
                })?;
        }

        if let Some(v) = obj.get("history_allowed") {
            policy.history_allowed = v.as_bool().ok_or_else(|| PolicyError::InvalidType {
                key: "history_allowed".to_string(),
                message: "expected boolean".to_string(),
            })?;
        }

        for (key, slot) in [
            ("tools_allowed", &mut policy.tools_allowed),
            ("pools_allowed", &mut policy.pools_allowed),
            ("edge_allowed", &mut policy.edge_allowed),
            ("analytics_allowed", &mut policy.analytics_allowed),
        ] {
            if let Some(v) = obj.get(key) {
                *slot = v.as_bool().ok_or_else(|| PolicyError::InvalidType {
                    key: key.to_string(),
                    message: "expected boolean".to_string(),
                })?;
            }
        }

        if let Some(v) = obj.get("analytics_k_anonymity") {
            policy.analytics_k_anonymity = v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| PolicyError::InvalidType {
                    key: "analytics_k_anonymity".to_string(),
                    message: "expected positive 32-bit integer".to_string(),
                })?;
        }

        // Validate complete policy ranges and cross-validations
        policy.validate()?;

        Ok(policy)
    }

    /// Parses a JSON string slice into a `TenantPolicy`.
    pub fn from_json_str(s: &str) -> Result<Self, PolicyError> {
        let val: Value = serde_json::from_str(s)?;
        Self::from_json_value(val)
    }
}
