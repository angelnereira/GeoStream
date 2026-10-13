//! Integration tests for the invitations REST API (T-F3-06, FR-INV-001..003, docs/07 §9.4).
#![forbid(unsafe_code)]

use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use gs_api::{SessionApiState, sessions_router};
use gs_service::admin::{create_api_key, create_application, create_tenant};
use gs_service::auth::DeviceTokenKeys;
use gs_service::gs_auth::crypto::{CryptoRotator, DataCipher};
use gs_service::gs_auth::{generate_signing_key, parse_private_pem};
use gs_service::migrate::{connect_pool, run_migrations};
use gs_service::tokens::{TokenTtlBounds, mint_device_token};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, ETAG};
use serde_json::{Value, json};

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

struct ApiFixture {
    address: String,
    _server: tokio::task::JoinHandle<()>,
    app_key_write: String,
    app_key_readonly: String,
    device_token_owner: String,
    device_token_member: String,
    device_token_other: String,
}

async fn setup_fixture() -> ApiFixture {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");

    let tenant = create_tenant(&pool, "API Invitations Tenant")
        .await
        .expect("tenant");
    let app = create_application(
        &pool,
        &tenant.id.to_string(),
        "api-invitations-app",
        None,
        None,
    )
    .await
    .expect("application");

    let write_key = create_api_key(
        &pool,
        &app.id.to_string(),
        Some(vec![
            "sessions:write".into(),
            "sessions:read".into(),
            "members:write".into(),
            "invitations:write".into(),
            "tokens:mint".into(),
        ]),
        None,
        None,
    )
    .await
    .expect("write key");

    let read_key = create_api_key(
        &pool,
        &app.id.to_string(),
        Some(vec!["sessions:read".into()]),
        None,
        None,
    )
    .await
    .expect("read key");

    let generated = generate_signing_key("api-inv-test").expect("generate key");
    let signing = parse_private_pem(generated.private_pem()).expect("parse key");
    let kid = "api-inv-test";

    let mut keyset = DeviceTokenKeys::default();
    keyset.insert_signing_key(kid, signing.clone());

    let bounds = TokenTtlBounds {
        min_seconds: 300,
        default_seconds: 900,
        max_seconds: 3600,
    };

    // Mint owner token (with invitations:redeem scope)
    let token_owner = mint_device_token(
        &pool,
        &write_key.credential,
        &serde_json::from_value(json!({
            "external_subject_id": "api-owner",
            "installation_id": "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0001",
            "platform": "ANDROID",
            "scopes": ["invitations:redeem"]
        }))
        .unwrap(),
        bounds,
        "https://geostream.example.com",
        &signing,
        kid,
    )
    .await
    .expect("mint owner");

    // Mint member token (with invitations:redeem scope)
    let token_member = mint_device_token(
        &pool,
        &write_key.credential,
        &serde_json::from_value(json!({
            "external_subject_id": "api-member",
            "installation_id": "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0002",
            "platform": "IOS",
            "scopes": ["invitations:redeem"]
        }))
        .unwrap(),
        bounds,
        "https://geostream.example.com",
        &signing,
        kid,
    )
    .await
    .expect("mint member");

    // Mint another device token (with invitations:redeem scope)
    let token_other = mint_device_token(
        &pool,
        &write_key.credential,
        &serde_json::from_value(json!({
            "external_subject_id": "api-other",
            "installation_id": "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0003",
            "platform": "WEB",
            "scopes": ["invitations:redeem"]
        }))
        .unwrap(),
        bounds,
        "https://geostream.example.com",
        &signing,
        kid,
    )
    .await
    .expect("mint other");

    let raw = [42u8; 32];
    let rotator = CryptoRotator::empty()
        .with_active(DataCipher::new("test_key_invitations", &raw).expect("cipher"));

    let state = SessionApiState {
        pool: pool.clone(),
        signing_keys: Arc::new(keyset),
        issuer: "https://geostream.example.com".to_string(),
        leeway_seconds: 60,
        crypto_rotator: Some(Arc::new(rotator)),
        idempotency_retention: Duration::from_secs(86_400),
        ephemeral: gs_service::InMemoryEphemeralStore::new(),
        bus: gs_service::InMemoryBus::new(),
        redis_key_prefix: "gs:test".into(),
        rate_limit_config: gs_api::RateLimitConfig::default(),
    };

    let (address, server) = spawn(sessions_router(state)).await;

    ApiFixture {
        address,
        _server: server,
        app_key_write: write_key.credential.to_string(),
        app_key_readonly: read_key.credential.to_string(),
        device_token_owner: token_owner.response.access_token,
        device_token_member: token_member.response.access_token,
        device_token_other: token_other.response.access_token,
    }
}

async fn json_response(resp: reqwest::Response) -> Value {
    let text = resp.text().await.expect("text");
    serde_json::from_str(&text).expect("json")
}

#[tokio::test]
async fn test_api_create_list_and_revoke_invitation() {
    let fix = setup_fixture().await;
    let client = reqwest::Client::new();

    // 1. Create a session using app key
    let create_sess_resp = client
        .post(format!("{}/v1/sessions", fix.address))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "mode": "MANY_TO_MANY",
                "expires_in_seconds": 3600
            })
            .to_string(),
        )
        .send()
        .await
        .expect("send create session");
    assert_eq!(create_sess_resp.status(), 201);
    let session_body: Value = json_response(create_sess_resp).await;
    let session_id = session_body["id"].as_str().unwrap();

    // 2. Readonly key cannot create invitation -> 403 InsufficientScope
    let denied_create = client
        .post(format!(
            "{}/v1/sessions/{session_id}/invitations",
            fix.address
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_readonly))
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "max_uses": 2,
                "expires_in_seconds": 300
            })
            .to_string(),
        )
        .send()
        .await
        .expect("send");
    assert_eq!(denied_create.status(), 403);

    // 3. Create invitation with Idempotency-Key
    let idemp_key = "idemp-inv-1";
    let create_inv_resp = client
        .post(format!(
            "{}/v1/sessions/{session_id}/invitations",
            fix.address
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .header("Idempotency-Key", idemp_key)
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "max_uses": 2,
                "expires_in_seconds": 600,
                "grants": {
                    "publish": true,
                    "read": true,
                    "invite": false,
                    "manage": false
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("send create inv");
    assert_eq!(create_inv_resp.status(), 201);
    let inv_body: Value = json_response(create_inv_resp).await;
    let invitation_id = inv_body["id"].as_str().unwrap();
    let invitation_code = inv_body["code"].as_str().unwrap();
    assert!(!invitation_code.is_empty());

    // 4. Replay with same idempotency key -> 201 Created with idempotent-replayed
    let replay_resp = client
        .post(format!(
            "{}/v1/sessions/{session_id}/invitations",
            fix.address
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .header("Idempotency-Key", idemp_key)
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "max_uses": 2,
                "expires_in_seconds": 600,
                "grants": {
                    "publish": true,
                    "read": true,
                    "invite": false,
                    "manage": false
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("send replay");
    assert_eq!(replay_resp.status(), 201);
    assert_eq!(
        replay_resp
            .headers()
            .get("idempotent-replayed")
            .and_then(|h| h.to_str().ok()),
        Some("true")
    );

    // 5. List invitations -> 200 OK
    let list_resp = client
        .get(format!(
            "{}/v1/sessions/{session_id}/invitations",
            fix.address
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .send()
        .await
        .expect("send list");
    assert_eq!(list_resp.status(), 200);
    let list_body: Value = json_response(list_resp).await;
    assert_eq!(list_body["data"].as_array().unwrap().len(), 1);

    // 6. Revoke invitation -> 204 No Content
    let revoke_resp = client
        .delete(format!(
            "{}/v1/sessions/{session_id}/invitations/{invitation_id}",
            fix.address
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .send()
        .await
        .expect("send revoke");
    assert_eq!(revoke_resp.status(), 204);

    // Revocation is idempotent -> 204 No Content
    let revoke_again = client
        .delete(format!(
            "{}/v1/sessions/{session_id}/invitations/{invitation_id}",
            fix.address
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .send()
        .await
        .expect("send revoke again");
    assert_eq!(revoke_again.status(), 204);

    // 7. List invitations after revocation -> status is REVOKED
    let list_after = client
        .get(format!(
            "{}/v1/sessions/{session_id}/invitations",
            fix.address
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .send()
        .await
        .expect("send list after");
    assert_eq!(list_after.status(), 200);
    let list_after_body: Value = json_response(list_after).await;
    assert_eq!(
        list_after_body["data"][0]["status"].as_str().unwrap(),
        "REVOKED"
    );
}

#[tokio::test]
async fn test_api_redeem_invitation_flow() {
    let fix = setup_fixture().await;
    let client = reqwest::Client::new();

    // 1. Create session
    let create_sess_resp = client
        .post(format!("{}/v1/sessions", fix.address))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "mode": "MANY_TO_MANY",
                "expires_in_seconds": 3600
            })
            .to_string(),
        )
        .send()
        .await
        .expect("send create session");
    assert_eq!(create_sess_resp.status(), 201);
    let session_body: Value = json_response(create_sess_resp).await;
    let session_id = session_body["id"].as_str().unwrap();

    // 2. Create an active invitation
    let create_inv_resp = client
        .post(format!(
            "{}/v1/sessions/{session_id}/invitations",
            fix.address
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "max_uses": 2,
                "expires_in_seconds": 600
            })
            .to_string(),
        )
        .send()
        .await
        .expect("send create inv");
    assert_eq!(create_inv_resp.status(), 201);
    let inv_body: Value = json_response(create_inv_resp).await;
    let code = inv_body["code"].as_str().unwrap();

    // 3. Application cannot redeem -> 403 Forbidden
    let app_redeem = client
        .post(format!("{}/v1/invitations/redeem", fix.address))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "code": code }).to_string())
        .send()
        .await
        .expect("send app redeem");
    assert_eq!(app_redeem.status(), 403);

    // 4. Invalid formatted code -> 404 INVITATION_INVALID (docs/09 §5)
    let invalid_code_resp = client
        .post(format!("{}/v1/invitations/redeem", fix.address))
        .header(AUTHORIZATION, format!("Bearer {}", fix.device_token_owner))
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "code": "NOT-A-VALID-CODE-123456789" }).to_string())
        .send()
        .await
        .expect("send invalid code");
    assert_eq!(invalid_code_resp.status(), 404);
    let err_body: Value = json_response(invalid_code_resp).await;
    assert_eq!(err_body["code"].as_str().unwrap(), "INVITATION_INVALID");

    // 5. Successful redeem by device_token_owner
    let redeem_resp = client
        .post(format!("{}/v1/invitations/redeem", fix.address))
        .header(AUTHORIZATION, format!("Bearer {}", fix.device_token_owner))
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "code": code }).to_string())
        .send()
        .await
        .expect("send valid redeem");
    assert_eq!(redeem_resp.status(), 201);
    assert!(redeem_resp.headers().contains_key(ETAG));
    let redeem_body: Value = json_response(redeem_resp).await;
    assert_eq!(
        redeem_body["member"]["session_id"].as_str().unwrap(),
        session_id
    );
    assert_eq!(
        redeem_body["member"]["joined_via"].as_str().unwrap(),
        "INVITATION"
    );

    // 6. Re-redeem by same device/subject -> 409 CONFLICT
    let reredeem_resp = client
        .post(format!("{}/v1/invitations/redeem", fix.address))
        .header(AUTHORIZATION, format!("Bearer {}", fix.device_token_owner))
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "code": code }).to_string())
        .send()
        .await
        .expect("send reredeem");
    assert_eq!(reredeem_resp.status(), 409);
    let conflict_body: Value = json_response(reredeem_resp).await;
    assert_eq!(conflict_body["code"].as_str().unwrap(), "CONFLICT");

    // 7. Second device redeems -> 201 (since max_uses was 2)
    let redeem2_resp = client
        .post(format!("{}/v1/invitations/redeem", fix.address))
        .header(AUTHORIZATION, format!("Bearer {}", fix.device_token_member))
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "code": code }).to_string())
        .send()
        .await
        .expect("send redeem 2");
    assert_eq!(redeem2_resp.status(), 201);

    // 8. Third device redeems -> 404 INVITATION_INVALID (exhausted)
    let redeem3_resp = client
        .post(format!("{}/v1/invitations/redeem", fix.address))
        .header(AUTHORIZATION, format!("Bearer {}", fix.device_token_other))
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "code": code }).to_string())
        .send()
        .await
        .expect("send redeem 3");
    assert_eq!(redeem3_resp.status(), 404);
}
