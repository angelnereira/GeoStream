//! Server configuration loading, validation, and environment restrictions (docs/17 §1–§3, FR-OPS-003).
#![forbid(unsafe_code)]

use ipnet::IpNet;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;

use crate::env::{Environment, LogFormat, StateBackend};
use crate::error::ConfigError;
use crate::secret::{DataEncryptionKey, SecretString};

pub const EXAMPLE_DATABASE_URL: &str =
    "postgres://geostream:geostream_dev_password@localhost:5432/geostream_dev";
pub const EXAMPLE_REDIS_URL: &str = "redis://localhost:6379";
pub const EXAMPLE_DATA_ENCRYPTION_KEY: &str = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";

pub const KNOWN_SERVER_ENV_KEYS: &[&str] = &[
    "GS_ENV",
    "GS_CONFIG_FILE",
    "GS_INSTANCE_ID",
    "GS_HTTP_ADDR",
    "GS_ADMIN_ADDR",
    "GS_PUBLIC_BASE_URL",
    "GS_TRUST_PROXY_HEADERS",
    "GS_TRUSTED_PROXY_CIDRS",
    "GS_SHUTDOWN_GRACE_S",
    "GS_LOG_FORMAT",
    "GS_LOG_LEVEL",
    "GS_LOG_UNSAFE_LOCATIONS",
    "GS_OTLP_ENDPOINT",
    "GS_DATABASE_URL",
    "GS_DATABASE_URL_FILE",
    "GS_DATABASE_MAX_CONNECTIONS",
    "GS_DATABASE_CONNECT_TIMEOUT_MS",
    "GS_DATABASE_STATEMENT_TIMEOUT_MS",
    "GS_AUTO_MIGRATE",
    "GS_STATE_BACKEND",
    "GS_REDIS_URL",
    "GS_REDIS_URL_FILE",
    "GS_REDIS_KEY_PREFIX",
    "GS_REDIS_COMMAND_TIMEOUT_MS",
    "GS_PRESENCE_TTL_S",
    "GS_AUTHZ_CACHE_TTL_MS",
    "GS_JWT_ISSUER",
    "GS_JWT_KEYS_DIR",
    "GS_JWT_ACTIVE_KID",
    "GS_JWT_LEEWAY_S",
    "GS_TOKEN_MIN_TTL_S",
    "GS_TOKEN_DEFAULT_TTL_S",
    "GS_TOKEN_MAX_TTL_S",
    "GS_TICKET_TTL_S",
    "GS_DATA_ENCRYPTION_KEY",
    "GS_DATA_ENCRYPTION_KEY_FILE",
    "GS_DATA_ENCRYPTION_KEYS_OLD",
    "GS_DATA_ENCRYPTION_KEYS_OLD_FILE",
    "GS_REST_MAX_BODY_BYTES",
    "GS_REST_RATE_LIMIT_DEVICE_RPM",
    "GS_REST_RATE_LIMIT_APP_RPM",
    "GS_INVITE_REDEEM_RPM",
    "GS_AUTH_FAILURE_RPM_PER_IP",
    "GS_STREAM_JSON_ENABLED",
    "GS_STREAM_MAX_CONNECTIONS",
    "GS_STREAM_MAX_FRAME_BYTES",
    "GS_STREAM_HELLO_TIMEOUT_MS",
    "GS_STREAM_HEARTBEAT_INTERVAL_MS",
    "GS_STREAM_IDLE_TIMEOUT_MS",
    "GS_STREAM_MAX_SUBSCRIPTIONS",
    "GS_STREAM_MAX_SUBSCRIPTIONS_APP",
    "GS_STREAM_OUTBOUND_CONTROL_QUEUE",
    "GS_STREAM_PENDING_EVENTS_MAX",
    "GS_STREAM_OFFLINE_GRACE_MS",
    "GS_STREAM_AUTH_EXPIRING_LEAD_S",
    "GS_STREAM_APP_MAX_CONNECTION_AGE_S",
    "GS_STREAM_UPGRADE_RPM_PER_IP",
    "GS_STREAM_MAX_INBOUND_MSGS_PER_S",
    "GS_STREAM_ABUSE_WINDOW_S",
    "GS_STREAM_MAX_PROTOCOL_ERRORS",
    "GS_STREAM_DRAIN_SPREAD_MS",
    "GS_LOCATION_MAX_FUTURE_SKEW_MS",
    "GS_LOCATION_MAX_EVENT_AGE_MS",
    "GS_WEBHOOK_TIMEOUT_MS",
    "GS_WEBHOOK_MAX_ATTEMPTS",
    "GS_WEBHOOK_DISABLE_AFTER_H",
    "GS_WEBHOOK_ALLOW_PRIVATE_NETWORKS",
    "GS_WEBHOOK_ALLOWED_PORTS",
    "GS_WORKER_EXPIRY_INTERVAL_S",
    "GS_WORKER_WEBHOOK_POLL_MS",
    "GS_WORKER_RETENTION_HOUR_UTC",
    "GS_WORKER_BATCH_SIZE",
    "GS_RETENTION_AUDIT_D",
    "GS_RETENTION_IDEMPOTENCY_H",
    "GS_RETENTION_INVITATIONS_D",
    "GS_RETENTION_WEBHOOK_DELIVERIES_D",
    "GS_RETENTION_CLOSED_SESSIONS_D",
];

/// Server configuration matching docs/17 §2.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub env: Environment,
    pub instance_id: String,
    pub http_addr: SocketAddr,
    pub admin_addr: SocketAddr,
    pub public_base_url: String,
    pub trust_proxy_headers: bool,
    pub trusted_proxy_cidrs: Vec<IpNet>,
    pub shutdown_grace_s: u64,
    pub log_format: LogFormat,
    pub log_level: String,
    pub log_unsafe_locations: bool,
    pub otlp_endpoint: Option<String>,

    pub database_url: SecretString,
    pub database_max_connections: u32,
    pub database_connect_timeout_ms: u64,
    pub database_statement_timeout_ms: u64,
    pub auto_migrate: bool,

    pub state_backend: StateBackend,
    pub redis_url: Option<SecretString>,
    pub redis_key_prefix: String,
    pub redis_command_timeout_ms: u64,
    pub presence_ttl_s: u64,
    pub authz_cache_ttl_ms: u64,

    pub jwt_issuer: String,
    pub jwt_keys_dir: PathBuf,
    pub jwt_active_kid: String,
    pub jwt_leeway_s: u64,
    pub token_min_ttl_s: u64,
    pub token_default_ttl_s: u64,
    pub token_max_ttl_s: u64,
    pub ticket_ttl_s: u64,
    pub data_encryption_key: Option<DataEncryptionKey>,
    pub data_encryption_keys_old: Vec<DataEncryptionKey>,

    pub rest_max_body_bytes: usize,
    pub rest_rate_limit_device_rpm: u32,
    pub rest_rate_limit_app_rpm: u32,
    pub invite_redeem_rpm: u32,
    pub auth_failure_rpm_per_ip: u32,

    pub stream_json_enabled: bool,
    pub stream_max_connections: u32,
    pub stream_max_frame_bytes: usize,
    pub stream_hello_timeout_ms: u64,
    pub stream_heartbeat_interval_ms: u64,
    pub stream_idle_timeout_ms: u64,
    pub stream_max_subscriptions: u32,
    pub stream_max_subscriptions_app: u32,
    pub stream_outbound_control_queue: usize,
    pub stream_pending_events_max: usize,
    pub stream_offline_grace_ms: u64,
    pub stream_auth_expiring_lead_s: u64,
    pub stream_app_max_connection_age_s: u64,
    pub stream_upgrade_rpm_per_ip: u32,
    pub stream_max_inbound_msgs_per_s: u32,
    pub stream_abuse_window_s: u64,
    pub stream_max_protocol_errors: u32,
    pub stream_drain_spread_ms: u64,

    pub location_max_future_skew_ms: u64,
    pub location_max_event_age_ms: u64,

    pub webhook_timeout_ms: u64,
    pub webhook_max_attempts: u32,
    pub webhook_disable_after_h: u32,
    pub webhook_allow_private_networks: bool,
    pub webhook_allowed_ports: Vec<u16>,

    pub worker_expiry_interval_s: u64,
    pub worker_webhook_poll_ms: u64,
    pub worker_retention_hour_utc: u32,
    pub worker_batch_size: u32,
    pub retention_audit_d: u32,
    pub retention_idempotency_h: u32,
    pub retention_invitations_d: u32,
    pub retention_webhook_deliveries_d: u32,
    pub retention_closed_sessions_d: u32,
}

impl ServerConfig {
    /// Loads configuration from process environment and optional TOML file specified by `GS_CONFIG_FILE`.
    pub fn load() -> Result<Self, ConfigError> {
        let env_map: HashMap<String, String> = std::env::vars().collect();
        Self::load_from_map(&env_map)
    }

    /// Loads configuration from a given environment map and optional TOML file.
    pub fn load_from_map(env_map: &HashMap<String, String>) -> Result<Self, ConfigError> {
        // 1. Check for unknown GS_* environment variables
        let known_env: HashSet<&'static str> = KNOWN_SERVER_ENV_KEYS.iter().copied().collect();
        for key in env_map.keys() {
            if key.starts_with("GS_") && !known_env.contains(key.as_str()) {
                // Special check for integrator keys from 17 §6
                if key != "GS_API_KEY" && key != "GS_BASE_URL" {
                    return Err(ConfigError::UnknownKey(key.clone()));
                }
            }
        }

        // 2. Parse optional TOML configuration file
        let mut toml_map: HashMap<String, toml::Value> = HashMap::new();
        if let Some(config_file_path) = env_map.get("GS_CONFIG_FILE") {
            let content = std::fs::read_to_string(config_file_path).map_err(|e| {
                ConfigError::FileReadError {
                    path: config_file_path.clone(),
                    message: e.to_string(),
                }
            })?;

            let parsed: toml::Table =
                toml::from_str(&content).map_err(|e| ConfigError::InvalidValue {
                    key: "GS_CONFIG_FILE".to_string(),
                    message: format!("invalid TOML format: {e}"),
                })?;

            // Build valid TOML keys set: lowercase of known_env without "GS_" prefix
            let known_toml: HashSet<String> = KNOWN_SERVER_ENV_KEYS
                .iter()
                .filter(|k| !k.ends_with("_FILE"))
                .map(|k| k.strip_prefix("GS_").unwrap().to_ascii_lowercase())
                .collect();

            for (k, v) in parsed {
                if !known_toml.contains(&k) {
                    return Err(ConfigError::UnknownKey(k));
                }
                toml_map.insert(k, v);
            }
        }

        // Helper to resolve string value (Env > Env_FILE > TOML)
        let get_str = |key_upper: &str| -> Result<Option<String>, ConfigError> {
            if let Some(val) = env_map.get(key_upper) {
                return Ok(Some(val.clone()));
            }
            // Check file secret variant
            let file_key = format!("{key_upper}_FILE");
            if let Some(file_path) = env_map.get(&file_key) {
                let content =
                    std::fs::read_to_string(file_path).map_err(|e| ConfigError::FileReadError {
                        path: file_path.clone(),
                        message: format!("failed to read secret from {file_key}: {e}"),
                    })?;
                return Ok(Some(content.trim().to_string()));
            }
            // Check TOML
            let toml_key = key_upper.strip_prefix("GS_").unwrap().to_ascii_lowercase();
            if let Some(tv) = toml_map.get(&toml_key) {
                match tv {
                    toml::Value::String(s) => return Ok(Some(s.clone())),
                    other => {
                        return Err(ConfigError::InvalidValue {
                            key: key_upper.to_string(),
                            message: format!("expected string in TOML, got {other}"),
                        });
                    }
                }
            }
            Ok(None)
        };

        // Helper for booleans
        let get_bool = |key_upper: &str, default: bool| -> Result<bool, ConfigError> {
            if let Some(val) = env_map.get(key_upper) {
                match val.trim().to_ascii_lowercase().as_str() {
                    "true" | "1" => return Ok(true),
                    "false" | "0" => return Ok(false),
                    other => {
                        return Err(ConfigError::InvalidValue {
                            key: key_upper.to_string(),
                            message: format!("expected boolean ('true' or 'false'), got '{other}'"),
                        });
                    }
                }
            }
            let toml_key = key_upper.strip_prefix("GS_").unwrap().to_ascii_lowercase();
            if let Some(tv) = toml_map.get(&toml_key) {
                match tv {
                    toml::Value::Boolean(b) => return Ok(*b),
                    other => {
                        return Err(ConfigError::InvalidValue {
                            key: key_upper.to_string(),
                            message: format!("expected boolean in TOML, got {other}"),
                        });
                    }
                }
            }
            Ok(default)
        };

        // Helper for integers
        let get_u64 =
            |key_upper: &str, default: u64, min: u64, max: u64| -> Result<u64, ConfigError> {
                let val_u64 = if let Some(val) = env_map.get(key_upper) {
                    val.trim()
                        .parse::<u64>()
                        .map_err(|e| ConfigError::InvalidValue {
                            key: key_upper.to_string(),
                            message: format!("expected unsigned integer: {e}"),
                        })?
                } else {
                    let toml_key = key_upper.strip_prefix("GS_").unwrap().to_ascii_lowercase();
                    if let Some(tv) = toml_map.get(&toml_key) {
                        match tv {
                            toml::Value::Integer(i) if *i >= 0 => *i as u64,
                            other => {
                                return Err(ConfigError::InvalidValue {
                                    key: key_upper.to_string(),
                                    message: format!(
                                        "expected positive integer in TOML, got {other}"
                                    ),
                                });
                            }
                        }
                    } else {
                        default
                    }
                };

                if val_u64 < min || val_u64 > max {
                    return Err(ConfigError::OutOfRange {
                        key: key_upper.to_string(),
                        message: format!("value {val_u64} outside range {min}..={max}"),
                    });
                }
                Ok(val_u64)
            };

        let get_u32 =
            |key_upper: &str, default: u32, min: u32, max: u32| -> Result<u32, ConfigError> {
                let n = get_u64(key_upper, default as u64, min as u64, max as u64)?;
                u32::try_from(n).map_err(|_| ConfigError::OutOfRange {
                    key: key_upper.to_string(),
                    message: format!("value {n} exceeds 32-bit integer range"),
                })
            };

        let get_usize = |key_upper: &str,
                         default: usize,
                         min: usize,
                         max: usize|
         -> Result<usize, ConfigError> {
            let n = get_u64(key_upper, default as u64, min as u64, max as u64)?;
            usize::try_from(n).map_err(|_| ConfigError::OutOfRange {
                key: key_upper.to_string(),
                message: format!("value {n} exceeds usize integer range"),
            })
        };

        // Environment
        let env = match get_str("GS_ENV")? {
            Some(s) => s.parse::<Environment>()?,
            None => Environment::Dev,
        };

        // Instance ID: ^[a-z0-9-]{1,64}$
        let instance_id = match get_str("GS_INSTANCE_ID")? {
            Some(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty()
                    || trimmed.len() > 64
                    || !trimmed
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                {
                    return Err(ConfigError::InvalidValue {
                        key: "GS_INSTANCE_ID".to_string(),
                        message: format!("instance ID '{trimmed}' must match ^[a-z0-9-]{{1,64}}$"),
                    });
                }
                trimmed.to_string()
            }
            None => {
                // Generate a random uuid v4
                uuid::Uuid::new_v4().to_string()
            }
        };

        // HTTP Addr
        let http_addr_str = get_str("GS_HTTP_ADDR")?.unwrap_or_else(|| "0.0.0.0:8080".to_string());
        let http_addr: SocketAddr =
            http_addr_str
                .parse()
                .map_err(|e| ConfigError::InvalidValue {
                    key: "GS_HTTP_ADDR".to_string(),
                    message: format!("invalid socket address '{http_addr_str}': {e}"),
                })?;

        // Admin Addr
        let admin_addr_str =
            get_str("GS_ADMIN_ADDR")?.unwrap_or_else(|| "127.0.0.1:9090".to_string());
        let admin_addr: SocketAddr =
            admin_addr_str
                .parse()
                .map_err(|e| ConfigError::InvalidValue {
                    key: "GS_ADMIN_ADDR".to_string(),
                    message: format!("invalid socket address '{admin_addr_str}': {e}"),
                })?;

        // Public Base URL
        let public_base_url =
            get_str("GS_PUBLIC_BASE_URL")?.unwrap_or_else(|| "http://localhost:8080".to_string());
        if env == Environment::Prod && !public_base_url.starts_with("https://") {
            return Err(ConfigError::EnvironmentRestriction {
                key: "GS_PUBLIC_BASE_URL".to_string(),
                env: "prod".to_string(),
                message: "GS_PUBLIC_BASE_URL must use https:// in production".to_string(),
            });
        }

        let trust_proxy_headers = get_bool("GS_TRUST_PROXY_HEADERS", false)?;

        let trusted_proxy_cidrs = match get_str("GS_TRUSTED_PROXY_CIDRS")? {
            Some(s) if !s.trim().is_empty() => {
                let mut cidrs = Vec::new();
                for part in s.split(',') {
                    let trimmed = part.trim();
                    if !trimmed.is_empty() {
                        let cidr: IpNet =
                            trimmed.parse().map_err(|e| ConfigError::InvalidValue {
                                key: "GS_TRUSTED_PROXY_CIDRS".to_string(),
                                message: format!("invalid CIDR '{trimmed}': {e}"),
                            })?;
                        cidrs.push(cidr);
                    }
                }
                cidrs
            }
            _ => Vec::new(),
        };

        let shutdown_grace_s = get_u64("GS_SHUTDOWN_GRACE_S", 30, 5, 300)?;

        let log_format = match get_str("GS_LOG_FORMAT")? {
            Some(s) => s.parse::<LogFormat>()?,
            None => match env {
                Environment::Dev => LogFormat::Pretty,
                _ => LogFormat::Json,
            },
        };

        let log_level = get_str("GS_LOG_LEVEL")?.unwrap_or_else(|| "info".to_string());

        let log_unsafe_locations = get_bool("GS_LOG_UNSAFE_LOCATIONS", false)?;
        if log_unsafe_locations && env != Environment::Dev {
            return Err(ConfigError::EnvironmentRestriction {
                key: "GS_LOG_UNSAFE_LOCATIONS".to_string(),
                env: env.as_str().to_string(),
                message: "GS_LOG_UNSAFE_LOCATIONS=true is permitted only in dev".to_string(),
            });
        }

        let otlp_endpoint = get_str("GS_OTLP_ENDPOINT")?;

        // Database
        let database_url_str = get_str("GS_DATABASE_URL")?
            .ok_or_else(|| ConfigError::MissingKey("GS_DATABASE_URL".to_string()))?;
        if env == Environment::Prod && database_url_str == EXAMPLE_DATABASE_URL {
            return Err(ConfigError::EnvironmentRestriction {
                key: "GS_DATABASE_URL".to_string(),
                env: "prod".to_string(),
                message: "example database credentials from deploy/.env.example are forbidden in production".to_string(),
            });
        }
        let database_url = SecretString::new(database_url_str);
        let database_max_connections = get_u32("GS_DATABASE_MAX_CONNECTIONS", 20, 1, 200)?;
        let database_connect_timeout_ms =
            get_u64("GS_DATABASE_CONNECT_TIMEOUT_MS", 5000, 100, 60000)?;
        let database_statement_timeout_ms =
            get_u64("GS_DATABASE_STATEMENT_TIMEOUT_MS", 5000, 100, 60000)?;

        let auto_migrate = get_bool("GS_AUTO_MIGRATE", false)?;
        if auto_migrate && env == Environment::Prod {
            return Err(ConfigError::EnvironmentRestriction {
                key: "GS_AUTO_MIGRATE".to_string(),
                env: "prod".to_string(),
                message: "GS_AUTO_MIGRATE=true is forbidden in production (run geostream migrate separately)".to_string(),
            });
        }

        // State backend & Redis
        let state_backend = match get_str("GS_STATE_BACKEND")? {
            Some(s) => s.parse::<StateBackend>()?,
            None => StateBackend::Memory,
        };

        let redis_url_str = get_str("GS_REDIS_URL")?;
        if state_backend == StateBackend::Redis && redis_url_str.as_deref().unwrap_or("").is_empty()
        {
            return Err(ConfigError::MissingKey("GS_REDIS_URL".to_string()));
        }
        if env == Environment::Prod && redis_url_str.as_deref() == Some(EXAMPLE_REDIS_URL) {
            return Err(ConfigError::EnvironmentRestriction {
                key: "GS_REDIS_URL".to_string(),
                env: "prod".to_string(),
                message:
                    "example redis credentials from deploy/.env.example are forbidden in production"
                        .to_string(),
            });
        }
        let redis_url = redis_url_str.map(SecretString::new);

        let redis_key_prefix = get_str("GS_REDIS_KEY_PREFIX")?.unwrap_or_else(|| "gs".to_string());
        if redis_key_prefix.is_empty()
            || redis_key_prefix.len() > 32
            || !redis_key_prefix
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        {
            return Err(ConfigError::InvalidValue {
                key: "GS_REDIS_KEY_PREFIX".to_string(),
                message: format!(
                    "redis key prefix '{redis_key_prefix}' must match ^[a-z0-9_-]{{1,32}}$"
                ),
            });
        }

        let redis_command_timeout_ms = get_u64("GS_REDIS_COMMAND_TIMEOUT_MS", 500, 50, 10000)?;
        let presence_ttl_s = get_u64("GS_PRESENCE_TTL_S", 90, 30, 600)?;
        let authz_cache_ttl_ms = get_u64("GS_AUTHZ_CACHE_TTL_MS", 5000, 500, 30000)?;

        // Auth & Keys
        let jwt_issuer = get_str("GS_JWT_ISSUER")?.unwrap_or_else(|| public_base_url.clone());
        let jwt_keys_dir_str = get_str("GS_JWT_KEYS_DIR")?
            .ok_or_else(|| ConfigError::MissingKey("GS_JWT_KEYS_DIR".to_string()))?;
        let jwt_keys_dir = PathBuf::from(jwt_keys_dir_str);

        let jwt_active_kid = get_str("GS_JWT_ACTIVE_KID")?
            .ok_or_else(|| ConfigError::MissingKey("GS_JWT_ACTIVE_KID".to_string()))?;
        if jwt_active_kid.is_empty()
            || jwt_active_kid.len() > 64
            || !jwt_active_kid
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(ConfigError::InvalidValue {
                key: "GS_JWT_ACTIVE_KID".to_string(),
                message: format!("active kid '{jwt_active_kid}' must match ^[a-z0-9-]{{1,64}}$"),
            });
        }

        let jwt_leeway_s = get_u64("GS_JWT_LEEWAY_S", 30, 0, 120)?;
        let token_min_ttl_s = get_u64("GS_TOKEN_MIN_TTL_S", 300, 60, 3600)?;
        let token_max_ttl_s = get_u64("GS_TOKEN_MAX_TTL_S", 3600, token_min_ttl_s, 86400)?;
        let token_default_ttl_s = get_u64(
            "GS_TOKEN_DEFAULT_TTL_S",
            900,
            token_min_ttl_s,
            token_max_ttl_s,
        )?;
        let ticket_ttl_s = get_u64("GS_TICKET_TTL_S", 30, 5, 120)?;

        // Encryption keys
        let data_encryption_key = if let Some(key_b64) = get_str("GS_DATA_ENCRYPTION_KEY")? {
            if env == Environment::Prod && key_b64 == EXAMPLE_DATA_ENCRYPTION_KEY {
                return Err(ConfigError::EnvironmentRestriction {
                    key: "GS_DATA_ENCRYPTION_KEY".to_string(),
                    env: "prod".to_string(),
                    message:
                        "example encryption key from deploy/.env.example is forbidden in production"
                            .to_string(),
                });
            }
            Some(DataEncryptionKey::from_base64(
                &key_b64,
                "GS_DATA_ENCRYPTION_KEY",
            )?)
        } else {
            None
        };

        let data_encryption_keys_old =
            if let Some(keys_b64_str) = get_str("GS_DATA_ENCRYPTION_KEYS_OLD")? {
                let mut list = Vec::new();
                for part in keys_b64_str.split(',') {
                    let trimmed = part.trim();
                    if !trimmed.is_empty() {
                        list.push(DataEncryptionKey::from_base64(
                            trimmed,
                            "GS_DATA_ENCRYPTION_KEYS_OLD",
                        )?);
                    }
                }
                list
            } else {
                Vec::new()
            };

        // REST limits
        let rest_max_body_bytes = get_usize("GS_REST_MAX_BODY_BYTES", 65536, 1024, 1048576)?;
        let rest_rate_limit_device_rpm = get_u32("GS_REST_RATE_LIMIT_DEVICE_RPM", 120, 1, 10000)?;
        let rest_rate_limit_app_rpm = get_u32("GS_REST_RATE_LIMIT_APP_RPM", 6000, 1, 1000000)?;
        let invite_redeem_rpm = get_u32("GS_INVITE_REDEEM_RPM", 10, 1, 100)?;
        let auth_failure_rpm_per_ip = get_u32("GS_AUTH_FAILURE_RPM_PER_IP", 60, 1, 10000)?;

        // Stream
        let stream_json_default = env != Environment::Prod;
        let stream_json_enabled = get_bool("GS_STREAM_JSON_ENABLED", stream_json_default)?;
        let stream_max_connections = get_u32("GS_STREAM_MAX_CONNECTIONS", 20000, 1, 1000000)?;
        let stream_max_frame_bytes = get_usize("GS_STREAM_MAX_FRAME_BYTES", 16384, 1024, 1048576)?;
        let stream_hello_timeout_ms = get_u64("GS_STREAM_HELLO_TIMEOUT_MS", 5000, 1000, 30000)?;
        let stream_heartbeat_interval_ms =
            get_u64("GS_STREAM_HEARTBEAT_INTERVAL_MS", 20000, 5000, 60000)?;

        let idle_min = 2 * stream_heartbeat_interval_ms;
        let stream_idle_timeout_ms = get_u64("GS_STREAM_IDLE_TIMEOUT_MS", 60000, idle_min, 300000)?;

        let stream_max_subscriptions = get_u32("GS_STREAM_MAX_SUBSCRIPTIONS", 20, 1, 200)?;
        let stream_max_subscriptions_app =
            get_u32("GS_STREAM_MAX_SUBSCRIPTIONS_APP", 1000, 1, 100000)?;
        let stream_outbound_control_queue =
            get_usize("GS_STREAM_OUTBOUND_CONTROL_QUEUE", 1024, 64, 65536)?;
        let stream_pending_events_max = get_usize("GS_STREAM_PENDING_EVENTS_MAX", 256, 16, 4096)?;
        let stream_offline_grace_ms = get_u64("GS_STREAM_OFFLINE_GRACE_MS", 10000, 0, 120000)?;
        let stream_auth_expiring_lead_s = get_u64("GS_STREAM_AUTH_EXPIRING_LEAD_S", 60, 10, 600)?;
        let stream_app_max_connection_age_s =
            get_u64("GS_STREAM_APP_MAX_CONNECTION_AGE_S", 3600, 300, 86400)?;
        let stream_upgrade_rpm_per_ip = get_u32("GS_STREAM_UPGRADE_RPM_PER_IP", 120, 1, 100000)?;
        let stream_max_inbound_msgs_per_s =
            get_u32("GS_STREAM_MAX_INBOUND_MSGS_PER_S", 20, 1, 1000)?;
        let stream_abuse_window_s = get_u64("GS_STREAM_ABUSE_WINDOW_S", 10, 1, 300)?;
        let stream_max_protocol_errors = get_u32("GS_STREAM_MAX_PROTOCOL_ERRORS", 10, 1, 100)?;
        let stream_drain_spread_ms = get_u64("GS_STREAM_DRAIN_SPREAD_MS", 5000, 0, 60000)?;

        // Location
        let location_max_future_skew_ms =
            get_u64("GS_LOCATION_MAX_FUTURE_SKEW_MS", 5000, 0, 60000)?;
        let location_max_event_age_ms =
            get_u64("GS_LOCATION_MAX_EVENT_AGE_MS", 120000, 5000, 86400000)?;

        // Webhooks
        let webhook_timeout_ms = get_u64("GS_WEBHOOK_TIMEOUT_MS", 10000, 1000, 30000)?;
        let webhook_max_attempts = get_u32("GS_WEBHOOK_MAX_ATTEMPTS", 10, 1, 20)?;
        let webhook_disable_after_h = get_u32("GS_WEBHOOK_DISABLE_AFTER_H", 72, 1, 720)?;
        let webhook_allow_private_networks = get_bool("GS_WEBHOOK_ALLOW_PRIVATE_NETWORKS", false)?;

        let webhook_allowed_ports = match get_str("GS_WEBHOOK_ALLOWED_PORTS")? {
            Some(s) if !s.trim().is_empty() => {
                let mut ports = Vec::new();
                for part in s.split(',') {
                    let trimmed = part.trim();
                    if !trimmed.is_empty() {
                        let p = trimmed
                            .parse::<u16>()
                            .map_err(|e| ConfigError::InvalidValue {
                                key: "GS_WEBHOOK_ALLOWED_PORTS".to_string(),
                                message: format!("invalid port '{trimmed}': {e}"),
                            })?;
                        if p == 0 {
                            return Err(ConfigError::OutOfRange {
                                key: "GS_WEBHOOK_ALLOWED_PORTS".to_string(),
                                message: "port 0 is not allowed".to_string(),
                            });
                        }
                        ports.push(p);
                    }
                }
                ports
            }
            _ => vec![443],
        };

        // Worker & Retention
        let worker_expiry_interval_s = get_u64("GS_WORKER_EXPIRY_INTERVAL_S", 15, 1, 300)?;
        let worker_webhook_poll_ms = get_u64("GS_WORKER_WEBHOOK_POLL_MS", 1000, 100, 60000)?;
        let worker_retention_hour_utc = get_u32("GS_WORKER_RETENTION_HOUR_UTC", 3, 0, 23)?;
        let worker_batch_size = get_u32("GS_WORKER_BATCH_SIZE", 500, 10, 10000)?;
        let retention_audit_d = get_u32("GS_RETENTION_AUDIT_D", 365, 1, 3650)?;
        let retention_idempotency_h = get_u32("GS_RETENTION_IDEMPOTENCY_H", 24, 1, 168)?;
        let retention_invitations_d = get_u32("GS_RETENTION_INVITATIONS_D", 30, 1, 365)?;
        let retention_webhook_deliveries_d =
            get_u32("GS_RETENTION_WEBHOOK_DELIVERIES_D", 30, 1, 365)?;
        let retention_closed_sessions_d = get_u32("GS_RETENTION_CLOSED_SESSIONS_D", 90, 1, 3650)?;

        Ok(Self {
            env,
            instance_id,
            http_addr,
            admin_addr,
            public_base_url,
            trust_proxy_headers,
            trusted_proxy_cidrs,
            shutdown_grace_s,
            log_format,
            log_level,
            log_unsafe_locations,
            otlp_endpoint,
            database_url,
            database_max_connections,
            database_connect_timeout_ms,
            database_statement_timeout_ms,
            auto_migrate,
            state_backend,
            redis_url,
            redis_key_prefix,
            redis_command_timeout_ms,
            presence_ttl_s,
            authz_cache_ttl_ms,
            jwt_issuer,
            jwt_keys_dir,
            jwt_active_kid,
            jwt_leeway_s,
            token_min_ttl_s,
            token_default_ttl_s,
            token_max_ttl_s,
            ticket_ttl_s,
            data_encryption_key,
            data_encryption_keys_old,
            rest_max_body_bytes,
            rest_rate_limit_device_rpm,
            rest_rate_limit_app_rpm,
            invite_redeem_rpm,
            auth_failure_rpm_per_ip,
            stream_json_enabled,
            stream_max_connections,
            stream_max_frame_bytes,
            stream_hello_timeout_ms,
            stream_heartbeat_interval_ms,
            stream_idle_timeout_ms,
            stream_max_subscriptions,
            stream_max_subscriptions_app,
            stream_outbound_control_queue,
            stream_pending_events_max,
            stream_offline_grace_ms,
            stream_auth_expiring_lead_s,
            stream_app_max_connection_age_s,
            stream_upgrade_rpm_per_ip,
            stream_max_inbound_msgs_per_s,
            stream_abuse_window_s,
            stream_max_protocol_errors,
            stream_drain_spread_ms,
            location_max_future_skew_ms,
            location_max_event_age_ms,
            webhook_timeout_ms,
            webhook_max_attempts,
            webhook_disable_after_h,
            webhook_allow_private_networks,
            webhook_allowed_ports,
            worker_expiry_interval_s,
            worker_webhook_poll_ms,
            worker_retention_hour_utc,
            worker_batch_size,
            retention_audit_d,
            retention_idempotency_h,
            retention_invitations_d,
            retention_webhook_deliveries_d,
            retention_closed_sessions_d,
        })
    }
}
