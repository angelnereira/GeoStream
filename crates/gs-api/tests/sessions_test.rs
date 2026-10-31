//! Integration tests for the session REST control plane (T-F3-03, FR-SES-001..006, docs/07 §9.2).
#![forbid(unsafe_code)]

use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use gs_api::{REQUEST_ID_HEADER, SessionApiState, StreamTicketConfig, sessions_router};
use gs_service::admin::{create_api_key, create_application, create_tenant};
use gs_service::auth::DeviceTokenKeys;
use gs_service::gs_auth::crypto::{CryptoRotator, DataCipher};
use gs_service::gs_auth::{generate_signing_key, parse_private_pem};
use gs_service::gs_ephemeral::EphemeralStore;
use gs_service::migrate::{connect_pool, run_migrations};
use gs_service::pbjson_types::Timestamp;
use gs_service::prost::Message as _;
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
    app_key_locations_read: String,
    device_token: String,
    device_token_other: String,
    device_token_no_read: String,
    ephemeral: Arc<gs_service::EphemeralBackend>,
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

    let locations_key = create_api_key(
        &pool,
        &app.id.to_string(),
        Some(vec![
            "sessions:write".into(),
            "sessions:read".into(),
            "locations:read".into(),
            "tokens:mint".into(),
        ]),
        None,
        None,
    )
    .await
    .expect("locations key");

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

    let token_req_3 = serde_json::from_value(json!({
        "external_subject_id": "no-read-user-3",
        "installation_id": "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d3c",
        "platform": "ANDROID"
    }))
    .expect("token req 3");

    let minted_3 = mint_device_token(
        &pool,
        write_key.credential.as_str(),
        &token_req_3,
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
    .expect("mint 3");

    let raw = [11u8; 32];
    let rotator =
        CryptoRotator::empty().with_active(DataCipher::new("test_key", &raw).expect("cipher"));

    let ephemeral = gs_service::EphemeralBackend::memory();

    let api_state = SessionApiState {
        pool: pool.clone(),
        signing_keys: Arc::new(keyset),
        issuer: "https://geostream.example.com".to_string(),
        leeway_seconds: 5,
        crypto_rotator: Some(Arc::new(rotator)),
        idempotency_retention: Duration::from_secs(3600),
        ephemeral: ephemeral.clone(),
        bus: gs_service::BusBackend::memory(),
        redis_key_prefix: "gs".to_string(),
        stream_ticket: StreamTicketConfig::default(),
        rate_limit_config: gs_api::RateLimitConfig::default(),
    };

    let router = sessions_router(api_state);
    let (address, _server) = spawn(router).await;

    TestFixture {
        address,
        _server,
        app_key_write: write_key.credential.to_string(),
        app_key_readonly: read_key.credential.to_string(),
        app_key_locations_read: locations_key.credential.to_string(),
        device_token: minted_1.response.access_token,
        device_token_other: minted_2.response.access_token,
        device_token_no_read: minted_3.response.access_token,
        ephemeral,
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

#[tokio::test]
async fn test_fr_int_002_application_reads_snapshot_with_locations_read() {
    // covers: FR-INT-002, docs/07 §9.6
    let fixture = setup_fixture().await;
    let client = reqwest::Client::new();

    // 1. Create a session with snapshot_ttl_seconds = 600 and member-user-1
    let create_resp = client
        .post(format!("{}/v1/sessions", fixture.address))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fixture.app_key_locations_read),
        )
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "mode": "ONE_TO_MANY",
                "expires_in_seconds": 3600,
                "max_members": 5,
                "snapshot_ttl_seconds": 600,
                "initial_members": [
                    {
                        "external_subject_id": "member-user-1",
                        "capabilities": {
                            "publish": true,
                            "read": true,
                            "invite": false,
                            "manage": false
                        }
                    }
                ]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("create");
    assert_eq!(create_resp.status(), reqwest::StatusCode::CREATED);
    let created = json_response(create_resp).await;
    let session_id = created["id"].as_str().expect("id");

    // 2. Device 1 activates sharing via /me/sharing
    let share_resp = client
        .put(format!(
            "{}/v1/sessions/{session_id}/members/me/sharing",
            fixture.address
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.device_token))
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "sharing": true }).to_string())
        .send()
        .await
        .expect("share");
    assert_eq!(share_resp.status(), reqwest::StatusCode::OK);
    let share_body = json_response(share_resp).await;
    assert_eq!(share_body["sharing"], true);
    let member_id = share_body["id"].as_str().expect("member_id");

    // 3. Seed snapshot into ephemeral store
    let now = time::OffsetDateTime::now_utc();
    let update = gs_service::gs_protocol::LocationUpdate {
        session_id: session_id.to_string(),
        member_id: member_id.to_string(),
        sample: Some(gs_service::gs_protocol::LocationSample {
            sequence: 101,
            captured_at: Some(Timestamp {
                seconds: now.unix_timestamp() - 5,
                nanos: 0,
            }),
            position: Some(gs_service::gs_protocol::Position {
                latitude: 40.7128,
                longitude: -74.0060,
                horizontal_accuracy_m: 8.5,
                altitude_m: Some(10.0),
                vertical_accuracy_m: Some(1.5),
                speed_mps: Some(2.5),
                speed_accuracy_mps: Some(0.2),
                heading_deg: Some(180.0),
                heading_accuracy_deg: Some(5.0),
            }),
            quality: Some(gs_service::gs_protocol::LocationQuality {
                source: gs_service::gs_protocol::LocationSource::Gnss as i32,
                kind: gs_service::gs_protocol::LocationKind::Measured as i32,
                is_mock: Some(false),
                motion: gs_service::gs_protocol::MotionActivity::Automotive as i32,
            }),
        }),
        received_at: Some(Timestamp {
            seconds: now.unix_timestamp() - 4,
            nanos: 0,
        }),
        delivery: gs_service::gs_protocol::DeliveryKind::Live as i32,
        estimate: None,
    };
    let snap_key = format!("gs:snap:{session_id}:{member_id}");
    fixture
        .ephemeral
        .set_snapshot(&snap_key, &update.encode_to_vec(), Duration::from_secs(600))
        .await
        .expect("set_snapshot");

    // 4. Application reads snapshot
    let snapshot_resp = client
        .get(format!(
            "{}/v1/sessions/{session_id}/snapshot",
            fixture.address
        ))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fixture.app_key_locations_read),
        )
        .send()
        .await
        .expect("snapshot");
    assert_eq!(snapshot_resp.status(), reqwest::StatusCode::OK);
    assert_eq!(
        snapshot_resp
            .headers()
            .get(reqwest::header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok()),
        Some("no-store")
    );
    assert!(snapshot_resp.headers().contains_key(REQUEST_ID_HEADER));

    let snapshot_body = json_response(snapshot_resp).await;
    assert_eq!(snapshot_body["session_id"], session_id);
    assert!(snapshot_body["generated_at"].is_string());

    let items = snapshot_body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);

    let item = &items[0];
    assert_eq!(item["member_id"], member_id);
    assert_eq!(item["external_subject_id"], "member-user-1");
    assert!(item["received_at"].is_string());
    assert!(item["age_ms"].as_u64().expect("age_ms") >= 4000);

    let sample = &item["sample"];
    assert_eq!(sample["sequence"], 101);
    assert!(sample["captured_at"].is_string());
    assert_eq!(sample["position"]["latitude"], 40.7128);
    assert_eq!(sample["position"]["longitude"], -74.0060);
    assert_eq!(sample["position"]["horizontal_accuracy_m"], 8.5);
    assert_eq!(sample["position"]["altitude_m"], 10.0);
    assert_eq!(sample["position"]["vertical_accuracy_m"], 1.5);
    assert_eq!(sample["position"]["speed_mps"], 2.5);
    assert_eq!(sample["position"]["speed_accuracy_mps"], 0.2);
    assert_eq!(sample["position"]["heading_deg"], 180.0);
    assert_eq!(sample["position"]["heading_accuracy_deg"], 5.0);

    assert_eq!(sample["quality"]["source"], "GNSS");
    assert_eq!(sample["quality"]["kind"], "MEASURED");
    assert_eq!(sample["quality"]["motion"], "AUTOMOTIVE");
    assert_eq!(sample["quality"]["is_mock"], false);
}

#[tokio::test]
async fn test_fr_int_002_device_with_read_capability_reads_snapshot() {
    // covers: FR-INT-002, docs/07 §9.6
    let fixture = setup_fixture().await;
    let client = reqwest::Client::new();

    // Create session via device 1 (owner, automatically gets read: true)
    let create_resp = client
        .post(format!("{}/v1/sessions", fixture.address))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.device_token))
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "mode": "ONE_TO_ONE",
                "expires_in_seconds": 3600
            })
            .to_string(),
        )
        .send()
        .await
        .expect("create");
    assert_eq!(create_resp.status(), reqwest::StatusCode::CREATED);
    let created = json_response(create_resp).await;
    let session_id = created["id"].as_str().expect("id");
    let owner_member_id = created["owner_member_id"]
        .as_str()
        .expect("owner_member_id");

    // Activate sharing
    let share_resp = client
        .put(format!(
            "{}/v1/sessions/{session_id}/members/me/sharing",
            fixture.address
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.device_token))
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "sharing": true }).to_string())
        .send()
        .await
        .expect("share");
    assert_eq!(share_resp.status(), reqwest::StatusCode::OK);

    // Seed snapshot in ephemeral store
    let now = time::OffsetDateTime::now_utc();
    let update = gs_service::gs_protocol::LocationUpdate {
        session_id: session_id.to_string(),
        member_id: owner_member_id.to_string(),
        sample: Some(gs_service::gs_protocol::LocationSample {
            sequence: 1,
            captured_at: Some(Timestamp {
                seconds: now.unix_timestamp() - 2,
                nanos: 0,
            }),
            position: Some(gs_service::gs_protocol::Position {
                latitude: -12.0464,
                longitude: -77.0428,
                horizontal_accuracy_m: 5.0,
                altitude_m: None,
                vertical_accuracy_m: None,
                speed_mps: None,
                speed_accuracy_mps: None,
                heading_deg: None,
                heading_accuracy_deg: None,
            }),
            quality: Some(gs_service::gs_protocol::LocationQuality {
                source: gs_service::gs_protocol::LocationSource::Fused as i32,
                kind: gs_service::gs_protocol::LocationKind::Measured as i32,
                is_mock: None,
                motion: gs_service::gs_protocol::MotionActivity::Walking as i32,
            }),
        }),
        received_at: Some(Timestamp {
            seconds: now.unix_timestamp() - 1,
            nanos: 0,
        }),
        delivery: gs_service::gs_protocol::DeliveryKind::Live as i32,
        estimate: None,
    };
    let snap_key = format!("gs:snap:{session_id}:{owner_member_id}");
    fixture
        .ephemeral
        .set_snapshot(&snap_key, &update.encode_to_vec(), Duration::from_secs(600))
        .await
        .expect("set_snapshot");

    // Device 1 reads snapshot
    let snapshot_resp = client
        .get(format!(
            "{}/v1/sessions/{session_id}/snapshot",
            fixture.address
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.device_token))
        .send()
        .await
        .expect("snapshot");
    assert_eq!(snapshot_resp.status(), reqwest::StatusCode::OK);

    let snapshot_body = json_response(snapshot_resp).await;
    assert_eq!(snapshot_body["session_id"], session_id);
    let items = snapshot_body["items"].as_array().expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["member_id"], owner_member_id);
    assert_eq!(items[0]["sample"]["sequence"], 1);
    assert_eq!(items[0]["sample"]["position"]["latitude"], -12.0464);
    assert_eq!(items[0]["sample"]["position"]["longitude"], -77.0428);
    assert_eq!(items[0]["sample"]["quality"]["source"], "FUSED");
    assert_eq!(items[0]["sample"]["quality"]["motion"], "WALKING");
}

#[tokio::test]
async fn test_fr_int_002_device_without_read_capability_gets_403() {
    // covers: FR-INT-002, docs/07 §9.6
    let fixture = setup_fixture().await;
    let client = reqwest::Client::new();

    // Create session with initial member without read capability
    let create_resp = client
        .post(format!("{}/v1/sessions", fixture.address))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fixture.app_key_locations_read),
        )
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "mode": "ONE_TO_MANY",
                "expires_in_seconds": 3600,
                "initial_members": [
                    {
                        "external_subject_id": "no-read-user-3",
                        "capabilities": {
                            "publish": true,
                            "read": false,
                            "invite": false,
                            "manage": false
                        }
                    }
                ]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("create");
    assert_eq!(create_resp.status(), reqwest::StatusCode::CREATED);
    let created = json_response(create_resp).await;
    let session_id = created["id"].as_str().expect("id");

    // Device 3 (no-read-user-3) attempts to read snapshot -> 403 PERMISSION_DENIED
    let snapshot_resp = client
        .get(format!(
            "{}/v1/sessions/{session_id}/snapshot",
            fixture.address
        ))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fixture.device_token_no_read),
        )
        .send()
        .await
        .expect("snapshot");
    assert_eq!(snapshot_resp.status(), reqwest::StatusCode::FORBIDDEN);
    let problem = json_response(snapshot_resp).await;
    assert_eq!(problem["code"], "PERMISSION_DENIED");
}

#[tokio::test]
async fn test_fr_int_002_device_not_member_gets_404() {
    // covers: FR-INT-002, docs/07 §9.6
    let fixture = setup_fixture().await;
    let client = reqwest::Client::new();

    // Create session without device 2
    let create_resp = client
        .post(format!("{}/v1/sessions", fixture.address))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.device_token))
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "mode": "ONE_TO_ONE", "expires_in_seconds": 3600 }).to_string())
        .send()
        .await
        .expect("create");
    let created = json_response(create_resp).await;
    let session_id = created["id"].as_str().expect("id");

    // Device 2 (other-user-2, not member) attempts to read snapshot -> 404 NOT_FOUND
    let snapshot_resp = client
        .get(format!(
            "{}/v1/sessions/{session_id}/snapshot",
            fixture.address
        ))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fixture.device_token_other),
        )
        .send()
        .await
        .expect("snapshot");
    assert_eq!(snapshot_resp.status(), reqwest::StatusCode::NOT_FOUND);
    let problem = json_response(snapshot_resp).await;
    assert_eq!(problem["code"], "NOT_FOUND");
}

#[tokio::test]
async fn test_fr_int_002_application_without_locations_read_scope_gets_403() {
    // covers: FR-INT-002, docs/07 §9.6
    let fixture = setup_fixture().await;
    let client = reqwest::Client::new();

    let create_resp = client
        .post(format!("{}/v1/sessions", fixture.address))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fixture.app_key_locations_read),
        )
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "mode": "ONE_TO_ONE", "expires_in_seconds": 3600 }).to_string())
        .send()
        .await
        .expect("create");
    let created = json_response(create_resp).await;
    let session_id = created["id"].as_str().expect("id");

    // Application with app_key_readonly (only sessions:read, no locations:read) -> 403 INSUFFICIENT_SCOPE
    let snapshot_resp = client
        .get(format!(
            "{}/v1/sessions/{session_id}/snapshot",
            fixture.address
        ))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fixture.app_key_readonly),
        )
        .send()
        .await
        .expect("snapshot");
    assert_eq!(snapshot_resp.status(), reqwest::StatusCode::FORBIDDEN);
    let problem = json_response(snapshot_resp).await;
    assert_eq!(problem["code"], "INSUFFICIENT_SCOPE");
}

#[tokio::test]
async fn test_fr_int_002_session_with_snapshot_ttl_zero_returns_empty_items() {
    // covers: FR-INT-002, docs/07 §9.6
    let fixture = setup_fixture().await;
    let client = reqwest::Client::new();

    // Create session with snapshot_ttl_seconds: 0
    let create_resp = client
        .post(format!("{}/v1/sessions", fixture.address))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fixture.app_key_locations_read),
        )
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "mode": "ONE_TO_MANY",
                "expires_in_seconds": 3600,
                "snapshot_ttl_seconds": 0,
                "initial_members": [
                    {
                        "external_subject_id": "member-user-1",
                        "capabilities": {
                            "publish": true,
                            "read": true,
                            "invite": false,
                            "manage": false
                        }
                    }
                ]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("create");
    let created = json_response(create_resp).await;
    let session_id = created["id"].as_str().expect("id");

    // Device 1 shares
    let share_resp = client
        .put(format!(
            "{}/v1/sessions/{session_id}/members/me/sharing",
            fixture.address
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.device_token))
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "sharing": true }).to_string())
        .send()
        .await
        .expect("share");
    let share_body = json_response(share_resp).await;
    let member_id = share_body["id"].as_str().expect("member_id");

    // Even if an ephemeral snapshot somehow exists:
    let now = time::OffsetDateTime::now_utc();
    let update = gs_service::gs_protocol::LocationUpdate {
        session_id: session_id.to_string(),
        member_id: member_id.to_string(),
        sample: Some(gs_service::gs_protocol::LocationSample {
            sequence: 50,
            captured_at: Some(Timestamp {
                seconds: now.unix_timestamp(),
                nanos: 0,
            }),
            position: Some(gs_service::gs_protocol::Position {
                latitude: 10.0,
                longitude: 20.0,
                horizontal_accuracy_m: 5.0,
                altitude_m: None,
                vertical_accuracy_m: None,
                speed_mps: None,
                speed_accuracy_mps: None,
                heading_deg: None,
                heading_accuracy_deg: None,
            }),
            quality: Some(gs_service::gs_protocol::LocationQuality {
                source: 1,
                kind: 1,
                is_mock: None,
                motion: 0,
            }),
        }),
        received_at: Some(Timestamp {
            seconds: now.unix_timestamp(),
            nanos: 0,
        }),
        delivery: 2,
        estimate: None,
    };
    let snap_key = format!("gs:snap:{session_id}:{member_id}");
    fixture
        .ephemeral
        .set_snapshot(&snap_key, &update.encode_to_vec(), Duration::from_secs(600))
        .await
        .expect("set_snapshot");

    // Must return 200 with empty items array (FR-INT-002: "vacío si la sesión tiene snapshot_ttl_seconds=0")
    let snapshot_resp = client
        .get(format!(
            "{}/v1/sessions/{session_id}/snapshot",
            fixture.address
        ))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fixture.app_key_locations_read),
        )
        .send()
        .await
        .expect("snapshot");
    assert_eq!(snapshot_resp.status(), reqwest::StatusCode::OK);
    let snapshot_body = json_response(snapshot_resp).await;
    let items = snapshot_body["items"].as_array().expect("items");
    assert!(
        items.is_empty(),
        "snapshot must be empty when snapshot_ttl_seconds=0"
    );
}

#[tokio::test]
async fn test_fr_int_002_non_sharing_member_is_excluded_from_snapshot() {
    // covers: FR-INT-002, docs/07 §9.6
    let fixture = setup_fixture().await;
    let client = reqwest::Client::new();

    let create_resp = client
        .post(format!("{}/v1/sessions", fixture.address))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fixture.app_key_locations_read),
        )
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "mode": "ONE_TO_MANY",
                "expires_in_seconds": 3600,
                "snapshot_ttl_seconds": 600,
                "initial_members": [
                    {
                        "external_subject_id": "member-user-1",
                        "capabilities": {
                            "publish": true,
                            "read": true,
                            "invite": false,
                            "manage": false
                        }
                    }
                ]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("create");
    let created = json_response(create_resp).await;
    let session_id = created["id"].as_str().expect("id");

    // Member has sharing = false by default (INV-SHR-1)
    // Seed snapshot in ephemeral anyway:
    let now = time::OffsetDateTime::now_utc();
    let update = gs_service::gs_protocol::LocationUpdate {
        session_id: session_id.to_string(),
        member_id: "mem_fake".to_string(),
        sample: Some(gs_service::gs_protocol::LocationSample {
            sequence: 1,
            captured_at: Some(Timestamp {
                seconds: now.unix_timestamp(),
                nanos: 0,
            }),
            position: Some(gs_service::gs_protocol::Position {
                latitude: 10.0,
                longitude: 20.0,
                horizontal_accuracy_m: 5.0,
                altitude_m: None,
                vertical_accuracy_m: None,
                speed_mps: None,
                speed_accuracy_mps: None,
                heading_deg: None,
                heading_accuracy_deg: None,
            }),
            quality: Some(gs_service::gs_protocol::LocationQuality {
                source: 1,
                kind: 1,
                is_mock: None,
                motion: 0,
            }),
        }),
        received_at: Some(Timestamp {
            seconds: now.unix_timestamp(),
            nanos: 0,
        }),
        delivery: 2,
        estimate: None,
    };
    let snap_key = format!("gs:snap:{session_id}:mem_fake");
    fixture
        .ephemeral
        .set_snapshot(&snap_key, &update.encode_to_vec(), Duration::from_secs(600))
        .await
        .expect("set_snapshot");

    // Member is not sharing, so snapshot must be empty
    let snapshot_resp = client
        .get(format!(
            "{}/v1/sessions/{session_id}/snapshot",
            fixture.address
        ))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fixture.app_key_locations_read),
        )
        .send()
        .await
        .expect("snapshot");
    assert_eq!(snapshot_resp.status(), reqwest::StatusCode::OK);
    let snapshot_body = json_response(snapshot_resp).await;
    let items = snapshot_body["items"].as_array().expect("items");
    assert!(
        items.is_empty(),
        "non-sharing member must be excluded from snapshot"
    );
}

#[tokio::test]
async fn test_fr_int_002_closed_session_returns_409() {
    // covers: FR-INT-002, docs/07 §9.6
    let fixture = setup_fixture().await;
    let client = reqwest::Client::new();

    let create_resp = client
        .post(format!("{}/v1/sessions", fixture.address))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fixture.app_key_locations_read),
        )
        .header(CONTENT_TYPE, "application/json")
        .body(json!({ "mode": "ONE_TO_ONE", "expires_in_seconds": 3600 }).to_string())
        .send()
        .await
        .expect("create");
    let created = json_response(create_resp).await;
    let session_id = created["id"].as_str().expect("id");

    // Close session
    let close_resp = client
        .post(format!(
            "{}/v1/sessions/{session_id}/close",
            fixture.address
        ))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fixture.app_key_locations_read),
        )
        .send()
        .await
        .expect("close");
    assert_eq!(close_resp.status(), reqwest::StatusCode::OK);

    // GET snapshot on closed session -> 409 SESSION_CLOSED
    let snapshot_resp = client
        .get(format!(
            "{}/v1/sessions/{session_id}/snapshot",
            fixture.address
        ))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fixture.app_key_locations_read),
        )
        .send()
        .await
        .expect("snapshot");
    assert_eq!(snapshot_resp.status(), reqwest::StatusCode::CONFLICT);
    let problem = json_response(snapshot_resp).await;
    assert_eq!(problem["code"], "SESSION_CLOSED");
}

#[tokio::test]
async fn test_fr_int_002_nonexistent_session_returns_404() {
    // covers: FR-INT-002, docs/07 §9.6
    let fixture = setup_fixture().await;
    let client = reqwest::Client::new();

    let snapshot_resp = client
        .get(format!(
            "{}/v1/sessions/ses_0192b6c0a1e47c3f9d2a7b5e8f1c0000/snapshot",
            fixture.address
        ))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", fixture.app_key_locations_read),
        )
        .send()
        .await
        .expect("snapshot");
    assert_eq!(snapshot_resp.status(), reqwest::StatusCode::NOT_FOUND);
    let problem = json_response(snapshot_resp).await;
    assert_eq!(problem["code"], "NOT_FOUND");
}
