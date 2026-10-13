//! Device JWT issuance and verification (docs/06_IDENTITY_AUTH.md §4).
//!
//! Steps 1 to 3 of the verification order of `docs/06` §4.3 (compact form and `kid`, EdDSA
//! signature, `iss`/`aud`/`nbf`/`exp` with `GS_JWT_LEEWAY_S`) are implemented here. Step 4
//! (tenant, application, subject epoch and device state) and step 5 (scope) need the truth in
//! PostgreSQL and belong to the authentication middleware of the service layer.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::Path;

use ed25519_dalek::pkcs8::EncodePrivateKey;
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, TokenData, Validation, decode, decode_header,
    encode,
};
use serde::{Deserialize, Serialize};
use thiserror::Error as ThisError;
use time::OffsetDateTime;

use gs_domain::error::{AsErrorCode, ErrorCode};
use gs_domain::ids::{ApplicationId, DeviceId, SubjectId, TenantId};

use crate::signing_key::SigningKeyError;

/// Audience of every GeoStream device token (docs/06 §4.2).
pub const TOKEN_AUDIENCE: &str = "geostream";

/// Suffix of the private key files that the key set loads (docs/06 §4.4).
const PRIVATE_KEY_FILE_SUFFIX: &str = ".key.pem";

/// Suffix of the public key files that the key set loads (docs/06 §4.4).
const PUBLIC_KEY_FILE_SUFFIX: &str = ".pub.pem";

/// Claims of a device token, in the exact shape of `contracts/test-vectors/token_claims.json`
/// (docs/06 §4.2). No other claim may be added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceTokenClaims {
    /// Issuer (`GS_JWT_ISSUER`).
    pub iss: String,
    /// Audience: always [`TOKEN_AUDIENCE`].
    pub aud: String,
    /// Subject id.
    pub sub: SubjectId,
    /// Tenant id.
    pub tid: TenantId,
    /// Application id.
    pub aid: ApplicationId,
    /// Device id.
    pub did: DeviceId,
    /// Device scopes.
    pub scp: Vec<String>,
    /// Token epoch of the subject at issuance time.
    pub sep: u32,
    /// Issued at (seconds).
    pub iat: i64,
    /// Not before (seconds).
    pub nbf: i64,
    /// Expires at (seconds).
    pub exp: i64,
    /// Token id (UUIDv4).
    pub jti: String,
}

impl DeviceTokenClaims {
    /// Builds claims for issuance time `issued_at` with the given ttl (docs/06 §4.2).
    // The argument list is the normative claim set of docs/06 §4.2; a builder would not make
    // the token shape clearer.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        issuer: impl Into<String>,
        subject_id: SubjectId,
        tenant_id: TenantId,
        application_id: ApplicationId,
        device_id: DeviceId,
        scopes: Vec<String>,
        token_epoch: u32,
        issued_at: OffsetDateTime,
        ttl_seconds: u32,
    ) -> Result<Self, JwtError> {
        let issuer = issuer.into();
        if issuer.is_empty() {
            return Err(JwtError::MissingIssuer);
        }
        if ttl_seconds == 0 {
            return Err(JwtError::InvalidTtl);
        }
        let issued_at_unix = issued_at.unix_timestamp();
        let expires_at_unix = issued_at_unix + i64::from(ttl_seconds);
        Ok(Self {
            iss: issuer,
            aud: TOKEN_AUDIENCE.to_string(),
            sub: subject_id,
            tid: tenant_id,
            aid: application_id,
            did: device_id,
            scp: scopes,
            sep: token_epoch,
            iat: issued_at_unix,
            nbf: issued_at_unix,
            exp: expires_at_unix,
            jti: uuid::Uuid::new_v4().to_string(),
        })
    }

    /// Returns true when the scope is granted to the token (docs/06 §3.4, §6).
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scp.iter().any(|granted| granted == scope)
    }
}

/// Issued token: the compact JWS plus its claims.
#[derive(Debug, Clone)]
pub struct IssuedDeviceToken {
    /// Compact JWS (`header.payload.signature`).
    pub token: String,
    /// Claims embedded in the token.
    pub claims: DeviceTokenClaims,
}

/// Errors of token issuance, verification and key management.
#[derive(Debug, ThisError)]
pub enum JwtError {
    /// The issuer is empty.
    #[error("token issuer must not be empty")]
    MissingIssuer,

    /// The ttl is zero or out of range.
    #[error("token ttl must be greater than zero")]
    InvalidTtl,

    /// The keys directory could not be read.
    #[error("cannot read signing keys directory '{}': {message}", path.display())]
    KeyDirectory {
        path: std::path::PathBuf,
        message: String,
    },

    /// A key file is malformed.
    #[error("malformed signing key '{}': {message}", path.display())]
    MalformedKey {
        path: std::path::PathBuf,
        message: String,
    },

    /// The active kid has no private key in the key set.
    #[error("no private key for active signing key id '{kid}'")]
    MissingSigningKey { kid: String },

    /// The token could not be encoded.
    #[error("cannot encode device token: {0}")]
    Encoding(String),

    /// The token is not a valid JWS (docs/06 §4.3 step 1).
    #[error("invalid device token format")]
    InvalidFormat,

    /// The token algorithm is not EdDSA (docs/06 §4.3 step 1).
    #[error("device token must be signed with EdDSA")]
    InvalidAlgorithm,

    /// The token kid is missing or unknown (docs/06 §4.3 step 1).
    #[error("unknown device token signing key id")]
    UnknownKid,

    /// The token is expired (docs/06 §4.3 step 3).
    #[error("device token expired")]
    Expired,

    /// The token is not valid yet (docs/06 §4.3 step 3).
    #[error("device token is not valid yet")]
    NotYetValid,

    /// Signature or claim verification failed (docs/06 §4.3 steps 2 and 3).
    #[error("device token verification failed: {0}")]
    Verification(String),
}

impl AsErrorCode for JwtError {
    fn error_code(&self) -> ErrorCode {
        match self {
            Self::Expired => ErrorCode::TokenExpired,
            _ => ErrorCode::Unauthenticated,
        }
    }
}

impl From<SigningKeyError> for JwtError {
    fn from(error: SigningKeyError) -> Self {
        match error {
            SigningKeyError::AlreadyExists { path } => Self::MalformedKey {
                path,
                message: "file already exists".to_string(),
            },
            other => Self::MalformedKey {
                path: std::path::PathBuf::new(),
                message: other.to_string(),
            },
        }
    }
}

/// Signing key type used by issuance (docs/06 §4.4). Re-exported for the layers above.
pub type SigningKey = ed25519_dalek::SigningKey;

/// Verification key type used by verification (docs/06 §4.4). Re-exported for the layers above.
pub type VerifyingKey = ed25519_dalek::VerifyingKey;

/// Verification and signing keys loaded from `GS_JWT_KEYS_DIR` (docs/06 §4.4).
///
/// Verification accepts every `<kid>.pub.pem` of the directory (rotation window: the old pair
/// stays until every token signed with it expires). Issuance uses `GS_JWT_ACTIVE_KID`, which
/// must have a `<kid>.key.pem`.
#[derive(Debug, Default)]
pub struct SigningKeySet {
    verifying: BTreeMap<String, VerifyingKey>,
    signing: BTreeMap<String, SigningKey>,
    active_kid: Option<String>,
}

impl SigningKeySet {
    /// Loads every PEM pair found in `keys_dir`.
    pub fn load_from_dir(keys_dir: &Path) -> Result<Self, JwtError> {
        let mut set = Self::default();

        let entries = std::fs::read_dir(keys_dir).map_err(|e| JwtError::KeyDirectory {
            path: keys_dir.to_path_buf(),
            message: e.to_string(),
        })?;

        let mut private_files: Vec<std::path::PathBuf> = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| JwtError::KeyDirectory {
                path: keys_dir.to_path_buf(),
                message: e.to_string(),
            })?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if let Some(kid) = name.strip_suffix(PRIVATE_KEY_FILE_SUFFIX) {
                crate::signing_key::validate_kid(kid)?;
                private_files.push(path);
            } else if let Some(kid) = name.strip_suffix(PUBLIC_KEY_FILE_SUFFIX) {
                crate::signing_key::validate_kid(kid)?;
                let pem = std::fs::read_to_string(&path).map_err(|e| JwtError::MalformedKey {
                    path: path.clone(),
                    message: e.to_string(),
                })?;
                let verifying = crate::signing_key::parse_public_pem(&pem).map_err(|e| {
                    JwtError::MalformedKey {
                        path: path.clone(),
                        message: e.to_string(),
                    }
                })?;
                set.verifying.insert(kid.to_string(), verifying);
            }
        }

        for path in private_files {
            let Some(kid) = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.strip_suffix(".key"))
                .map(str::to_string)
            else {
                continue;
            };
            let pem = std::fs::read_to_string(&path).map_err(|e| JwtError::MalformedKey {
                path: path.clone(),
                message: e.to_string(),
            })?;
            let signing = crate::signing_key::parse_private_pem(&pem).map_err(|e| {
                JwtError::MalformedKey {
                    path: path.clone(),
                    message: e.to_string(),
                }
            })?;
            set.signing.insert(kid, signing);
        }

        Ok(set)
    }

    /// Selects the active signing key id (`GS_JWT_ACTIVE_KID`).
    pub fn set_active_kid(&mut self, kid: &str) -> Result<(), JwtError> {
        if !self.signing.contains_key(kid) {
            return Err(JwtError::MissingSigningKey {
                kid: kid.to_string(),
            });
        }
        self.active_kid = Some(kid.to_string());
        Ok(())
    }

    /// Active signing key, if any.
    pub fn active_signing_key(&self) -> Option<(&str, &SigningKey)> {
        let kid = self.active_kid.as_deref()?;
        self.signing.get(kid).map(|key| (kid, key))
    }

    /// Returns every verification key with its kid.
    pub fn verifying_key(&self, kid: &str) -> Option<&ed25519_dalek::VerifyingKey> {
        self.verifying.get(kid)
    }
}

/// Issues a device token (docs/06 §4.1–§4.2).
pub fn issue_device_token(
    signing_key: &ed25519_dalek::SigningKey,
    kid: &str,
    claims: &DeviceTokenClaims,
) -> Result<IssuedDeviceToken, JwtError> {
    crate::signing_key::validate_kid(kid)?;

    let header = Header {
        alg: Algorithm::EdDSA,
        kid: Some(kid.to_string()),
        typ: Some("JWT".to_string()),
        ..Header::default()
    };
    let private_der = signing_key
        .to_pkcs8_der()
        .map_err(|e| JwtError::Encoding(e.to_string()))?;
    let encoding_key = EncodingKey::from_ed_der(private_der.as_bytes());

    let token =
        encode(&header, claims, &encoding_key).map_err(|e| JwtError::Encoding(e.to_string()))?;

    Ok(IssuedDeviceToken {
        token,
        claims: claims.clone(),
    })
}

/// Verifies a device token following the order of docs/06 §4.3 steps 1 to 3.
///
/// `leeway_seconds` is `GS_JWT_LEEWAY_S`. The principal state of step 4 (tenant, application,
/// subject epoch, device) and the scope check of step 5 are applied by the caller.
pub fn verify_device_token(
    token: &str,
    keys: &SigningKeySet,
    expected_issuer: &str,
    leeway_seconds: u64,
) -> Result<TokenData<DeviceTokenClaims>, JwtError> {
    // Step 1: compact form, EdDSA and a kid that belongs to the verification set.
    let header = decode_header(token).map_err(|_| JwtError::InvalidFormat)?;
    if header.alg != Algorithm::EdDSA {
        return Err(JwtError::InvalidAlgorithm);
    }
    let kid = header.kid.as_deref().ok_or(JwtError::UnknownKid)?;
    let verifying_key = keys.verifying_key(kid).ok_or(JwtError::UnknownKid)?;

    let public_der = verifying_key.to_bytes();
    let decoding_key = DecodingKey::from_ed_der(&public_der);

    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.algorithms = vec![Algorithm::EdDSA];
    validation.leeway = leeway_seconds;
    validation.validate_exp = true;
    validation.validate_nbf = true;
    validation.validate_aud = true;
    validation.required_spec_claims = ["exp", "nbf", "aud", "iss", "sub"]
        .iter()
        .map(|claim| (*claim).to_string())
        .collect();
    validation.set_audience(&[TOKEN_AUDIENCE]);
    validation.set_issuer(&[expected_issuer]);

    decode::<DeviceTokenClaims>(token, &decoding_key, &validation).map_err(|error| {
        use jsonwebtoken::errors::ErrorKind;
        match error.kind() {
            ErrorKind::ExpiredSignature => JwtError::Expired,
            ErrorKind::ImmatureSignature => JwtError::NotYetValid,
            other => JwtError::Verification(format!("{other:?}")),
        }
    })
}
