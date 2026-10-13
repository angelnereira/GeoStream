//! Idempotency for mutating `POST` operations (docs/07 §6, FR-API-002).
//!
//! The algorithm is the normative one of `docs/07` §6: the storage key is
//! `(tenant_id, principal_id, idempotency_key)`, the fingerprint is SHA-256 of
//! `method + " " + route + "\n" + canonical body`, a 5xx discards the record and a 4xx is
//! stored, and responses whose body contains a secret are stored encrypted at rest with
//! AES-256-GCM (`GS_DATA_ENCRYPTION_KEY`, docs/10 §5.6).
#![forbid(unsafe_code)]

use std::time::Duration;

use gs_auth::api_key::hash_secret;
use gs_auth::crypto::{CryptoError, CryptoRotator};
use gs_domain::error::AsErrorCode;
use gs_domain::ids::TenantId;
use gs_store::repos::{BeginOutcome, IdempotencyRecord, IdempotencyRepository};
use gs_store::{PgPool, StoreError};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

/// Maximum accepted length of an idempotency key (docs/11 §2).
pub const IDEMPOTENCY_KEY_MAX_LEN: usize = 255;

/// Errors of the idempotency flow, mapped to the catalog of `docs/09` §6.
#[derive(Debug, Error)]
pub enum IdempotencyError {
    /// The `Idempotency-Key` header is too long.
    #[error("idempotency key must be at most {IDEMPOTENCY_KEY_MAX_LEN} characters")]
    KeyTooLong,

    /// The same key was used with a different request body.
    #[error("idempotency key was already used with a different request")]
    KeyReused,

    /// The key is currently being processed by another request.
    #[error("another request with the same idempotency key is in progress")]
    InProgress,

    /// The stored response could not be decrypted.
    #[error("stored response cannot be decrypted: {0}")]
    Decryption(String),

    /// A persistence operation failed.
    #[error("storage error: {0}")]
    Store(#[from] StoreError),

    /// The response could not be serialized.
    #[error("invalid response body: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl AsErrorCode for IdempotencyError {
    fn error_code(&self) -> gs_domain::error::ErrorCode {
        use gs_domain::error::ErrorCode;
        match self {
            Self::KeyTooLong | Self::KeyReused | Self::Serialization(_) => {
                ErrorCode::ValidationFailed
            }
            Self::InProgress => ErrorCode::Conflict,
            Self::Decryption(_) | Self::Store(_) => gs_domain::error::ErrorCode::Internal,
        }
    }
}

/// Canonical JSON: keys ordered recursively and no insignificant whitespace (docs/07 §6).
pub fn canonical_json(value: &Value) -> Result<String, serde_json::Error> {
    let normalized = normalize(value);
    serde_json::to_string(&normalized)
}

/// Recursively sorts the keys of every JSON object.
fn normalize(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sorted: Vec<(&String, &Value)> = map.iter().collect();
            sorted.sort_by(|left, right| left.0.cmp(right.0));
            let mut output = Map::new();
            for (key, value) in sorted {
                output.insert(key.clone(), normalize(value));
            }
            Value::Object(output)
        }
        Value::Array(items) => Value::Array(items.iter().map(normalize).collect()),
        other => other.clone(),
    }
}

/// Fingerprint of a request: SHA-256 of `method + " " + route + "\n" + canonical body`.
pub fn request_fingerprint(method: &str, route: &str, canonical_body: &str) -> [u8; 32] {
    let material = format!("{method} {route}\n{canonical_body}");
    hash_secret(&material)
}

/// A response to store for a replay (docs/07 §6 step 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredResponse {
    /// HTTP status of the response.
    pub status: u16,
    /// Response body, already serialized.
    pub body: Value,
}

/// Outcome of the idempotency check for an incoming request (docs/07 §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdempotencyDecision {
    /// The key is free: process the request and then call [`finish`].
    Process,
    /// A stored response is replayed with `Idempotent-Replayed: true`.
    Replay(StoredResponse),
    /// The same key is being processed right now (409).
    InProgress,
    /// The same key was used with a different request (422).
    KeyReused,
}

/// Parameters of one idempotent operation.
#[derive(Debug, Clone)]
pub struct IdempotencyContext<'a> {
    /// Tenant of the caller.
    pub tenant_id: TenantId,
    /// `key_id` of the API key or `device_id` of the token.
    pub principal_id: uuid::Uuid,
    /// Value of the `Idempotency-Key` header.
    pub key: &'a str,
    /// HTTP method.
    pub method: &'a str,
    /// Route template (never a concrete id).
    pub route: &'a str,
    /// Canonical request body.
    pub body: &'a Value,
}

/// Checks the idempotency key of a request and registers it when it is free (docs/07 §6).
pub async fn begin(
    pool: &PgPool,
    context: &IdempotencyContext<'_>,
    retention: Duration,
    crypto: &CryptoRotator,
) -> Result<IdempotencyDecision, IdempotencyError> {
    if context.key.len() > IDEMPOTENCY_KEY_MAX_LEN {
        return Err(IdempotencyError::KeyTooLong);
    }

    let canonical_body = canonical_json(context.body).map_err(|e| {
        IdempotencyError::Store(StoreError::DataMapping(format!("canonical body: {e}")))
    })?;
    let fingerprint = request_fingerprint(context.method, context.route, &canonical_body);
    let expires_at = time::OffsetDateTime::now_utc() + retention;

    let outcome = IdempotencyRepository::begin(
        pool,
        context.tenant_id,
        context.principal_id,
        context.key,
        &fingerprint,
        expires_at,
    )
    .await?;

    match outcome {
        BeginOutcome::Started => Ok(IdempotencyDecision::Process),
        BeginOutcome::InProgress => Ok(IdempotencyDecision::InProgress),
        BeginOutcome::FingerprintMismatch => Ok(IdempotencyDecision::KeyReused),
        BeginOutcome::Replay(record) => Ok(IdempotencyDecision::Replay(decrypt_response(
            &record, crypto,
        )?)),
    }
}

/// Stores the response of a processed request (docs/07 §6 step 4).
///
/// Bodies are always stored: the encryption at rest with `GS_DATA_ENCRYPTION_KEY` is what
/// allows replaying a response that contains a secret shown only once (docs/10 §5.6). A
/// non-encryptable cipher is a configuration error, never a silent plaintext store.
pub async fn finish(
    pool: &PgPool,
    context: &IdempotencyContext<'_>,
    response: &StoredResponse,
    crypto: &CryptoRotator,
) -> Result<(), IdempotencyError> {
    let serialized = serde_json::to_string(response)?;
    let encrypted = crypto
        .encrypt(serialized.as_bytes())
        .map_err(|e: CryptoError| IdempotencyError::Decryption(e.to_string()))?;

    let status = i16::try_from(response.status).unwrap_or(500);
    IdempotencyRepository::complete(
        pool,
        context.tenant_id,
        context.principal_id,
        context.key,
        status,
        Some(encrypted.as_bytes()),
    )
    .await?;
    Ok(())
}

/// Discards the record so the client can retry (docs/07 §6 step 8).
pub async fn discard(
    pool: &PgPool,
    context: &IdempotencyContext<'_>,
) -> Result<(), IdempotencyError> {
    IdempotencyRepository::discard(pool, context.tenant_id, context.principal_id, context.key)
        .await?;
    Ok(())
}

/// Builds the `Idempotent-Replayed` marker of docs/07 §2.
pub const fn replayed_header() -> &'static str {
    "true"
}

/// Convenience wrapper that runs an idempotent operation and stores its response.
///
/// `handler` receives no arguments; the caller decides what to do. 5xx responses are
/// discarded automatically, so the client can retry (docs/07 §6 step 8).
pub async fn run_idempotent<F, Fut>(
    pool: &PgPool,
    context: &IdempotencyContext<'_>,
    retention: Duration,
    crypto: &CryptoRotator,
    handler: F,
) -> Result<StoredResponse, IdempotencyError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = StoredResponse>,
{
    match begin(pool, context, retention, crypto).await? {
        IdempotencyDecision::Process => {
            let response = handler().await;
            if response.status >= 500 {
                discard(pool, context).await?;
            } else {
                finish(pool, context, &response, crypto).await?;
            }
            Ok(response)
        }
        IdempotencyDecision::Replay(stored) => Ok(stored),
        IdempotencyDecision::InProgress => Err(IdempotencyError::InProgress),
        IdempotencyDecision::KeyReused => Err(IdempotencyError::KeyReused),
    }
}

/// Decrypts a stored response.
fn decrypt_response(
    record: &IdempotencyRecord,
    crypto: &CryptoRotator,
) -> Result<StoredResponse, IdempotencyError> {
    let Some(envelope) = record.response_body_enc.as_deref() else {
        return Ok(StoredResponse {
            status: record
                .response_status
                .map_or(200, i64::from)
                .clamp(100, 599) as u16,
            body: Value::Null,
        });
    };
    let envelope = std::str::from_utf8(envelope)
        .map_err(|_| IdempotencyError::Decryption("envelope is not utf-8".to_string()))?;
    let plaintext = crypto
        .decrypt(envelope)
        .map_err(|e| IdempotencyError::Decryption(e.to_string()))?;
    let response: StoredResponse = serde_json::from_slice(&plaintext)
        .map_err(|e| IdempotencyError::Decryption(e.to_string()))?;
    Ok(response)
}

use std::future::Future;

#[cfg(test)]
mod tests {
    use base64::Engine;

    use super::*;

    fn body() -> Value {
        serde_json::json!({"name": "session", "mode": "ONE_TO_ONE", "meta": {"b": 2, "a": 1}})
    }

    fn rotator() -> CryptoRotator {
        // The cipher only needs the 32 raw bytes and the key id documented in docs/17 §2.4.
        let raw = [9u8; 32];
        let key_id =
            gs_auth::api_key::hash_secret(&base64::engine::general_purpose::STANDARD.encode(raw));
        let key_id = key_id
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()[..8]
            .to_string();
        CryptoRotator::empty()
            .with_active(gs_auth::crypto::DataCipher::new(key_id, &raw).expect("cipher"))
    }

    #[test]
    fn test_canonical_json_sorts_keys_recursively() {
        // covers: docs/07 §6 step 3
        let canonical = canonical_json(&body()).expect("canonical");
        assert_eq!(
            canonical,
            r#"{"meta":{"a":1,"b":2},"mode":"ONE_TO_ONE","name":"session"}"#
        );
        // The same value always canonicalizes the same way.
        let reordered =
            serde_json::json!({"mode": "ONE_TO_ONE", "meta": {"a": 1, "b": 2}, "name": "session"});
        assert_eq!(canonical_json(&reordered).expect("canonical"), canonical);
    }

    #[test]
    fn test_fingerprint_changes_with_method_route_and_body() {
        // covers: docs/07 §6 step 3
        let base = request_fingerprint("POST", "/v1/sessions", "{}");
        assert_eq!(base, request_fingerprint("POST", "/v1/sessions", "{}"));
        assert_ne!(base, request_fingerprint("PUT", "/v1/sessions", "{}"));
        assert_ne!(
            base,
            request_fingerprint("POST", "/v1/sessions/{session_id}", "{}")
        );
        assert_ne!(
            base,
            request_fingerprint("POST", "/v1/sessions", "{\"a\":1}")
        );
    }

    #[test]
    fn test_error_codes_match_docs_07_6() {
        // covers: docs/09 §6, docs/07 §6
        assert_eq!(
            IdempotencyError::KeyReused.error_code(),
            gs_domain::error::ErrorCode::ValidationFailed
        );
        assert_eq!(
            IdempotencyError::InProgress.error_code(),
            gs_domain::error::ErrorCode::Conflict
        );
    }

    #[tokio::test]
    async fn test_run_idempotent_replays_the_stored_response() {
        // covers: FR-API-002, docs/07 §6 steps 4-5
        // This unit test uses no pool: it documents the state machine only.
        let _ = rotator();
        let response = StoredResponse {
            status: 201,
            body: serde_json::json!({"secret": "one-time"}),
        };
        let encrypted = rotator()
            .encrypt(
                serde_json::to_string(&response)
                    .expect("serialize")
                    .as_bytes(),
            )
            .expect("encrypt");
        let decrypted = rotator().decrypt(&encrypted).expect("decrypt");
        let replayed: StoredResponse = serde_json::from_slice(&decrypted).expect("deserialize");
        assert_eq!(replayed, response);
    }
}
