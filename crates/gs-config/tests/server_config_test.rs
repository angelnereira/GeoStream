//! Tests for ServerConfig loading, range checking, environment restrictions, and secrets (docs/17, FR-OPS-003).
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

use gs_config::env::{Environment, LogFormat, StateBackend};
use gs_config::error::ConfigError;
use gs_config::secret::{DataEncryptionKey, SecretString};
use gs_config::server::{
    EXAMPLE_DATA_ENCRYPTION_KEY, EXAMPLE_DATABASE_URL, EXAMPLE_REDIS_URL, ServerConfig,
};

fn base_valid_env() -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert(
        "GS_DATABASE_URL".to_string(),
        "postgres://custom_user:custom_pass@localhost:5432/custom_db".to_string(),
    );
    map.insert("GS_JWT_KEYS_DIR".to_string(), "/var/run/keys".to_string());
    map.insert("GS_JWT_ACTIVE_KID".to_string(), "key-2026".to_string());
    map
}

#[test]
fn test_minimal_config_loads_with_defaults() {
    let env = base_valid_env();
    let config = ServerConfig::load_from_map(&env).expect("valid minimal configuration");

    assert_eq!(config.env, Environment::Dev);
    assert_eq!(config.http_addr.to_string(), "0.0.0.0:8080");
    assert_eq!(config.admin_addr.to_string(), "127.0.0.1:9090");
    assert_eq!(config.public_base_url, "http://localhost:8080");
    assert_eq!(config.shutdown_grace_s, 30);
    assert_eq!(config.log_format, LogFormat::Pretty); // pretty in dev
    assert_eq!(config.log_level, "info");
    assert!(!config.log_unsafe_locations);
    assert_eq!(config.database_max_connections, 20);
    assert_eq!(config.state_backend, StateBackend::Memory);
    assert_eq!(config.presence_ttl_s, 90);
    assert_eq!(config.stream_max_connections, 20000);
    assert_eq!(config.stream_heartbeat_interval_ms, 20000);
    assert_eq!(config.stream_idle_timeout_ms, 60000);
    assert_eq!(config.retention_audit_d, 365);
    assert_eq!(config.jwt_active_kid, "key-2026");
}

#[test]
fn test_missing_required_keys_fail_with_key_name() {
    // Missing GS_DATABASE_URL
    let mut env = base_valid_env();
    env.remove("GS_DATABASE_URL");
    let err = ServerConfig::load_from_map(&env).unwrap_err();
    assert_eq!(err, ConfigError::MissingKey("GS_DATABASE_URL".to_string()));

    // Missing GS_JWT_KEYS_DIR
    let mut env = base_valid_env();
    env.remove("GS_JWT_KEYS_DIR");
    let err = ServerConfig::load_from_map(&env).unwrap_err();
    assert_eq!(err, ConfigError::MissingKey("GS_JWT_KEYS_DIR".to_string()));

    // Missing GS_JWT_ACTIVE_KID
    let mut env = base_valid_env();
    env.remove("GS_JWT_ACTIVE_KID");
    let err = ServerConfig::load_from_map(&env).unwrap_err();
    assert_eq!(
        err,
        ConfigError::MissingKey("GS_JWT_ACTIVE_KID".to_string())
    );
}

#[test]
fn test_unknown_gs_env_key_fails_with_key_name() {
    let mut env = base_valid_env();
    env.insert("GS_UNKNOWN_PARAMETER".to_string(), "foo".to_string());
    let err = ServerConfig::load_from_map(&env).unwrap_err();
    assert_eq!(
        err,
        ConfigError::UnknownKey("GS_UNKNOWN_PARAMETER".to_string())
    );
}

#[test]
fn test_out_of_range_checks() {
    // GS_SHUTDOWN_GRACE_S < 5
    let mut env = base_valid_env();
    env.insert("GS_SHUTDOWN_GRACE_S".to_string(), "4".to_string());
    let err = ServerConfig::load_from_map(&env).unwrap_err();
    match err {
        ConfigError::OutOfRange { key, .. } => assert_eq!(key, "GS_SHUTDOWN_GRACE_S"),
        other => panic!("expected OutOfRange, got {other:?}"),
    }

    // GS_DATABASE_MAX_CONNECTIONS > 200
    let mut env = base_valid_env();
    env.insert("GS_DATABASE_MAX_CONNECTIONS".to_string(), "250".to_string());
    let err = ServerConfig::load_from_map(&env).unwrap_err();
    match err {
        ConfigError::OutOfRange { key, .. } => assert_eq!(key, "GS_DATABASE_MAX_CONNECTIONS"),
        other => panic!("expected OutOfRange, got {other:?}"),
    }

    // GS_REST_MAX_BODY_BYTES < 1024
    let mut env = base_valid_env();
    env.insert("GS_REST_MAX_BODY_BYTES".to_string(), "500".to_string());
    let err = ServerConfig::load_from_map(&env).unwrap_err();
    match err {
        ConfigError::OutOfRange { key, .. } => assert_eq!(key, "GS_REST_MAX_BODY_BYTES"),
        other => panic!("expected OutOfRange, got {other:?}"),
    }

    // GS_STREAM_IDLE_TIMEOUT_MS < 2 * heartbeat (2 * 20000 = 40000)
    let mut env = base_valid_env();
    env.insert("GS_STREAM_IDLE_TIMEOUT_MS".to_string(), "35000".to_string());
    let err = ServerConfig::load_from_map(&env).unwrap_err();
    match err {
        ConfigError::OutOfRange { key, .. } => assert_eq!(key, "GS_STREAM_IDLE_TIMEOUT_MS"),
        other => panic!("expected OutOfRange, got {other:?}"),
    }
}

#[test]
fn test_cross_validations() {
    // Redis backend requires GS_REDIS_URL
    let mut env = base_valid_env();
    env.insert("GS_STATE_BACKEND".to_string(), "redis".to_string());
    let err = ServerConfig::load_from_map(&env).unwrap_err();
    assert_eq!(err, ConfigError::MissingKey("GS_REDIS_URL".to_string()));

    // Token default TTL outside min..max
    let mut env = base_valid_env();
    env.insert("GS_TOKEN_MIN_TTL_S".to_string(), "500".to_string());
    env.insert("GS_TOKEN_DEFAULT_TTL_S".to_string(), "400".to_string());
    let err = ServerConfig::load_from_map(&env).unwrap_err();
    match err {
        ConfigError::OutOfRange { key, .. } => assert_eq!(key, "GS_TOKEN_DEFAULT_TTL_S"),
        other => panic!("expected OutOfRange, got {other:?}"),
    }
}

#[test]
fn test_environment_restrictions() {
    // GS_PUBLIC_BASE_URL without https:// in prod
    let mut env = base_valid_env();
    env.insert("GS_ENV".to_string(), "prod".to_string());
    env.insert(
        "GS_PUBLIC_BASE_URL".to_string(),
        "http://api.geostream.io".to_string(),
    );
    let err = ServerConfig::load_from_map(&env).unwrap_err();
    match err {
        ConfigError::EnvironmentRestriction { key, env, .. } => {
            assert_eq!(key, "GS_PUBLIC_BASE_URL");
            assert_eq!(env, "prod");
        }
        other => panic!("expected EnvironmentRestriction, got {other:?}"),
    }

    // GS_LOG_UNSAFE_LOCATIONS=true in prod
    let mut env = base_valid_env();
    env.insert("GS_ENV".to_string(), "prod".to_string());
    env.insert(
        "GS_PUBLIC_BASE_URL".to_string(),
        "https://api.geostream.io".to_string(),
    );
    env.insert("GS_LOG_UNSAFE_LOCATIONS".to_string(), "true".to_string());
    let err = ServerConfig::load_from_map(&env).unwrap_err();
    match err {
        ConfigError::EnvironmentRestriction { key, env, .. } => {
            assert_eq!(key, "GS_LOG_UNSAFE_LOCATIONS");
            assert_eq!(env, "prod");
        }
        other => panic!("expected EnvironmentRestriction, got {other:?}"),
    }

    // GS_LOG_UNSAFE_LOCATIONS=true in test
    let mut env = base_valid_env();
    env.insert("GS_ENV".to_string(), "test".to_string());
    env.insert("GS_LOG_UNSAFE_LOCATIONS".to_string(), "true".to_string());
    let err = ServerConfig::load_from_map(&env).unwrap_err();
    match err {
        ConfigError::EnvironmentRestriction { key, env, .. } => {
            assert_eq!(key, "GS_LOG_UNSAFE_LOCATIONS");
            assert_eq!(env, "test");
        }
        other => panic!("expected EnvironmentRestriction, got {other:?}"),
    }

    // GS_AUTO_MIGRATE=true in prod
    let mut env = base_valid_env();
    env.insert("GS_ENV".to_string(), "prod".to_string());
    env.insert(
        "GS_PUBLIC_BASE_URL".to_string(),
        "https://api.geostream.io".to_string(),
    );
    env.insert("GS_AUTO_MIGRATE".to_string(), "true".to_string());
    let err = ServerConfig::load_from_map(&env).unwrap_err();
    match err {
        ConfigError::EnvironmentRestriction { key, env, .. } => {
            assert_eq!(key, "GS_AUTO_MIGRATE");
            assert_eq!(env, "prod");
        }
        other => panic!("expected EnvironmentRestriction, got {other:?}"),
    }
}

#[test]
fn test_example_secrets_rejected_in_prod() {
    // Database URL matching deploy/.env.example in prod
    let mut env = base_valid_env();
    env.insert("GS_ENV".to_string(), "prod".to_string());
    env.insert(
        "GS_PUBLIC_BASE_URL".to_string(),
        "https://api.geostream.io".to_string(),
    );
    env.insert(
        "GS_DATABASE_URL".to_string(),
        EXAMPLE_DATABASE_URL.to_string(),
    );
    let err = ServerConfig::load_from_map(&env).unwrap_err();
    match err {
        ConfigError::EnvironmentRestriction { key, .. } => assert_eq!(key, "GS_DATABASE_URL"),
        other => panic!("expected EnvironmentRestriction, got {other:?}"),
    }

    // Redis URL matching deploy/.env.example in prod
    let mut env = base_valid_env();
    env.insert("GS_ENV".to_string(), "prod".to_string());
    env.insert(
        "GS_PUBLIC_BASE_URL".to_string(),
        "https://api.geostream.io".to_string(),
    );
    env.insert("GS_STATE_BACKEND".to_string(), "redis".to_string());
    env.insert("GS_REDIS_URL".to_string(), EXAMPLE_REDIS_URL.to_string());
    let err = ServerConfig::load_from_map(&env).unwrap_err();
    match err {
        ConfigError::EnvironmentRestriction { key, .. } => assert_eq!(key, "GS_REDIS_URL"),
        other => panic!("expected EnvironmentRestriction, got {other:?}"),
    }

    // Encryption key matching deploy/.env.example in prod
    let mut env = base_valid_env();
    env.insert("GS_ENV".to_string(), "prod".to_string());
    env.insert(
        "GS_PUBLIC_BASE_URL".to_string(),
        "https://api.geostream.io".to_string(),
    );
    env.insert(
        "GS_DATA_ENCRYPTION_KEY".to_string(),
        EXAMPLE_DATA_ENCRYPTION_KEY.to_string(),
    );
    let err = ServerConfig::load_from_map(&env).unwrap_err();
    match err {
        ConfigError::EnvironmentRestriction { key, .. } => {
            assert_eq!(key, "GS_DATA_ENCRYPTION_KEY")
        }
        other => panic!("expected EnvironmentRestriction, got {other:?}"),
    }
}

#[test]
fn test_secret_redaction_and_encryption_key() {
    let secret = SecretString::new("my-super-secret-password");
    assert_eq!(format!("{secret:?}"), "SecretString(<redacted>)");
    assert_eq!(format!("{secret}"), "<redacted>");
    assert_eq!(secret.expose_secret(), "my-super-secret-password");

    // 32-byte key: 32 bytes of 'A' (ASCII 65) encoded in base64:
    // "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE="
    let b64 = "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE=";
    let enc_key = DataEncryptionKey::from_base64(b64, "GS_DATA_ENCRYPTION_KEY").unwrap();
    assert_eq!(enc_key.raw_bytes().len(), 32);
    assert_eq!(enc_key.key_id(), "22a48051");
    assert_eq!(
        format!("{enc_key:?}"),
        "DataEncryptionKey { key_id: \"22a48051\", raw_bytes: \"<redacted>\" }"
    );
}

struct TestDir(std::path::PathBuf);

impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("gs_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn test_toml_config_file_loading_and_env_override() {
    let dir = TestDir::new();
    let toml_path = dir.path().join("geostream.toml");

    let toml_content = r#"
stream_max_frame_bytes = 32768
shutdown_grace_s = 45
http_addr = "127.0.0.1:8000"
"#;
    let mut file = File::create(&toml_path).unwrap();
    file.write_all(toml_content.as_bytes()).unwrap();

    let mut env = base_valid_env();
    env.insert(
        "GS_CONFIG_FILE".to_string(),
        toml_path.to_str().unwrap().to_string(),
    );
    // Env override for shutdown_grace_s
    env.insert("GS_SHUTDOWN_GRACE_S".to_string(), "60".to_string());

    let config = ServerConfig::load_from_map(&env).expect("load with toml");
    assert_eq!(config.stream_max_frame_bytes, 32768); // From TOML
    assert_eq!(config.shutdown_grace_s, 60); // Overridden by env
    assert_eq!(config.http_addr.to_string(), "127.0.0.1:8000"); // From TOML
}

#[test]
fn test_unknown_key_in_toml_is_rejected() {
    let dir = TestDir::new();
    let toml_path = dir.path().join("invalid.toml");

    let toml_content = r#"
unknown_setting_name = 123
"#;
    let mut file = File::create(&toml_path).unwrap();
    file.write_all(toml_content.as_bytes()).unwrap();

    let mut env = base_valid_env();
    env.insert(
        "GS_CONFIG_FILE".to_string(),
        toml_path.to_str().unwrap().to_string(),
    );

    let err = ServerConfig::load_from_map(&env).unwrap_err();
    assert_eq!(
        err,
        ConfigError::UnknownKey("unknown_setting_name".to_string())
    );
}

#[test]
fn test_secret_file_variant_loading() {
    let dir = TestDir::new();
    let db_secret_file = dir.path().join("db_password.txt");
    let mut file = File::create(&db_secret_file).unwrap();
    file.write_all(b"postgres://from_file_user:secret@localhost:5432/from_file_db\n")
        .unwrap();

    let mut env = base_valid_env();
    env.remove("GS_DATABASE_URL");
    env.insert(
        "GS_DATABASE_URL_FILE".to_string(),
        db_secret_file.to_str().unwrap().to_string(),
    );

    let config = ServerConfig::load_from_map(&env).expect("load with secret file");
    assert_eq!(
        config.database_url.expose_secret(),
        "postgres://from_file_user:secret@localhost:5432/from_file_db"
    );
}

// ---------- Developer console (ADR-0036, docs/17 §2.12) ----------

fn console_env() -> HashMap<String, String> {
    use base64::Engine as _;
    let mut map = base_valid_env();
    map.insert(
        "GS_CONSOLE_OTP_HMAC_KEY".to_string(),
        base64::engine::general_purpose::STANDARD.encode([7u8; 32]),
    );
    map.insert(
        "GS_CONSOLE_PUBLIC_URL".to_string(),
        "http://localhost:8088".to_string(),
    );
    map.insert(
        "GS_CONSOLE_EMAIL_FROM".to_string(),
        "GeoStream <no-reply@example.test>".to_string(),
    );
    map.insert(
        "GS_CONSOLE_EMAIL_API_KEY".to_string(),
        "re_test_key".to_string(),
    );
    map
}

#[test]
fn test_fr_con_001_the_console_is_off_without_its_key() {
    // covers: FR-CON-001
    let config = ServerConfig::load_from_map(&base_valid_env()).expect("config");
    assert!(config.console.is_none());
}

#[test]
fn test_fr_con_001_console_defaults_match_the_documentation() {
    // covers: FR-CON-001, docs/17 §2.12
    let config = ServerConfig::load_from_map(&console_env()).expect("config");
    let console = config.console.expect("console enabled");
    assert_eq!(console.otp_length, 6);
    assert_eq!(console.otp_ttl_s, 600);
    assert_eq!(console.otp_max_attempts, 5);
    assert_eq!(console.otp_cooldown_s, 60);
    assert_eq!(console.session_ttl_s, 43_200);
    assert_eq!(console.email_daily_cap, 500);
    assert_eq!(console.max_orgs_per_account, 3);
    assert_eq!(console.signup_rpm_per_ip, 10);
    assert_eq!(console.otp_hmac_key.as_bytes(), &[7u8; 32]);
    assert!(matches!(
        console.email,
        gs_config::EmailConfig::Resend { .. }
    ));
}

#[test]
fn test_fr_con_001_console_settings_without_the_key_are_an_error_not_ignored() {
    // covers: FR-CON-001 (a half-configured console must not start silently disabled)
    let mut env = console_env();
    env.remove("GS_CONSOLE_OTP_HMAC_KEY");
    assert_eq!(
        ServerConfig::load_from_map(&env).unwrap_err(),
        ConfigError::MissingKey("GS_CONSOLE_OTP_HMAC_KEY".to_string())
    );
}

#[test]
fn test_fr_con_001_the_hmac_key_must_be_32_bytes_of_base64() {
    // covers: FR-CON-001, 10 T-41
    for bad in ["no-es-base64!!", "AAAA", ""] {
        let mut env = console_env();
        env.insert("GS_CONSOLE_OTP_HMAC_KEY".to_string(), bad.to_string());
        assert!(
            matches!(
                ServerConfig::load_from_map(&env),
                Err(ConfigError::InvalidValue { ref key, .. }) if key == "GS_CONSOLE_OTP_HMAC_KEY"
            ),
            "{bad:?}"
        );
    }
}

#[test]
fn test_fr_con_001_resend_needs_its_api_key_and_a_sender() {
    // covers: FR-CON-001
    let mut env = console_env();
    env.remove("GS_CONSOLE_EMAIL_API_KEY");
    assert_eq!(
        ServerConfig::load_from_map(&env).unwrap_err(),
        ConfigError::MissingKey("GS_CONSOLE_EMAIL_API_KEY".to_string())
    );
    let mut env = console_env();
    env.remove("GS_CONSOLE_EMAIL_FROM");
    assert_eq!(
        ServerConfig::load_from_map(&env).unwrap_err(),
        ConfigError::MissingKey("GS_CONSOLE_EMAIL_FROM".to_string())
    );
}

#[test]
fn test_fr_con_011_the_file_mail_provider_is_never_allowed_in_prod() {
    // covers: FR-CON-011 (the file provider exists for dev and test only)
    let mut env = console_env();
    env.insert("GS_CONSOLE_EMAIL_PROVIDER".to_string(), "file".to_string());
    env.insert("GS_CONSOLE_EMAIL_DIR".to_string(), "/tmp/mail".to_string());
    let dev = ServerConfig::load_from_map(&env).expect("file provider in dev");
    assert!(matches!(
        dev.console.expect("console").email,
        gs_config::EmailConfig::File { .. }
    ));
    env.insert("GS_ENV".to_string(), "prod".to_string());
    env.insert(
        "GS_PUBLIC_BASE_URL".to_string(),
        "https://api.example.test".to_string(),
    );
    env.insert(
        "GS_CONSOLE_PUBLIC_URL".to_string(),
        "https://console.example.test".to_string(),
    );
    assert!(matches!(
        ServerConfig::load_from_map(&env),
        Err(ConfigError::EnvironmentRestriction { ref key, .. }) if key == "GS_CONSOLE_EMAIL_PROVIDER"
    ));
}

#[test]
fn test_fr_con_001_the_console_origin_must_be_https_in_prod() {
    // covers: FR-CON-001, 10 T-33
    let mut env = console_env();
    env.insert("GS_ENV".to_string(), "prod".to_string());
    env.insert(
        "GS_PUBLIC_BASE_URL".to_string(),
        "https://api.example.test".to_string(),
    );
    assert!(matches!(
        ServerConfig::load_from_map(&env),
        Err(ConfigError::EnvironmentRestriction { ref key, .. }) if key == "GS_CONSOLE_PUBLIC_URL"
    ));
}

#[test]
fn test_fr_con_001_console_ranges_are_enforced() {
    // covers: FR-CON-001, FR-CON-009, docs/17 §2.12
    for (key, bad) in [
        ("GS_CONSOLE_OTP_LENGTH", "5"),
        ("GS_CONSOLE_OTP_LENGTH", "11"),
        ("GS_CONSOLE_OTP_TTL_S", "59"),
        ("GS_CONSOLE_OTP_MAX_ATTEMPTS", "2"),
        ("GS_CONSOLE_OTP_MAX_ATTEMPTS", "11"),
        ("GS_CONSOLE_OTP_COOLDOWN_S", "9"),
        ("GS_CONSOLE_SESSION_TTL_S", "899"),
        ("GS_CONSOLE_EMAIL_DAILY_CAP", "0"),
        ("GS_CONSOLE_MAX_ORGS_PER_ACCOUNT", "0"),
        ("GS_CONSOLE_SIGNUP_RPM_PER_IP", "601"),
    ] {
        let mut env = console_env();
        env.insert(key.to_string(), bad.to_string());
        assert!(
            ServerConfig::load_from_map(&env).is_err(),
            "{key}={bad} must be rejected"
        );
    }
}

#[test]
fn test_fr_con_011_console_secrets_are_redacted_in_debug() {
    // covers: FR-CON-011, docs/10 §8.2
    let config = ServerConfig::load_from_map(&console_env()).expect("config");
    let rendered = format!("{config:?}");
    assert!(!rendered.contains("re_test_key"), "provider API key leaked");
    assert!(!rendered.contains("[7, 7, 7"), "HMAC key leaked");
}
