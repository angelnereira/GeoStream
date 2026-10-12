//! Ed25519 signing key pair generation for device JWTs (docs/06_IDENTITY_AUTH.md §4.4).
//!
//! The pair is written as `<kid>.key.pem` (PKCS#8 private key, file mode `0600`) and
//! `<kid>.pub.pem` (SPKI public key) inside `GS_JWT_KEYS_DIR`.
//! PEM envelopes are produced from DER with the base64 engine (RFC 7468, 64-character lines).
#![forbid(unsafe_code)]

use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use ed25519_dalek::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey};
use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::Rng;
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

/// Suffix of the private key file inside `GS_JWT_KEYS_DIR` (docs/06 §4.4).
pub const PRIVATE_KEY_SUFFIX: &str = "key.pem";

/// Suffix of the public key file inside `GS_JWT_KEYS_DIR` (docs/06 §4.4).
pub const PUBLIC_KEY_SUFFIX: &str = "pub.pem";

/// Maximum length of a key id (docs/06 §4.4).
pub const KID_MAX_LEN: usize = 64;

/// File mode of the private key file: owner read and write only (docs/06 §4.4).
#[cfg(unix)]
pub const PRIVATE_KEY_MODE: u32 = 0o600;

/// PEM label of the PKCS#8 private key.
const PRIVATE_KEY_PEM_LABEL: &str = "PRIVATE KEY";

/// PEM label of the SPKI public key.
const PUBLIC_KEY_PEM_LABEL: &str = "PUBLIC KEY";

/// Base64 body line width of the PEM envelope (RFC 7468).
const PEM_LINE_WIDTH: usize = 64;

/// Errors produced while generating or persisting a signing key pair.
#[derive(Debug, Error)]
pub enum SigningKeyError {
    /// The key id does not match `^[a-z0-9-]{1,64}$`.
    #[error("invalid key id '{kid}': expected up to 64 characters of [a-z0-9-]")]
    InvalidKid { kid: String },

    /// The target file already exists: keys are never silently overwritten.
    #[error("signing key file already exists: {}", path.display())]
    AlreadyExists { path: PathBuf },

    /// Creating the target directory failed.
    #[error("cannot create signing keys directory '{}': {message}", path.display())]
    CreateDirectory { path: PathBuf, message: String },

    /// Writing a PEM file failed.
    #[error("cannot write signing key file '{}': {message}", path.display())]
    Write { path: PathBuf, message: String },

    /// Encoding the key pair as PKCS#8/SPKI PEM failed.
    #[error("cannot encode Ed25519 key pair as PEM: {message}")]
    Encoding { message: String },
}

/// An Ed25519 key pair generated in memory. The private PEM is zeroized on drop.
pub struct GeneratedSigningKey {
    /// Key id (`kid`) of the pair.
    pub kid: String,
    /// PKCS#8 private key in PEM (redacted in `Debug`).
    private_pem: Zeroizing<String>,
    /// SPKI public key in PEM.
    pub public_pem: String,
}

impl GeneratedSigningKey {
    /// Returns the PKCS#8 private key PEM. Handle as a secret (never logged).
    pub fn private_pem(&self) -> &str {
        &self.private_pem
    }

    /// Writes the pair as `<out_dir>/<kid>.key.pem` (mode `0600`) and `<out_dir>/<kid>.pub.pem`.
    ///
    /// Existing files are never overwritten.
    pub fn write_pair(&self, out_dir: &Path) -> Result<(PathBuf, PathBuf), SigningKeyError> {
        let private_path = out_dir.join(format!("{}.{}", self.kid, PRIVATE_KEY_SUFFIX));
        let public_path = out_dir.join(format!("{}.{}", self.kid, PUBLIC_KEY_SUFFIX));
        if private_path.exists() {
            return Err(SigningKeyError::AlreadyExists { path: private_path });
        }
        if public_path.exists() {
            return Err(SigningKeyError::AlreadyExists { path: public_path });
        }

        std::fs::create_dir_all(out_dir).map_err(|e| SigningKeyError::CreateDirectory {
            path: out_dir.to_path_buf(),
            message: e.to_string(),
        })?;

        write_new_private(&private_path, &self.private_pem)?;
        write_new_file(&public_path, self.public_pem.as_bytes())?;

        Ok((private_path, public_path))
    }
}

impl fmt::Debug for GeneratedSigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GeneratedSigningKey")
            .field("kid", &self.kid)
            .field("private_pem", &"<redacted>")
            .field("public_pem", &self.public_pem)
            .finish()
    }
}

/// Validates a key id against `^[a-z0-9-]{1,64}$` (docs/06 §4.4).
pub fn validate_kid(kid: &str) -> Result<(), SigningKeyError> {
    if kid.is_empty() || kid.len() > KID_MAX_LEN {
        return Err(SigningKeyError::InvalidKid {
            kid: kid.to_string(),
        });
    }
    if !kid
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(SigningKeyError::InvalidKid {
            kid: kid.to_string(),
        });
    }
    Ok(())
}

/// Generates a fresh Ed25519 key pair and encodes it as PKCS#8 and SPKI PEM.
pub fn generate(kid: &str) -> Result<GeneratedSigningKey, SigningKeyError> {
    validate_kid(kid)?;

    let mut secret_bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut secret_bytes);
    let signing_key = SigningKey::from_bytes(&secret_bytes);
    secret_bytes.zeroize();

    let verifying_key: VerifyingKey = signing_key.verifying_key();

    let private_der = signing_key
        .to_pkcs8_der()
        .map_err(|e| SigningKeyError::Encoding {
            message: e.to_string(),
        })?;
    let public_der = verifying_key
        .to_public_key_der()
        .map_err(|e| SigningKeyError::Encoding {
            message: e.to_string(),
        })?;

    let private_pem = pem_envelope(PRIVATE_KEY_PEM_LABEL, private_der.as_bytes())?;
    let public_pem = pem_envelope(PUBLIC_KEY_PEM_LABEL, public_der.as_bytes())?;

    Ok(GeneratedSigningKey {
        kid: kid.to_string(),
        private_pem: Zeroizing::new(private_pem),
        public_pem,
    })
}

/// Parses a PKCS#8 private key PEM produced by [`generate`].
///
/// Used by tests and by the signing stack when loading `GS_JWT_KEYS_DIR`.
pub fn parse_private_pem(pem: &str) -> Result<SigningKey, SigningKeyError> {
    SigningKey::from_pkcs8_pem(pem).map_err(|e| SigningKeyError::Encoding {
        message: e.to_string(),
    })
}

/// Parses an SPKI public key PEM produced by [`generate`].
pub fn parse_public_pem(pem: &str) -> Result<VerifyingKey, SigningKeyError> {
    VerifyingKey::from_public_key_pem(pem).map_err(|e| SigningKeyError::Encoding {
        message: e.to_string(),
    })
}

/// Wraps DER bytes in a PEM envelope with 64-character base64 lines (RFC 7468).
fn pem_envelope(label: &str, der: &[u8]) -> Result<String, SigningKeyError> {
    let encoded = BASE64_STANDARD.encode(der);
    let mut out = String::with_capacity(encoded.len() + 2 * label.len() + 32);
    out.push_str("-----BEGIN ");
    out.push_str(label);
    out.push_str("-----\n");
    for chunk in encoded.as_bytes().chunks(PEM_LINE_WIDTH) {
        let line = std::str::from_utf8(chunk).map_err(|_| SigningKeyError::Encoding {
            message: "base64 body is not valid ASCII".to_string(),
        })?;
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("-----END ");
    out.push_str(label);
    out.push_str("-----\n");
    Ok(out)
}

/// Creates a file with `0600` permissions (unix) and writes the private PEM.
fn write_new_private(path: &Path, pem: &str) -> Result<(), SigningKeyError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(PRIVATE_KEY_MODE);
    }

    let mut file = options.open(path).map_err(|e| SigningKeyError::Write {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    file.write_all(pem.as_bytes())
        .map_err(|e| SigningKeyError::Write {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
}

/// Creates a new file and writes the public PEM.
fn write_new_file(path: &Path, contents: &[u8]) -> Result<(), SigningKeyError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    options
        .open(path)
        .and_then(|mut file| file.write_all(contents))
        .map_err(|e| SigningKeyError::Write {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
}
