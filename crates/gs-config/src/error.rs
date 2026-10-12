//! Error types for configuration and tenant policies (FR-OPS-003, FR-ADM-002).
#![forbid(unsafe_code)]

/// Errors occurring during server configuration loading and validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("unknown configuration key '{0}'")]
    UnknownKey(String),

    #[error("missing required configuration key '{0}'")]
    MissingKey(String),

    #[error("invalid value for configuration key '{key}': {message}")]
    InvalidValue { key: String, message: String },

    #[error("out of range value for configuration key '{key}': {message}")]
    OutOfRange { key: String, message: String },

    #[error("cross-validation failed between '{key1}' and '{key2}': {message}")]
    CrossValidation {
        key1: String,
        key2: String,
        message: String,
    },

    #[error("environment restriction violated for key '{key}' in environment '{env}': {message}")]
    EnvironmentRestriction {
        key: String,
        env: String,
        message: String,
    },

    #[error("failed to read configuration file '{path}': {message}")]
    FileReadError { path: String, message: String },
}

/// Errors occurring during tenant policy parsing and validation (17 §4, FR-ADM-002).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PolicyError {
    #[error("unknown policy key '{0}'")]
    UnknownKey(String),

    #[error("invalid type for policy key '{key}': {message}")]
    InvalidType { key: String, message: String },

    #[error("out of range value for policy key '{key}': {message}")]
    OutOfRange { key: String, message: String },

    #[error("cross-validation failed between '{key1}' and '{key2}': {message}")]
    CrossValidation {
        key1: String,
        key2: String,
        message: String,
    },

    #[error("JSON policy format error: {0}")]
    JsonError(String),
}

impl From<serde_json::Error> for PolicyError {
    fn from(err: serde_json::Error) -> Self {
        Self::JsonError(err.to_string())
    }
}
