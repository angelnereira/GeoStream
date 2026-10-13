//! Integration tests for the idempotency algorithm of docs/07 §6 against PostgreSQL (T-F3-01).
#![forbid(unsafe_code)]

use std::time::Duration;

use base64::Engine;
use gs_auth::crypto::CryptoRotator;
use gs_domain::ids::{ApplicationId, TenantId};
use gs_service::admin::{create_api_key, create_application, create_tenant};
use gs_service::idempotency::{
    IdempotencyContext, IdempotencyDecision, StoredResponse, begin as idem_begin, canonical_json,
    finish as idem_finish, request_fingerprint, run_idempotent,
};
use gs_store::{connect_pool, run_migrations};
use serde_json::{Value, json};
use sqlx::Row;

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

async fn setup() -> (gs_store::PgPool, TenantId, uuid::Uuid) {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");

    let tenant = create_tenant(&pool, "Idempotency Tenant")
        .await
        .expect("tenant");
    let app = create_application(&pool, &tenant.id.to_string(), "idem-app", None, None)
        .await
        .expect("application");
    let key = create_api_key(
        &pool,
        &app.id.to_string(),
        Some(vec!["tokens:mint".into()]),
        None,
        None,
    )
    .await
    .expect("api key");

    let principal_id = key.key.id.into_uuid();
    (pool, tenant.id, principal_id)
}

fn build_context<'a>(
    tenant_id: TenantId,
    principal_id: uuid::Uuid,
    key: &'a str,
    method: &'a str,
    route: &'a str,
    body: &'a Value,
) -> IdempotencyContext<'a> {
    IdempotencyContext {
        tenant_id,
        principal_id,
        key,
        method,
        route,
        body,
    }
}

fn rotator() -> CryptoRotator {
    // 32 raw bytes and the key id derived as docs/17 §2.4 documents.
    let raw = [11u8; 32];
    let digest =
        gs_auth::api_key::hash_secret(&base64::engine::general_purpose::STANDARD.encode(raw));
    let key_id = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()[..8]
        .to_string();
    CryptoRotator::empty()
        .with_active(gs_auth::crypto::DataCipher::new(key_id, &raw).expect("cipher"))
}

fn response() -> StoredResponse {
    StoredResponse {
        status: 201,
        body: json!({"session_id": "ses_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a", "invitation_code": "ABCD-EFGH-JKMN-PQRS"}),
    }
}

#[tokio::test]
async fn test_first_request_processes_and_second_replays() {
    // covers: FR-API-002, docs/07 §6 steps 3-5
    let (pool, tenant_id, principal_id) = setup().await;
    let body = json!({"mode": "ONE_TO_ONE"});
    let context = build_context(
        tenant_id,
        principal_id,
        "key-1",
        "POST",
        "/v1/sessions",
        &body,
    );
    let crypto = rotator();
    let retention = Duration::from_secs(3600);

    // First call processes.
    assert!(matches!(
        idem_begin(&pool, &context, retention, &crypto)
            .await
            .expect("begin"),
        IdempotencyDecision::Process
    ));
    idem_finish(&pool, &context, &response(), &crypto)
        .await
        .expect("finish");

    // Second call with the same body and key replays the stored response.
    match idem_begin(&pool, &context, retention, &crypto)
        .await
        .expect("begin again")
    {
        IdempotencyDecision::Replay(stored) => assert_eq!(stored, response()),
        other => panic!("expected a replay, got {other:?}"),
    }
}

#[tokio::test]
async fn test_same_key_with_another_body_is_reused() {
    // covers: FR-API-002, docs/07 §6 step 6
    let (pool, tenant_id, principal_id) = setup().await;
    let crypto = rotator();
    let retention = Duration::from_secs(3600);

    let body = json!({"mode": "ONE_TO_ONE"});
    let context = build_context(
        tenant_id,
        principal_id,
        "key-2",
        "POST",
        "/v1/sessions",
        &body,
    );
    assert!(matches!(
        idem_begin(&pool, &context, retention, &crypto)
            .await
            .expect("begin"),
        IdempotencyDecision::Process
    ));
    idem_finish(&pool, &context, &response(), &crypto)
        .await
        .expect("finish");

    let other_body = json!({"mode": "MANY_TO_MANY"});
    let context = build_context(
        tenant_id,
        principal_id,
        "key-2",
        "POST",
        "/v1/sessions",
        &other_body,
    );
    assert!(matches!(
        idem_begin(&pool, &context, retention, &crypto)
            .await
            .expect("begin with another body"),
        IdempotencyDecision::KeyReused
    ));
}

#[tokio::test]
async fn test_in_progress_key_returns_conflict() {
    // covers: FR-API-002, docs/07 §6 step 7
    let (pool, tenant_id, principal_id) = setup().await;
    let crypto = rotator();
    let retention = Duration::from_secs(3600);

    let body = json!({"mode": "ONE_TO_ONE"});
    let context = build_context(
        tenant_id,
        principal_id,
        "key-3",
        "POST",
        "/v1/sessions",
        &body,
    );
    assert!(matches!(
        idem_begin(&pool, &context, retention, &crypto)
            .await
            .expect("first begin"),
        IdempotencyDecision::Process
    ));
    // The record is still IN_PROGRESS: a second attempt conflicts.
    assert!(matches!(
        idem_begin(&pool, &context, retention, &crypto)
            .await
            .expect("second begin"),
        IdempotencyDecision::InProgress
    ));
}

#[tokio::test]
async fn test_a_5xx_discards_the_record() {
    // covers: FR-API-002, docs/07 §6 step 8
    let (pool, tenant_id, principal_id) = setup().await;
    let crypto = rotator();
    let retention = Duration::from_secs(3600);

    let body = json!({"mode": "ONE_TO_ONE"});
    let context = build_context(
        tenant_id,
        principal_id,
        "key-4",
        "POST",
        "/v1/sessions",
        &body,
    );

    let stored = run_idempotent(&pool, &context, retention, &crypto, || async {
        StoredResponse {
            status: 503,
            body: json!({"code": "TEMPORARILY_UNAVAILABLE"}),
        }
    })
    .await
    .expect("run");

    assert_eq!(stored.status, 503);
    // The client can retry: the key is free again.
    assert!(matches!(
        idem_begin(&pool, &context, retention, &crypto)
            .await
            .expect("retry"),
        IdempotencyDecision::Process
    ));
}

#[tokio::test]
async fn test_a_4xx_is_stored_and_replayed() {
    // covers: FR-API-002, docs/07 §6 step 8
    let (pool, tenant_id, principal_id) = setup().await;
    let crypto = rotator();
    let retention = Duration::from_secs(3600);

    let body = json!({"mode": "NOT_A_MODE"});
    let context = build_context(
        tenant_id,
        principal_id,
        "key-5",
        "POST",
        "/v1/sessions",
        &body,
    );

    let stored = run_idempotent(&pool, &context, retention, &crypto, || async {
        StoredResponse {
            status: 422,
            body: json!({"code": "VALIDATION_FAILED"}),
        }
    })
    .await
    .expect("run");

    assert_eq!(stored.status, 422);
    match idem_begin(&pool, &context, retention, &crypto)
        .await
        .expect("replay")
    {
        IdempotencyDecision::Replay(replayed) => assert_eq!(replayed.status, 422),
        other => panic!("expected a replay, got {other:?}"),
    }
}

#[tokio::test]
async fn test_the_stored_response_is_encrypted_at_rest() {
    // covers: FR-API-002, docs/10 §5.6, docs/07 §6
    let (pool, tenant_id, principal_id) = setup().await;
    let crypto = rotator();
    let retention = Duration::from_secs(3600);

    let body = json!({"mode": "ONE_TO_ONE"});
    let context = build_context(
        tenant_id,
        principal_id,
        "key-6",
        "POST",
        "/v1/invitations",
        &body,
    );
    let secret_response = StoredResponse {
        status: 201,
        body: json!({"invitation_code": "ABCD-EFGH-JKMN-PQRS"}),
    };
    // The record must exist before the response can be stored (docs/07 §6 step 4).
    idem_begin(&pool, &context, retention, &crypto)
        .await
        .expect("begin");
    idem_finish(&pool, &context, &secret_response, &crypto)
        .await
        .expect("finish");

    // The database only holds the envelope: no plaintext secret.
    let row = sqlx::query(
        r#"
        SELECT encode(response_body_enc, 'escape') AS envelope, response_status
        FROM idempotency_keys
        WHERE tenant_id = $1 AND principal_id = $2 AND idempotency_key = $3;
        "#,
    )
    .bind(tenant_id.into_uuid())
    .bind(principal_id)
    .bind("key-6")
    .fetch_one(&pool)
    .await
    .expect("read row");

    let envelope: String = row.get("envelope");
    assert!(
        !envelope.contains("ABCD-EFGH-JKMN-PQRS"),
        "the plaintext secret must never be stored: {envelope}"
    );
    assert!(
        envelope.contains("v1:"),
        "expected the documented envelope: {envelope}"
    );
    let status: i16 = row.get("response_status");
    assert_eq!(status, 201);

    // Round trip: the stored response decrypts back to the original.
    let decrypted = crypto.decrypt(&envelope).expect("decrypt");
    let replayed: StoredResponse = serde_json::from_slice(&decrypted).expect("deserialize");
    assert_eq!(replayed, secret_response);
}

#[tokio::test]
async fn test_expired_records_are_purged() {
    // covers: docs/07 §6 step 9
    let (pool, tenant_id, principal_id) = setup().await;
    let crypto = rotator();
    let retention = Duration::from_secs(3600);

    let body = json!({"mode": "ONE_TO_ONE"});
    let context = build_context(
        tenant_id,
        principal_id,
        "key-7",
        "POST",
        "/v1/sessions",
        &body,
    );
    idem_begin(&pool, &context, retention, &crypto)
        .await
        .expect("begin");
    idem_finish(&pool, &context, &response(), &crypto)
        .await
        .expect("finish");

    // Every purge expires the record by moving the clock forward in the database itself.
    let purged = sqlx::query(
        "UPDATE idempotency_keys SET expires_at = now() - interval '1 hour' WHERE idempotency_key = $1;",
    )
    .bind("key-7")
    .execute(&pool)
    .await
    .expect("expire rows");
    assert!(purged.rows_affected() > 0);

    let removed = gs_store::repos::IdempotencyRepository::purge_expired(
        &pool,
        time::OffsetDateTime::now_utc(),
    )
    .await
    .expect("purge");
    assert!(removed > 0);

    // The key is free again once the record has been purged.
    assert!(matches!(
        idem_begin(&pool, &context, retention, &crypto)
            .await
            .expect("begin after purge"),
        IdempotencyDecision::Process
    ));
}

#[test]
fn test_canonical_body_and_fingerprint_are_stable() {
    // covers: docs/07 §6 step 3
    let canonical = canonical_json(&json!({"b": 1, "a": {"d": 4, "c": 3}})).expect("canonical");
    assert_eq!(canonical, r#"{"a":{"c":3,"d":4},"b":1}"#);
    assert_eq!(
        request_fingerprint("POST", "/v1/sessions", &canonical),
        request_fingerprint("POST", "/v1/sessions", &canonical)
    );
    let _ = ApplicationId::new();
}
