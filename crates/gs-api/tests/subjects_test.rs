//! Integration tests for the subjects and devices REST API (T-F3-07, FR-AUTH-005, docs/07 §9.5).
#![forbid(unsafe_code)]

use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use gs_api::{SessionApiState, StreamTicketConfig, sessions_router};
use gs_service::admin::{create_api_key, create_application, create_tenant};
use gs_service::auth::DeviceTokenKeys;
use gs_service::gs_auth::crypto::{CryptoRotator, DataCipher};
use gs_service::gs_auth::{generate_signing_key, parse_private_pem};
use gs_service::migrate::{connect_pool, run_migrations};
use gs_service::tokens::mint_device_token;
use reqwest::header::{AUTHORIZATION, CACHE_CONTROL};
use serde_json::Value;

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

fn ephemeral_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.local_addr().expect("addr").port()
}

async fn spawn(app: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
    let port = ephemeral_port();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("bind");
    let address = format!("http://127.0.0.1:{port}");
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    (address, server)
}

async fn json_response(resp: reqwest::Response) -> Value {
    let text = resp.text().await.expect("text");
    serde_json::from_str(&text).expect("json")
}

struct ApiFixture {
    address: String,
    _server: tokio::task::JoinHandle<()>,
    app_key_manage: String,
    app_key_readonly: String,
    device_token_alice_1: String,
    device_id_alice_1: String,
    subject_id_alice: String,
    device_token_alice_2: String,
    device_id_alice_2: String,
    device_token_bob: String,
    device_id_bob: String,
}

async fn setup_fixture() -> ApiFixture {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");

    let tenant = create_tenant(&pool, "API Subjects Tenant")
        .await
        .expect("tenant");
    let app = create_application(
        &pool,
        &tenant.id.to_string(),
        "api-subjects-app",
        None,
        None,
    )
    .await
    .expect("application");

    let manage_key = create_api_key(
        &pool,
        &app.id.to_string(),
        Some(vec![
            "sessions:write".into(),
            "sessions:read".into(),
            "members:write".into(),
            "tokens:mint".into(),
            "subjects:manage".into(),
        ]),
        None,
        None,
    )
    .await
    .expect("manage key");

    let readonly_key = create_api_key(
        &pool,
        &app.id.to_string(),
        Some(vec!["sessions:read".into()]),
        None,
        None,
    )
    .await
    .expect("readonly key");

    let dir = std::env::temp_dir().join(format!("gs-api-sub-keys-{}", uuid::Uuid::new_v4()));
    let _ = std::fs::remove_dir_all(&dir);
    let generated = generate_signing_key("sub-api-key").expect("generate");
    generated.write_pair(&dir).expect("write");
    let signing_key = parse_private_pem(generated.private_pem()).expect("signing key");
    let keys = Arc::new(DeviceTokenKeys::load_from_dir(&dir).expect("load keys"));
    let _ = std::fs::remove_dir_all(&dir);

    // Mint Alice device 1
    let minted_alice_1 = mint_device_token(
        &pool,
        &manage_key.credential,
        &gs_service::tokens::TokenRequest {
            external_subject_id: "alice-ext".to_string(),
            installation_id: uuid::Uuid::new_v4().to_string(),
            platform: "ANDROID".into(),
            app_version: Some("1.0.0".into()),
            sdk_version: Some("0.1.0".into()),
            scopes: Some(vec![
                "sessions:create".into(),
                "stream:connect".into(),
                "sharing:write".into(),
            ]),
            ttl_seconds: Some(3600),
        },
        gs_service::tokens::TokenTtlBounds {
            min_seconds: 60,
            default_seconds: 900,
            max_seconds: 86400,
        },
        "https://geostream.example.com",
        &signing_key,
        "sub-api-key",
    )
    .await
    .expect("mint alice 1");

    // Mint Alice device 2 (same subject, different device)
    let minted_alice_2 = mint_device_token(
        &pool,
        &manage_key.credential,
        &gs_service::tokens::TokenRequest {
            external_subject_id: "alice-ext".to_string(),
            installation_id: uuid::Uuid::new_v4().to_string(),
            platform: "IOS".into(),
            app_version: Some("1.0.0".into()),
            sdk_version: Some("0.1.0".into()),
            scopes: Some(vec![
                "sessions:create".into(),
                "stream:connect".into(),
                "sharing:write".into(),
            ]),
            ttl_seconds: Some(3600),
        },
        gs_service::tokens::TokenTtlBounds {
            min_seconds: 60,
            default_seconds: 900,
            max_seconds: 86400,
        },
        "https://geostream.example.com",
        &signing_key,
        "sub-api-key",
    )
    .await
    .expect("mint alice 2");

    // Mint Bob device
    let minted_bob = mint_device_token(
        &pool,
        &manage_key.credential,
        &gs_service::tokens::TokenRequest {
            external_subject_id: "bob-ext".to_string(),
            installation_id: uuid::Uuid::new_v4().to_string(),
            platform: "WEB".into(),
            app_version: None,
            sdk_version: None,
            scopes: Some(vec!["stream:connect".into()]),
            ttl_seconds: Some(3600),
        },
        gs_service::tokens::TokenTtlBounds {
            min_seconds: 60,
            default_seconds: 900,
            max_seconds: 86400,
        },
        "https://geostream.example.com",
        &signing_key,
        "sub-api-key",
    )
    .await
    .expect("mint bob");

    let raw = [42u8; 32];
    let rotator = CryptoRotator::empty()
        .with_active(DataCipher::new("test_key_subjects", &raw).expect("cipher"));

    let state = SessionApiState {
        pool: pool.clone(),
        signing_keys: keys,
        issuer: "https://geostream.example.com".to_string(),
        leeway_seconds: 60,
        crypto_rotator: Some(Arc::new(rotator)),
        idempotency_retention: Duration::from_secs(86_400),
        ephemeral: gs_service::InMemoryEphemeralStore::new(),
        bus: gs_service::InMemoryBus::new(),
        redis_key_prefix: "gs:test".into(),
        stream_ticket: StreamTicketConfig::default(),
        rate_limit_config: gs_api::RateLimitConfig::default(),
    };

    let (address, server) = spawn(sessions_router(state)).await;

    ApiFixture {
        address,
        _server: server,
        app_key_manage: manage_key.credential.to_string(),
        app_key_readonly: readonly_key.credential.to_string(),
        device_token_alice_1: minted_alice_1.response.access_token,
        device_id_alice_1: minted_alice_1.device.id.to_string(),
        subject_id_alice: minted_alice_1.subject.id.to_string(),
        device_token_alice_2: minted_alice_2.response.access_token,
        device_id_alice_2: minted_alice_2.device.id.to_string(),
        device_token_bob: minted_bob.response.access_token,
        device_id_bob: minted_bob.device.id.to_string(),
    }
}

#[tokio::test]
async fn test_api_find_subjects_and_auth_matrix() {
    let fix = setup_fixture().await;
    let client = reqwest::Client::new();

    // 1. Success with subjects:manage scope
    let res = client
        .get(format!(
            "{}/v1/subjects?external_subject_id=alice-ext",
            fix.address
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_manage))
        .send()
        .await
        .expect("send");

    assert_eq!(res.status(), 200);
    assert_eq!(res.headers().get(CACHE_CONTROL).unwrap(), "no-store");
    assert!(res.headers().get("X-Request-Id").is_some());

    let body: Value = json_response(res).await;
    let data = body["data"].as_array().expect("data array");
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["id"], fix.subject_id_alice);
    assert_eq!(data[0]["external_subject_id"], "alice-ext");
    assert_eq!(data[0]["status"], "ACTIVE");
    assert_eq!(data[0]["token_epoch"], 0);
    assert!(body["next_cursor"].is_null());

    // 2. Query non-existent subject -> empty list
    let res_none = client
        .get(format!(
            "{}/v1/subjects?external_subject_id=nobody",
            fix.address
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_manage))
        .send()
        .await
        .expect("send");
    assert_eq!(res_none.status(), 200);
    let body_none: Value = json_response(res_none).await;
    assert!(body_none["data"].as_array().unwrap().is_empty());

    // 3. Missing query parameter -> 422 VALIDATION_FAILED
    let res_missing = client
        .get(format!("{}/v1/subjects", fix.address))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_manage))
        .send()
        .await
        .expect("send");
    assert_eq!(res_missing.status(), 422);

    // 4. Unknown query parameter -> 422 VALIDATION_FAILED
    let res_unknown = client
        .get(format!(
            "{}/v1/subjects?external_subject_id=alice-ext&foo=bar",
            fix.address
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_manage))
        .send()
        .await
        .expect("send");
    assert_eq!(res_unknown.status(), 422);

    // 5. Invalid external_subject_id format -> 422 VALIDATION_FAILED
    let res_invalid = client
        .get(format!(
            "{}/v1/subjects?external_subject_id=user%40example",
            fix.address
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_manage))
        .send()
        .await
        .expect("send");
    assert_eq!(res_invalid.status(), 422);

    // 6. Device caller -> 403 PERMISSION_DENIED
    let res_device = client
        .get(format!(
            "{}/v1/subjects?external_subject_id=alice-ext",
            fix.address
        ))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fix.device_token_alice_1),
        )
        .send()
        .await
        .expect("send");
    assert_eq!(res_device.status(), 403);
    let body_dev: Value = json_response(res_device).await;
    assert_eq!(body_dev["code"], "PERMISSION_DENIED");

    // 7. Application key without subjects:manage -> 403 INSUFFICIENT_SCOPE
    let res_no_scope = client
        .get(format!(
            "{}/v1/subjects?external_subject_id=alice-ext",
            fix.address
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_readonly))
        .send()
        .await
        .expect("send");
    assert_eq!(res_no_scope.status(), 403);
    let body_no_scope: Value = json_response(res_no_scope).await;
    assert_eq!(body_no_scope["code"], "INSUFFICIENT_SCOPE");
}

#[tokio::test]
async fn test_api_get_subject_and_revoke_tokens() {
    let fix = setup_fixture().await;
    let client = reqwest::Client::new();

    // 1. GET /v1/subjects/{subject_id} - success
    let res = client
        .get(format!(
            "{}/v1/subjects/{}",
            fix.address, fix.subject_id_alice
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_manage))
        .send()
        .await
        .expect("send");
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers().get(CACHE_CONTROL).unwrap(), "no-store");
    let body: Value = json_response(res).await;
    assert_eq!(body["id"], fix.subject_id_alice);
    assert_eq!(body["external_subject_id"], "alice-ext");
    assert_eq!(body["token_epoch"], 0);

    // 2. GET unknown subject -> 404 NOT_FOUND
    let random_sub = gs_service::gs_domain::ids::SubjectId::new().to_string();
    let res_404 = client
        .get(format!("{}/v1/subjects/{}", fix.address, random_sub))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_manage))
        .send()
        .await
        .expect("send");
    assert_eq!(res_404.status(), 404);

    // 3. GET malformed subject id -> 404 NOT_FOUND
    let res_malformed = client
        .get(format!("{}/v1/subjects/not-a-valid-id", fix.address))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_manage))
        .send()
        .await
        .expect("send");
    assert_eq!(res_malformed.status(), 404);

    // 4. Device token verifies beforehand
    let res_dev_before = client
        .get(format!(
            "{}/v1/devices/{}",
            fix.address, fix.device_id_alice_1
        ))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fix.device_token_alice_1),
        )
        .send()
        .await
        .expect("send");
    assert_eq!(res_dev_before.status(), 200);

    // 5. POST /v1/subjects/{subject_id}/revoke-tokens
    let res_revoke = client
        .post(format!(
            "{}/v1/subjects/{}/revoke-tokens",
            fix.address, fix.subject_id_alice
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_manage))
        .send()
        .await
        .expect("send");
    assert_eq!(res_revoke.status(), 200);
    let body_revoked: Value = json_response(res_revoke).await;
    assert_eq!(body_revoked["token_epoch"], 1);

    // 6. Device token alice 1 and alice 2 now fail with 401 UNAUTHENTICATED
    let res_dev_after = client
        .get(format!(
            "{}/v1/devices/{}",
            fix.address, fix.device_id_alice_1
        ))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fix.device_token_alice_1),
        )
        .send()
        .await
        .expect("send");
    assert_eq!(res_dev_after.status(), 401);

    let res_dev_after2 = client
        .get(format!(
            "{}/v1/devices/{}",
            fix.address, fix.device_id_alice_2
        ))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fix.device_token_alice_2),
        )
        .send()
        .await
        .expect("send");
    assert_eq!(res_dev_after2.status(), 401);

    // Bob's token (different subject) is unaffected
    let res_bob = client
        .get(format!("{}/v1/devices/{}", fix.address, fix.device_id_bob))
        .header(AUTHORIZATION, format!("Bearer {}", fix.device_token_bob))
        .send()
        .await
        .expect("send");
    assert_eq!(res_bob.status(), 200);
}

#[tokio::test]
async fn test_api_get_device_and_deactivate() {
    let fix = setup_fixture().await;
    let client = reqwest::Client::new();

    // 1. GET /v1/devices/{device_id} via application
    let res_app = client
        .get(format!(
            "{}/v1/devices/{}",
            fix.address, fix.device_id_alice_1
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_manage))
        .send()
        .await
        .expect("send");
    assert_eq!(res_app.status(), 200);
    assert_eq!(res_app.headers().get(CACHE_CONTROL).unwrap(), "no-store");
    let body: Value = json_response(res_app).await;
    assert_eq!(body["id"], fix.device_id_alice_1);
    assert_eq!(body["subject_id"], fix.subject_id_alice);
    assert_eq!(body["status"], "ACTIVE");
    assert_eq!(body["platform"], "ANDROID");

    // 2. GET own device via device caller
    let res_own = client
        .get(format!(
            "{}/v1/devices/{}",
            fix.address, fix.device_id_alice_1
        ))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fix.device_token_alice_1),
        )
        .send()
        .await
        .expect("send");
    assert_eq!(res_own.status(), 200);

    // 3. GET device of same subject via device caller (own_subject_device)
    let res_same_sub = client
        .get(format!(
            "{}/v1/devices/{}",
            fix.address, fix.device_id_alice_2
        ))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fix.device_token_alice_1),
        )
        .send()
        .await
        .expect("send");
    assert_eq!(res_same_sub.status(), 200);

    // 4. GET device of different subject via device caller -> 404 NOT_FOUND (FR-API-006)
    let res_diff_sub = client
        .get(format!("{}/v1/devices/{}", fix.address, fix.device_id_bob))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fix.device_token_alice_1),
        )
        .send()
        .await
        .expect("send");
    assert_eq!(res_diff_sub.status(), 404);

    // 5. DELETE device of another subject -> 404 NOT_FOUND
    let res_del_other = client
        .delete(format!("{}/v1/devices/{}", fix.address, fix.device_id_bob))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fix.device_token_alice_1),
        )
        .send()
        .await
        .expect("send");
    assert_eq!(res_del_other.status(), 404);

    // 6. DELETE device of same subject but not own device (own_device requirement) -> 404 NOT_FOUND
    let res_del_not_own = client
        .delete(format!(
            "{}/v1/devices/{}",
            fix.address, fix.device_id_alice_2
        ))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fix.device_token_alice_1),
        )
        .send()
        .await
        .expect("send");
    assert_eq!(res_del_not_own.status(), 404);

    // 7. DELETE own device -> 204 NO_CONTENT
    let res_del_own = client
        .delete(format!(
            "{}/v1/devices/{}",
            fix.address, fix.device_id_alice_1
        ))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fix.device_token_alice_1),
        )
        .send()
        .await
        .expect("send");
    assert_eq!(res_del_own.status(), 204);
    assert_eq!(
        res_del_own.headers().get(CACHE_CONTROL).unwrap(),
        "no-store"
    );

    // 8. Idempotent DELETE -> 204 NO_CONTENT (using app key or device)
    let res_del_idem = client
        .delete(format!(
            "{}/v1/devices/{}",
            fix.address, fix.device_id_alice_1
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_manage))
        .send()
        .await
        .expect("send");
    assert_eq!(res_del_idem.status(), 204);

    // 9. Deactivated device token now fails with 401 UNAUTHENTICATED
    let res_deact_token = client
        .get(format!(
            "{}/v1/devices/{}",
            fix.address, fix.device_id_alice_1
        ))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fix.device_token_alice_1),
        )
        .send()
        .await
        .expect("send");
    assert_eq!(res_deact_token.status(), 401);

    // 10. Device status is INACTIVE when viewed by application
    let res_after = client
        .get(format!(
            "{}/v1/devices/{}",
            fix.address, fix.device_id_alice_1
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_manage))
        .send()
        .await
        .expect("send");
    assert_eq!(res_after.status(), 200);
    let body_after: Value = json_response(res_after).await;
    assert_eq!(body_after["status"], "INACTIVE");
}
