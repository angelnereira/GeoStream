//! Loading and validation of configuration and tenant policies (docs/17, FR-OPS-003, FR-ADM-002).
#![forbid(unsafe_code)]

pub mod console;
pub mod env;
pub mod error;
pub mod policy;
pub mod secret;
pub mod server;

pub use console::{ConsoleConfig, ConsoleHmacKey, EmailConfig};
pub use env::{Environment, LogFormat, StateBackend};
pub use error::{ConfigError, PolicyError};
pub use policy::{POLICY_KEYS, TenantPolicy};
pub use secret::{DataEncryptionKey, SecretString};
pub use server::{
    EXAMPLE_DATA_ENCRYPTION_KEY, EXAMPLE_DATABASE_URL, EXAMPLE_REDIS_URL, KNOWN_SERVER_ENV_KEYS,
    ServerConfig,
};
