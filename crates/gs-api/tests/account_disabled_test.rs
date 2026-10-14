//! Integration tests for disabling/enabling tenant and application REST effects (T-F3-08, FR-ADM-003, docs/06 §7, docs/16 §3).
#![forbid(unsafe_code)]

use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use gs_api::{SessionApiState, StreamTicketConfig, sessions_router};
use gs_service::admin::{
    create_api_key, create_application, create_tenant, set_application_enabled_with_bus,
    set_tenant_enabled_with_bus,
};
use gs_service::auth::DeviceTokenKeys;
use gs_service::gs_auth::crypto::{CryptoRotator, DataCipher};
use gs_service::gs_auth::{generate_signing_key, parse_private_pem};
use gs_service::migrate::{connect_pool, run_migrations};
use gs_service::tokens::mint_device_token;
use reqwest::header::AUTHORIZATION;
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
    pool: gs_service::migrate::PgPool,
    bus: Arc<gs_service::InMemoryBus>,
    tenant_id: String,
    app_id: String,
    app_key: String,
    device_token: String,
    device_id: String,
    subject_id: String,
}

async fn setup_fixture() -> ApiFixture {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");

    let tenant = create_tenant(&pool, "Disable/Enable Tenant")
        .await
        .expect("tenant");
    let app = create_application(
        &pool,
        &tenant.id.to_string(),
        "disable-enable-app",
        None,
        None,
    )
    .await
    .expect("application");

    let app_key = create_api_key(
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
    .expect("app key");

    let key_dir = std::env::temp_dir().join(format!("gs-api-disable-keys-{}", tenant.id));
    let _ = std::fs::remove_dir_all(&key_dir);
    let key_pair = generate_signing_key("active-key").expect("generate key");
    key_pair.write_pair(&key_dir).expect("write key pair");
    let signing_keys = Arc::new(DeviceTokenKeys::load_from_dir(&key_dir).expect("load keys"));
    let private_key = parse_private_pem(key_pair.private_pem()).expect("parse private");

    let token_req = gs_service::tokens::TokenRequest {
        external_subject_id: "ext-user-disabled-test".into(),
        installation_id: uuid::Uuid::new_v4().to_string(),
        platform: "IOS".into(),
        app_version: Some("1.0.0".into()),
        sdk_version: Some("1.0.0".into()),
        scopes: Some(vec![
            "sessions:create".into(),
            "stream:connect".into(),
            "sharing:write".into(),
        ]),
        ttl_seconds: Some(3600),
    };
    let bounds = gs_service::tokens::TokenTtlBounds {
        min_seconds: 60,
        default_seconds: 3600,
        max_seconds: 86400,
    };
    let minted = mint_device_token(
        &pool,
        app_key.credential.as_str(),
        &token_req,
        bounds,
        "https://geostream.example.com",
        &private_key,
        "active-key",
    )
    .await
    .expect("mint device token");

    let raw_key = [7u8; 32];
    let rotator = CryptoRotator::empty()
        .with_active(DataCipher::new("test_key_disable", &raw_key).expect("cipher"));

    let ephemeral = gs_service::InMemoryEphemeralStore::new();
    let bus = gs_service::InMemoryBus::new();

    let state = SessionApiState {
        pool: pool.clone(),
        signing_keys,
        issuer: "https://geostream.example.com".into(),
        leeway_seconds: 30,
        crypto_rotator: Some(Arc::new(rotator)),
        idempotency_retention: Duration::from_secs(86_400),
        ephemeral,
        bus: bus.clone(),
        redis_key_prefix: "gs".into(),
        stream_ticket: StreamTicketConfig::default(),
        rate_limit_config: gs_api::RateLimitConfig::default(),
    };

    let router = sessions_router(state);
    let (address, server) = spawn(router).await;

    ApiFixture {
        address,
        _server: server,
        pool,
        bus,
        tenant_id: tenant.id.to_string(),
        app_id: app.id.to_string(),
        app_key: app_key.credential.as_str().to_string(),
        device_token: minted.response.access_token.as_str().to_string(),
        device_id: minted.device.id.to_string(),
        subject_id: minted.subject.id.to_string(),
    }
}

#[tokio::test]
async fn test_disabled_application_blocks_rest_and_returns_403_account_disabled() {
    // covers: FR-ADM-003, docs/06 §7, docs/07 §5
    let fixture = setup_fixture().await;
    let client = reqwest::Client::new();

    // Baseline: Both application and device can query resources successfully.
    let resp = client
        .get(format!(
            "{}/v1/subjects/{}",
            fixture.address, fixture.subject_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.app_key))
        .send()
        .await
        .expect("get subject");
    assert_eq!(resp.status(), 200);

    let resp = client
        .get(format!(
            "{}/v1/devices/{}",
            fixture.address, fixture.device_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.device_token))
        .send()
        .await
        .expect("get device");
    assert_eq!(resp.status(), 200);

    // 1. Disable the application with bus invalidation.
    set_application_enabled_with_bus(
        &fixture.pool,
        &fixture.app_id,
        false,
        Some(&*fixture.bus),
        Some("gs"),
    )
    .await
    .expect("disable application");

    // 2. Application key request must return 403 ACCOUNT_DISABLED.
    let resp = client
        .get(format!(
            "{}/v1/subjects/{}",
            fixture.address, fixture.subject_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.app_key))
        .send()
        .await
        .expect("get subject disabled app");
    assert_eq!(resp.status(), 403);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "application/problem+json"
    );
    let problem = json_response(resp).await;
    assert_eq!(problem["status"], 403);
    assert_eq!(problem["code"], "ACCOUNT_DISABLED");
    assert_eq!(problem["title"], "Account disabled");

    // 3. Device token request for that application must return 403 ACCOUNT_DISABLED.
    let resp = client
        .get(format!(
            "{}/v1/devices/{}",
            fixture.address, fixture.device_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.device_token))
        .send()
        .await
        .expect("get device disabled app");
    assert_eq!(resp.status(), 403);
    let problem = json_response(resp).await;
    assert_eq!(problem["status"], 403);
    assert_eq!(problem["code"], "ACCOUNT_DISABLED");
    assert_eq!(problem["title"], "Account disabled");

    // 4. Session creation request with device token must return 403 ACCOUNT_DISABLED.
    let resp = client
        .post(format!("{}/v1/sessions", fixture.address))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.device_token))
        .header("Content-Type", "application/json")
        .body(r#"{"mode":"DISPATCH"}"#)
        .send()
        .await
        .expect("create session disabled app");
    assert_eq!(resp.status(), 403);
    let problem = json_response(resp).await;
    assert_eq!(problem["code"], "ACCOUNT_DISABLED");

    // 5. Re-enable application -> Both can access resources again.
    set_application_enabled_with_bus(
        &fixture.pool,
        &fixture.app_id,
        true,
        Some(&*fixture.bus),
        Some("gs"),
    )
    .await
    .expect("enable application");

    let resp = client
        .get(format!(
            "{}/v1/subjects/{}",
            fixture.address, fixture.subject_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.app_key))
        .send()
        .await
        .expect("get subject re-enabled app");
    assert_eq!(resp.status(), 200);

    let resp = client
        .get(format!(
            "{}/v1/devices/{}",
            fixture.address, fixture.device_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.device_token))
        .send()
        .await
        .expect("get device re-enabled app");
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_disabled_tenant_blocks_rest_and_returns_403_account_disabled() {
    // covers: FR-ADM-003, docs/06 §7, docs/07 §5
    let fixture = setup_fixture().await;
    let client = reqwest::Client::new();

    // 1. Disable the tenant with bus invalidation.
    set_tenant_enabled_with_bus(
        &fixture.pool,
        &fixture.tenant_id,
        false,
        Some(&*fixture.bus),
        Some("gs"),
    )
    .await
    .expect("disable tenant");

    // 2. Application key request must return 403 ACCOUNT_DISABLED.
    let resp = client
        .get(format!(
            "{}/v1/subjects/{}",
            fixture.address, fixture.subject_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.app_key))
        .send()
        .await
        .expect("get subject disabled tenant");
    assert_eq!(resp.status(), 403);
    let problem = json_response(resp).await;
    assert_eq!(problem["status"], 403);
    assert_eq!(problem["code"], "ACCOUNT_DISABLED");
    assert_eq!(problem["title"], "Account disabled");

    // 3. Device token request must return 403 ACCOUNT_DISABLED.
    let resp = client
        .get(format!(
            "{}/v1/devices/{}",
            fixture.address, fixture.device_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.device_token))
        .send()
        .await
        .expect("get device disabled tenant");
    assert_eq!(resp.status(), 403);
    let problem = json_response(resp).await;
    assert_eq!(problem["status"], 403);
    assert_eq!(problem["code"], "ACCOUNT_DISABLED");

    // 4. Re-enable the tenant -> requests succeed again.
    set_tenant_enabled_with_bus(
        &fixture.pool,
        &fixture.tenant_id,
        true,
        Some(&*fixture.bus),
        Some("gs"),
    )
    .await
    .expect("enable tenant");

    let resp = client
        .get(format!(
            "{}/v1/subjects/{}",
            fixture.address, fixture.subject_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.app_key))
        .send()
        .await
        .expect("get subject re-enabled tenant");
    assert_eq!(resp.status(), 200);

    let resp = client
        .get(format!(
            "{}/v1/devices/{}",
            fixture.address, fixture.device_id
        ))
        .header(AUTHORIZATION, format!("Bearer {}", fixture.device_token))
        .send()
        .await
        .expect("get device re-enabled tenant");
    assert_eq!(resp.status(), 200);
}
