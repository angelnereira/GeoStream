//! Integration tests for the session REST control plane (T-F3-03, FR-SES-001..006, docs/07 §9.2).
#![forbid(unsafe_code)]

use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use gs_api::{REQUEST_ID_HEADER, SessionApiState, sessions_router};
use gs_service::admin::{create_api_key, create_application, create_tenant};
use gs_service::auth::DeviceTokenKeys;
use gs_service::gs_auth::crypto::{CryptoRotator, DataCipher};
use gs_service::gs_auth::{generate_signing_key, parse_private_pem};
use gs_service::migrate::{connect_pool, run_migrations};
use gs_service::tokens::{TokenTtlBounds, mint_device_token};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, ETAG, IF_MATCH};
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

struct TestFixture {
    address: String,
    _server: tokio::task::JoinHandle<()>,
    app_key_write: String,
    app_key_readonly: String,
    device_token: String,
    device_token_other: String,
}

async fn setup_fixture() -> TestFixture {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");

    let tenant = create_tenant(&pool, "API Sessions Tenant")
        .await
        .expect("tenant");
    let app = create_application(
        &pool,
        &tenant.id.to_string(),
        "api-sessions-app",
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

    let generated = generate_signing_key("api-session-test").expect("generate key");
    let signing = parse_private_pem(generated.private_pem()).expect("parse key");
    let kid = "api-session-test";

    let mut keyset = DeviceTokenKeys::default();
    keyset.insert_signing_key(kid, signing.clone());

    let token_req_1 = serde_json::from_value(json!({
        "external_subject_id": "member-user-1",
        "installation_id": "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d3a",
        "platform": "ANDROID"
    }))
    .expect("token req");

    let minted_1 = mint_device_token(
        &pool,
        write_key.credential.as_str(),
        &token_req_1,
        TokenTtlBounds {
            min_seconds: 300,
            default_seconds: 900,
            max_seconds: 3600,
        },
        "https://geostream.example.com",
        &signing,
        kid,
    )
    .await
    .expect("mint 1");

    let token_req_2 = serde_json::from_value(json!({
        "external_subject_id": "other-user-2",
        "installation_id": "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d3b",
        "platform": "IOS"
    }))
    .expect("token req");

    let minted_2 = mint_device_token(
        &pool,
        write_key.credential.as_str(),
        &token_req_2,
        TokenTtlBounds {
            min_seconds: 300,
            default_seconds: 900,
            max_seconds: 3600,
        },
        "https://geostream.example.com",
        &signing,
        kid,
    )
    .await
    .expect("mint 2");

    let raw = [11u8; 32];
    let rotator =
        CryptoRotator::empty().with_active(DataCipher::new("test_key", &raw).expect("cipher"));

    let api_state = SessionApiState {
        pool,
        signing_keys: Arc::new(keyset),
        issuer: "https://geostream.example.com".to_string(),
        leeway_seconds: 5,
        crypto_rotator: Some(Arc::new(rotator)),
        idempotency_retention: Duration::from_secs(3600),
    };

    let router = sessions_router(api_state);
    let (address, _server) = spawn(router).await;

    TestFixture {
        address,
        _server,
        app_key_write: write_key.credential.to_string(),
        app_key_readonly: read_key.credential.to_string(),
        device_token: minted_1.response.access_token,
        device_token_other: minted_2.response.access_token,
    }
}

async fn json_response(resp: reqwest::Response) -> Value {
    let text = resp.text().await.expect("text");
    serde_json::from_str(&text).expect("json")
}

#[tokio::test]
async fn test_application_creates_session_and_idempotency_replays() {
    let fixture = setup_fixture().await;
    let client = reqwest::Client::new();

    let session_payload = json!({
        "mode": "ONE_TO_ONE",
        "expires_in_seconds": 1800,
        "max_members": 2,
        "metadata": { "order_id": "ord_9988" }
    });

    let resp = client
        .post(format!("{}/v1/sessions", fixture.address))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.app_key_write))
        .header("Idempotency-Key", "idemp-session-create-1")
        .header(CONTENT_TYPE, "application/json")
        .body(session_payload.to_string())
        .send()
        .await
        .expect("send");

    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    assert!(resp.headers().contains_key(ETAG));
    assert!(resp.headers().contains_key(REQUEST_ID_HEADER));

    let body = json_response(resp).await;
    let session_id = body["id"].as_str().expect("id").to_string();
    assert_eq!(body["mode"], "ONE_TO_ONE");
    assert_eq!(body["created_by"]["type"], "APPLICATION");
    assert_eq!(body["metadata"]["order_id"], "ord_9988");

    // Second request with same idempotency key returns replayed response
    let replay_resp = client
        .post(format!("{}/v1/sessions", fixture.address))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.app_key_write))
        .header("Idempotency-Key", "idemp-session-create-1")
        .header(CONTENT_TYPE, "application/json")
        .body(session_payload.to_string())
        .send()
        .await
        .expect("send replay");

    assert_eq!(replay_resp.status(), reqwest::StatusCode::CREATED);
    assert_eq!(
        replay_resp
            .headers()
            .get("idempotent-replayed")
            .and_then(|v| v.to_str().ok()),
        Some("true")
    );
    let replay_body = json_response(replay_resp).await;
    assert_eq!(replay_body["id"], session_id);
}

#[tokio::test]
async fn test_device_creates_session_and_validation_rules() {
    let fixture = setup_fixture().await;
    let client = reqwest::Client::new();

    // 1. Device creates a session
    let valid_payload = json!({
        "mode": "ONE_TO_MANY",
        "expires_in_seconds": 1200,
        "max_members": 10
    });

    let resp = client
        .post(format!("{}/v1/sessions", fixture.address))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.device_token))
        .header(CONTENT_TYPE, "application/json")
        .body(valid_payload.to_string())
        .send()
        .await
        .expect("send");

    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    let body = json_response(resp).await;
    assert_eq!(body["mode"], "ONE_TO_MANY");
    assert_eq!(body["created_by"]["type"], "DEVICE");
    assert_eq!(body["active_member_count"], 1);
    assert!(body["owner_member_id"].is_string());

    // 2. Device trying to pass owner_external_subject_id must get 422
    let forbidden_owner_payload = json!({
        "mode": "ONE_TO_MANY",
        "owner_external_subject_id": "someone_else"
    });

    let err_resp = client
        .post(format!("{}/v1/sessions", fixture.address))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.device_token))
        .header(CONTENT_TYPE, "application/json")
        .body(forbidden_owner_payload.to_string())
        .send()
        .await
        .expect("send");

    assert_eq!(err_resp.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
    let err_body = json_response(err_resp).await;
    assert_eq!(err_body["code"], "VALIDATION_FAILED");
}

#[tokio::test]
async fn test_get_and_list_sessions_and_permissions() {
    let fixture = setup_fixture().await;
    let client = reqwest::Client::new();

    // Device creates a session where it is OWNER
    let resp = client
        .post(format!("{}/v1/sessions", fixture.address))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.device_token))
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "mode": "MANY_TO_MANY",
                "expires_in_seconds": 3600,
                "max_members": 5
            })
            .to_string(),
        )
        .send()
        .await
        .expect("create");
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    let created = json_response(resp).await;
    let session_id = created["id"].as_str().expect("id");

    // Device 1 gets its session: 200 OK
    let get_resp = client
        .get(format!("{}/v1/sessions/{}", fixture.address, session_id))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.device_token))
        .send()
        .await
        .expect("get");
    assert_eq!(get_resp.status(), reqwest::StatusCode::OK);
    assert!(get_resp.headers().contains_key(ETAG));

    // Device 2 (not a member) gets session: 404 Not Found (docs/07 §9.2)
    let get_other_resp = client
        .get(format!("{}/v1/sessions/{}", fixture.address, session_id))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fixture.device_token_other),
        )
        .send()
        .await
        .expect("get other");
    assert_eq!(get_other_resp.status(), reqwest::StatusCode::NOT_FOUND);

    // Device 1 lists sessions: 200 OK
    let list_dev_resp = client
        .get(format!("{}/v1/sessions", fixture.address))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.device_token))
        .send()
        .await
        .expect("list dev");
    assert_eq!(list_dev_resp.status(), reqwest::StatusCode::OK);
    let list_dev = json_response(list_dev_resp).await;
    let data = list_dev["data"].as_array().expect("array");
    assert!(data.iter().any(|s| s["id"] == session_id));

    // Device 1 tries to list with status filter: 422 VALIDATION_FAILED (docs/07 §9.2)
    let list_filter_err = client
        .get(format!("{}/v1/sessions?status=ACTIVE", fixture.address))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.device_token))
        .send()
        .await
        .expect("list filter");
    assert_eq!(
        list_filter_err.status(),
        reqwest::StatusCode::UNPROCESSABLE_ENTITY
    );

    // Application with sessions:read lists sessions: 200 OK
    let app_list_resp = client
        .get(format!("{}/v1/sessions?status=ACTIVE", fixture.address))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fixture.app_key_readonly),
        )
        .send()
        .await
        .expect("app list");
    assert_eq!(app_list_resp.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn test_update_session_with_if_match_and_concurrency() {
    let fixture = setup_fixture().await;
    let client = reqwest::Client::new();

    // Create session via application
    let create_resp = client
        .post(format!("{}/v1/sessions", fixture.address))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.app_key_write))
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "mode": "MANY_TO_MANY",
                "expires_in_seconds": 3600,
                "max_members": 5
            })
            .to_string(),
        )
        .send()
        .await
        .expect("create");
    let created = json_response(create_resp).await;
    let session_id = created["id"].as_str().expect("id");
    let version = created["version"].as_u64().expect("version");
    assert_eq!(version, 1);

    // 1. Missing If-Match header -> 412 PRECONDITION_FAILED
    let patch_no_if_match = client
        .patch(format!("{}/v1/sessions/{}", fixture.address, session_id))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.app_key_write))
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "max_members": 8 }).to_string())
        .send()
        .await
        .expect("patch no if match");
    assert_eq!(
        patch_no_if_match.status(),
        reqwest::StatusCode::PRECONDITION_FAILED
    );

    // 2. Mismatched If-Match header -> 412 PRECONDITION_FAILED
    let patch_mismatch = client
        .patch(format!("{}/v1/sessions/{}", fixture.address, session_id))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.app_key_write))
        .header(IF_MATCH, "\"999\"")
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "max_members": 8 }).to_string())
        .send()
        .await
        .expect("patch mismatch");
    assert_eq!(
        patch_mismatch.status(),
        reqwest::StatusCode::PRECONDITION_FAILED
    );

    // 3. Matched If-Match header -> 200 OK with incremented version
    let patch_ok = client
        .patch(format!("{}/v1/sessions/{}", fixture.address, session_id))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.app_key_write))
        .header(IF_MATCH, "\"1\"")
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "max_members": 8,
                "metadata": { "status": "updated" }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("patch ok");
    assert_eq!(patch_ok.status(), reqwest::StatusCode::OK);
    let updated = json_response(patch_ok).await;
    assert_eq!(updated["version"], 2);
    assert_eq!(updated["max_members"], 8);
    assert_eq!(updated["metadata"]["status"], "updated");
}

#[tokio::test]
async fn test_close_session_is_idempotent() {
    let fixture = setup_fixture().await;
    let client = reqwest::Client::new();

    // Create session
    let create_resp = client
        .post(format!("{}/v1/sessions", fixture.address))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.app_key_write))
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "mode": "ONE_TO_MANY",
                "expires_in_seconds": 1800,
                "max_members": 5
            })
            .to_string(),
        )
        .send()
        .await
        .expect("create");
    let created = json_response(create_resp).await;
    let session_id = created["id"].as_str().expect("id");

    // Close session
    let close_resp = client
        .post(format!(
            "{}/v1/sessions/{}/close",
            fixture.address, session_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.app_key_write))
        .send()
        .await
        .expect("close");
    assert_eq!(close_resp.status(), reqwest::StatusCode::OK);
    let closed = json_response(close_resp).await;
    assert_eq!(closed["status"], "CLOSED");
    assert_eq!(closed["closed_reason"], "CLOSED_BY_APPLICATION");

    // Second close returns 200 OK and same closed status (idempotent, INV-SES-3)
    let second_close_resp = client
        .post(format!(
            "{}/v1/sessions/{}/close",
            fixture.address, session_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.app_key_write))
        .send()
        .await
        .expect("second close");
    assert_eq!(second_close_resp.status(), reqwest::StatusCode::OK);
    let second_closed = json_response(second_close_resp).await;
    assert_eq!(second_closed["status"], "CLOSED");
}
