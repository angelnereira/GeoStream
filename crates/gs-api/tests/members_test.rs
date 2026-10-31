//! Integration tests for the membership REST API (T-F3-04, FR-MEM-001..007, FR-SES-007, docs/07 §9.3).
#![forbid(unsafe_code)]

use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use gs_api::{REQUEST_ID_HEADER, SessionApiState, StreamTicketConfig, sessions_router};
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
    device_token_no_sharing: String,
}

async fn setup_fixture() -> TestFixture {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");

    let tenant = create_tenant(&pool, "API Members Tenant")
        .await
        .expect("tenant");
    let app = create_application(&pool, &tenant.id.to_string(), "api-members-app", None, None)
        .await
        .expect("application");

    let write_key = create_api_key(
        &pool,
        &app.id.to_string(),
        Some(vec![
            "sessions:write".into(),
            "sessions:read".into(),
            "members:write".into(),
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

    let generated = generate_signing_key("api-member-test").expect("generate key");
    let signing = parse_private_pem(generated.private_pem()).expect("parse key");
    let kid = "api-member-test";

    let mut keyset = DeviceTokenKeys::default();
    keyset.insert_signing_key(kid, signing.clone());

    let token_req_1 = serde_json::from_value(json!({
        "external_subject_id": "device-member-1",
        "installation_id": "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d11",
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
        "external_subject_id": "device-member-2",
        "installation_id": "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d12",
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

    let token_req_no_sharing = serde_json::from_value(json!({
        "external_subject_id": "device-member-no-sharing",
        "installation_id": "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d13",
        "platform": "ANDROID",
        "scopes": ["sessions:create"]
    }))
    .expect("token req");

    let minted_no_sharing = mint_device_token(
        &pool,
        write_key.credential.as_str(),
        &token_req_no_sharing,
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
    .expect("mint no sharing");

    let raw = [12u8; 32];
    let rotator = CryptoRotator::empty()
        .with_active(DataCipher::new("test_key_members", &raw).expect("cipher"));

    let api_state = SessionApiState {
        pool,
        signing_keys: Arc::new(keyset),
        issuer: "https://geostream.example.com".to_string(),
        leeway_seconds: 60,
        crypto_rotator: Some(Arc::new(rotator)),
        idempotency_retention: Duration::from_secs(86_400),
        ephemeral: gs_service::EphemeralBackend::memory(),
        bus: gs_service::BusBackend::memory(),
        redis_key_prefix: "gs".to_string(),
        stream_ticket: StreamTicketConfig::default(),
        rate_limit_config: gs_api::RateLimitConfig::default(),
    };

    let router = sessions_router(api_state);
    let (address, server) = spawn(router).await;

    TestFixture {
        address,
        _server: server,
        app_key_write: write_key.credential.to_string(),
        app_key_readonly: read_key.credential.to_string(),
        device_token: minted_1.response.access_token,
        device_token_other: minted_2.response.access_token,
        device_token_no_sharing: minted_no_sharing.response.access_token,
    }
}

async fn json_response(resp: reqwest::Response) -> Value {
    let text = resp.text().await.expect("text");
    serde_json::from_str(&text).expect("json")
}

async fn create_test_session(fix: &TestFixture, mode: &str, max_members: Option<u32>) -> String {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/sessions", fix.address))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "mode": mode,
                "expires_in_seconds": 7200,
                "max_members": max_members
            })
            .to_string(),
        )
        .send()
        .await
        .expect("create session request");

    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    let body = json_response(resp).await;
    body["id"].as_str().expect("id").to_string()
}

#[tokio::test]
async fn test_api_add_member_and_authorization() {
    // covers: FR-MEM-001, docs/07 §9.3
    let fix = setup_fixture().await;
    let session_id = create_test_session(&fix, "MANY_TO_MANY", Some(10)).await;
    let client = reqwest::Client::new();

    // 1. Device caller attempting to add member directly -> 403 PERMISSION_DENIED
    let resp_device = client
        .post(format!(
            "{}/v1/sessions/{}/members",
            fix.address, session_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.device_token))
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "external_subject_id": "direct-sub-1" }).to_string())
        .send()
        .await
        .expect("send");
    assert_eq!(resp_device.status(), reqwest::StatusCode::FORBIDDEN);

    // 2. Application without members:write scope -> 403 INSUFFICIENT_SCOPE
    let resp_read_key = client
        .post(format!(
            "{}/v1/sessions/{}/members",
            fix.address, session_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_readonly))
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "external_subject_id": "direct-sub-1" }).to_string())
        .send()
        .await
        .expect("send");
    assert_eq!(resp_read_key.status(), reqwest::StatusCode::FORBIDDEN);

    // 3. Application with members:write scope -> 201 CREATED
    let resp_ok = client
        .post(format!(
            "{}/v1/sessions/{}/members",
            fix.address, session_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .header("Idempotency-Key", "idemp-add-member-1")
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "external_subject_id": "direct-sub-1",
                "capabilities": { "publish": true, "read": true, "invite": false, "manage": false }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("send");

    assert_eq!(resp_ok.status(), reqwest::StatusCode::CREATED);
    assert!(resp_ok.headers().contains_key(ETAG));
    assert!(resp_ok.headers().contains_key(REQUEST_ID_HEADER));

    let body = json_response(resp_ok).await;
    assert_eq!(body["external_subject_id"], "direct-sub-1");
    assert_eq!(body["role"], "MEMBER");
    assert_eq!(body["status"], "ACTIVE");
    assert_eq!(body["sharing"], false);
    assert_eq!(body["connectivity"], "UNSPECIFIED");
    assert_eq!(body["joined_via"], "DIRECT_ADD");

    // 4. Replay with same Idempotency-Key -> returns stored response with idempotent-replayed
    let resp_replay = client
        .post(format!(
            "{}/v1/sessions/{}/members",
            fix.address, session_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .header("Idempotency-Key", "idemp-add-member-1")
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "external_subject_id": "direct-sub-1",
                "capabilities": { "publish": true, "read": true, "invite": false, "manage": false }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("send");

    assert_eq!(resp_replay.status(), reqwest::StatusCode::CREATED);
    assert_eq!(
        resp_replay
            .headers()
            .get("idempotent-replayed")
            .and_then(|h| h.to_str().ok()),
        Some("true")
    );
}

#[tokio::test]
async fn test_api_list_members_and_pagination() {
    // covers: FR-MEM-002, docs/07 §9.3
    let fix = setup_fixture().await;
    let session_id = create_test_session(&fix, "MANY_TO_MANY", Some(10)).await;
    let client = reqwest::Client::new();

    // Add 3 members
    for i in 1..=3 {
        let resp = client
            .post(format!(
                "{}/v1/sessions/{}/members",
                fix.address, session_id
            ))
            .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
            .header(CONTENT_TYPE, "application/json")
            .body(json!({ "external_subject_id": format!("user-{i}") }).to_string())
            .send()
            .await
            .expect("send");
        assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    }

    // List with limit=2
    let resp_page1 = client
        .get(format!(
            "{}/v1/sessions/{}/members?limit=2",
            fix.address, session_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .send()
        .await
        .expect("send");
    assert_eq!(resp_page1.status(), reqwest::StatusCode::OK);

    let body1 = json_response(resp_page1).await;
    let data1 = body1["data"].as_array().expect("data array");
    assert_eq!(data1.len(), 2);
    let next_cursor = body1["next_cursor"].as_str().expect("next cursor");

    // Next page
    let resp_page2 = client
        .get(format!(
            "{}/v1/sessions/{}/members?limit=2&cursor={}",
            fix.address, session_id, next_cursor
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .send()
        .await
        .expect("send");
    assert_eq!(resp_page2.status(), reqwest::StatusCode::OK);

    let body2 = json_response(resp_page2).await;
    let data2 = body2["data"].as_array().expect("data array");
    assert_eq!(data2.len(), 1);
    assert!(body2["next_cursor"].is_null());
}

#[tokio::test]
async fn test_api_get_member_and_me() {
    // covers: FR-MEM-003, docs/07 §9.3
    let fix = setup_fixture().await;
    let client = reqwest::Client::new();

    // Create session by device
    let resp_create = client
        .post(format!("{}/v1/sessions", fix.address))
        .header(AUTHORIZATION, format!("Bearer {}", fix.device_token))
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "mode": "MANY_TO_MANY",
                "expires_in_seconds": 7200
            })
            .to_string(),
        )
        .send()
        .await
        .expect("send");
    assert_eq!(resp_create.status(), reqwest::StatusCode::CREATED);
    let sess_body = json_response(resp_create).await;
    let session_id = sess_body["id"].as_str().unwrap();

    // 1. Device calls GET .../members/me -> resolves to owner member
    let resp_me = client
        .get(format!(
            "{}/v1/sessions/{}/members/me",
            fix.address, session_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.device_token))
        .send()
        .await
        .expect("send");
    assert_eq!(resp_me.status(), reqwest::StatusCode::OK);
    let me_body = json_response(resp_me).await;
    assert_eq!(me_body["external_subject_id"], "device-member-1");
    assert_eq!(me_body["role"], "OWNER");
    let member_id = me_body["id"].as_str().unwrap();

    // 2. Application calls GET .../members/me -> 404 NOT_FOUND
    let resp_app_me = client
        .get(format!(
            "{}/v1/sessions/{}/members/me",
            fix.address, session_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .send()
        .await
        .expect("send");
    assert_eq!(resp_app_me.status(), reqwest::StatusCode::NOT_FOUND);

    // 3. Application calls GET .../members/{id} -> 200 OK
    let resp_by_id = client
        .get(format!(
            "{}/v1/sessions/{}/members/{}",
            fix.address, session_id, member_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .send()
        .await
        .expect("send");
    assert_eq!(resp_by_id.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn test_api_update_member_and_ownership_transfer() {
    // covers: FR-MEM-004, FR-MEM-005, INV-MEM-1..3
    let fix = setup_fixture().await;
    let session_id = create_test_session(&fix, "MANY_TO_MANY", Some(10)).await;
    let client = reqwest::Client::new();

    // Add target member
    let add_resp = client
        .post(format!(
            "{}/v1/sessions/{}/members",
            fix.address, session_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "external_subject_id": "transfer-target"
            })
            .to_string(),
        )
        .send()
        .await
        .expect("send");
    assert_eq!(add_resp.status(), reqwest::StatusCode::CREATED);
    let etag = add_resp
        .headers()
        .get(ETAG)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let add_body = json_response(add_resp).await;
    let target_id = add_body["id"].as_str().unwrap();

    // Stale If-Match fails with 412 PRECONDITION_FAILED
    let resp_stale = client
        .patch(format!(
            "{}/v1/sessions/{}/members/{}",
            fix.address, session_id, target_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .header(IF_MATCH, "\"999\"")
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "capabilities": { "publish": true, "read": true, "invite": true, "manage": false }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("send");
    assert_eq!(
        resp_stale.status(),
        reqwest::StatusCode::PRECONDITION_FAILED
    );

    // Valid update with If-Match
    let resp_patch = client
        .patch(format!(
            "{}/v1/sessions/{}/members/{}",
            fix.address, session_id, target_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .header(IF_MATCH, &etag)
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "capabilities": { "publish": true, "read": true, "invite": true, "manage": true }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("send");
    assert_eq!(resp_patch.status(), reqwest::StatusCode::OK);

    // Ownership transfer: role: OWNER
    let resp_transfer = client
        .patch(format!(
            "{}/v1/sessions/{}/members/{}",
            fix.address, session_id, target_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "role": "OWNER"
            })
            .to_string(),
        )
        .send()
        .await
        .expect("send");
    assert_eq!(resp_transfer.status(), reqwest::StatusCode::OK);
    let transfer_body = json_response(resp_transfer).await;
    assert_eq!(transfer_body["role"], "OWNER");
    assert_eq!(transfer_body["capabilities"]["manage"], true);
}

#[tokio::test]
async fn test_api_revoke_and_leave() {
    // covers: FR-MEM-006, FR-SES-007, docs/07 §9.3
    let fix = setup_fixture().await;
    let client = reqwest::Client::new();

    // Create session by device
    let resp_create = client
        .post(format!("{}/v1/sessions", fix.address))
        .header(AUTHORIZATION, format!("Bearer {}", fix.device_token))
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "mode": "MANY_TO_MANY",
                "expires_in_seconds": 7200
            })
            .to_string(),
        )
        .send()
        .await
        .expect("send");
    let sess_body = json_response(resp_create).await;
    let session_id = sess_body["id"].as_str().unwrap();

    // Add extra member
    let add_resp = client
        .post(format!(
            "{}/v1/sessions/{}/members",
            fix.address, session_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "external_subject_id": "device-member-2"
            })
            .to_string(),
        )
        .send()
        .await
        .expect("send");
    assert_eq!(add_resp.status(), reqwest::StatusCode::CREATED);
    let member_2_body = json_response(add_resp).await;
    let member_2_id = member_2_body["id"].as_str().unwrap();

    // 1. Device 2 leaves session -> 204 NO_CONTENT
    let resp_leave = client
        .post(format!("{}/v1/sessions/{}/leave", fix.address, session_id))
        .header(AUTHORIZATION, format!("Bearer {}", fix.device_token_other))
        .send()
        .await
        .expect("send");
    assert_eq!(resp_leave.status(), reqwest::StatusCode::NO_CONTENT);

    // 2. Application revokes member 2 -> idempotent 204 NO_CONTENT
    let resp_revoke = client
        .delete(format!(
            "{}/v1/sessions/{}/members/{}",
            fix.address, session_id, member_2_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .send()
        .await
        .expect("send");
    assert_eq!(resp_revoke.status(), reqwest::StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_api_sharing_control_plane() {
    // covers: FR-SHR-001..004, INV-SHR-1..3, docs/05 §7, docs/07 §9.3
    let fix = setup_fixture().await;
    let client = reqwest::Client::new();

    // 1. Create session by device 1
    let resp_create = client
        .post(format!("{}/v1/sessions", fix.address))
        .header(AUTHORIZATION, format!("Bearer {}", fix.device_token))
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "mode": "MANY_TO_MANY",
                "expires_in_seconds": 7200
            })
            .to_string(),
        )
        .send()
        .await
        .expect("send");
    assert_eq!(resp_create.status(), reqwest::StatusCode::CREATED);
    let sess_body = json_response(resp_create).await;
    let session_id = sess_body["id"].as_str().unwrap();

    // Get owner member id
    let resp_me = client
        .get(format!(
            "{}/v1/sessions/{}/members/me",
            fix.address, session_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.device_token))
        .send()
        .await
        .expect("send");
    let me_body = json_response(resp_me).await;
    let owner_member_id = me_body["id"].as_str().unwrap();
    assert_eq!(me_body["sharing"], false);

    // 2. Device without sharing:write tries to activate sharing -> 403 INSUFFICIENT_SCOPE
    let resp_no_scope = client
        .put(format!(
            "{}/v1/sessions/{}/members/me/sharing",
            fix.address, session_id
        ))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fix.device_token_no_sharing),
        )
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "sharing": true }).to_string())
        .send()
        .await
        .expect("send");
    assert_eq!(resp_no_scope.status(), reqwest::StatusCode::FORBIDDEN);

    // 3. Application tries /me/sharing -> 404 NOT_FOUND
    let resp_app_me = client
        .put(format!(
            "{}/v1/sessions/{}/members/me/sharing",
            fix.address, session_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "sharing": true }).to_string())
        .send()
        .await
        .expect("send");
    assert_eq!(resp_app_me.status(), reqwest::StatusCode::NOT_FOUND);

    // 4. Application with readonly key tries to modify sharing -> 403 INSUFFICIENT_SCOPE
    let resp_app_ro = client
        .put(format!(
            "{}/v1/sessions/{}/members/{}/sharing",
            fix.address, session_id, owner_member_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_readonly))
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "sharing": false }).to_string())
        .send()
        .await
        .expect("send");
    assert_eq!(resp_app_ro.status(), reqwest::StatusCode::FORBIDDEN);

    // 5. Application tries to activate sharing for owner -> 403 PERMISSION_DENIED (INV-SHR-2)
    let resp_app_start = client
        .put(format!(
            "{}/v1/sessions/{}/members/{}/sharing",
            fix.address, session_id, owner_member_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "sharing": true }).to_string())
        .send()
        .await
        .expect("send");
    assert_eq!(resp_app_start.status(), reqwest::StatusCode::FORBIDDEN);

    // 6. Device 1 activates sharing via /me/sharing -> 200 OK
    let resp_start = client
        .put(format!(
            "{}/v1/sessions/{}/members/me/sharing",
            fix.address, session_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.device_token))
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "sharing": true }).to_string())
        .send()
        .await
        .expect("send");
    assert_eq!(resp_start.status(), reqwest::StatusCode::OK);
    assert!(resp_start.headers().contains_key(ETAG));
    assert!(resp_start.headers().contains_key("x-request-id"));
    let start_body = json_response(resp_start).await;
    assert_eq!(start_body["sharing"], true);

    // 7. Device 1 idempotently activates sharing -> 200 OK
    let resp_start_idem = client
        .put(format!(
            "{}/v1/sessions/{}/members/me/sharing",
            fix.address, session_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.device_token))
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "sharing": true }).to_string())
        .send()
        .await
        .expect("send");
    assert_eq!(resp_start_idem.status(), reqwest::StatusCode::OK);
    let idem_body = json_response(resp_start_idem).await;
    assert_eq!(idem_body["sharing"], true);

    // 8. Application stops sharing for owner -> 200 OK
    let resp_app_stop = client
        .put(format!(
            "{}/v1/sessions/{}/members/{}/sharing",
            fix.address, session_id, owner_member_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.app_key_write))
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "sharing": false }).to_string())
        .send()
        .await
        .expect("send");
    assert_eq!(resp_app_stop.status(), reqwest::StatusCode::OK);
    let stop_body = json_response(resp_app_stop).await;
    assert_eq!(stop_body["sharing"], false);

    // 9. Device 1 stops sharing idempotently -> 200 OK
    let resp_dev_stop = client
        .put(format!(
            "{}/v1/sessions/{}/members/me/sharing",
            fix.address, session_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fix.device_token))
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "sharing": false }).to_string())
        .send()
        .await
        .expect("send");
    assert_eq!(resp_dev_stop.status(), reqwest::StatusCode::OK);
    let dev_stop_body = json_response(resp_dev_stop).await;
    assert_eq!(dev_stop_body["sharing"], false);
}
