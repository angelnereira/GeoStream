//! API keys, EdDSA JWTs, stream tickets, scopes and principal extraction.
#![forbid(unsafe_code)]

pub mod api_key;
pub mod crypto;
pub mod jwt;
pub mod scopes;
pub mod signing_key;

pub use api_key::{
    ApiKeyError, CREDENTIAL_PREFIX, KEY_ID_HEX_LEN, SECRET_BYTES, SECRET_LEN, format_credential,
    generate_secret, hash_secret, parse_credential, verify_credential,
};
pub use crypto::{
    CryptoError, CryptoRotator, DataCipher, ENVELOPE_VERSION, NONCE_LEN, cipher_from_config_key,
    parse_envelope,
};
pub use jwt::{
    DeviceTokenClaims, IssuedDeviceToken, JwtError, SigningKey, SigningKeySet, TOKEN_AUDIENCE,
    issue_device_token, verify_device_token,
};
pub use scopes::{
    APPLICATION_SCOPES, DEFAULT_API_KEY_SCOPES, DEFAULT_DEVICE_TOKEN_SCOPES, DEVICE_SCOPES,
    LOCATIONS_READ_SCOPE, ScopeError, default_api_key_scopes, is_application_scope,
    is_device_scope, validate_device_scopes, validate_scopes,
};
pub use signing_key::{
    GeneratedSigningKey, PRIVATE_KEY_MODE, PRIVATE_KEY_SUFFIX, PUBLIC_KEY_SUFFIX, SigningKeyError,
    generate as generate_signing_key, parse_private_pem, parse_public_pem, validate_kid,
};

// Effective tenant limits (docs/17_CONFIGURATION.md §4) are enforced by this crate when
// minting device tokens (FR-AUTH-002, INV-DEV-2), and `gs-auth` is the only crate inside the
// `gs-service` dependency cone allowed to depend on `gs-config` (docs/04_ARCHITECTURE.md §4.1).
// They are re-exported here so the service layer can read and validate tenant policy.
pub use gs_config::error::PolicyError;
pub use gs_config::policy::{POLICY_KEYS, TenantPolicy};
