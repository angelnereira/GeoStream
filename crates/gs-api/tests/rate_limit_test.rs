//! Integration tests for REST rate limits and invitation redemption limits
//! (T-F3-10, FR-API-004, docs/07 §8, docs/11 §3, docs/16 §4.2).
#![forbid(unsafe_code)]

use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use axum::http::header;
use gs_api::{RateLimitConfig, SessionApiState, StreamTicketConfig, sessions_router};
use gs_service::admin::{create_api_key, create_application, create_tenant};
use gs_service::auth::DeviceTokenKeys;
use gs_service::gs_auth::crypto::{CryptoRotator, DataCipher};
use gs_service::gs_auth::{generate_signing_key, parse_private_pem};
use gs_service::migrate::{connect_pool, run_migrations};
use gs_service::tokens::{TokenRequest, TokenTtlBounds, mint_device_token};
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

async fn json_response(resp: reqwest::Response) -> Value {
    let text = resp.text().await.expect("text");
    serde_json::from_str(&text).expect("json")
}

struct RateLimitFixture {
    address: String,
    _server: tokio::task::JoinHandle<()>,
    app_key_1: String,
    app_key_2: String,
    device_token_1: String,
    device_token_2: String,
}

async fn setup_fixture(rate_limit_config: RateLimitConfig) -> RateLimitFixture {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");

    let tenant = create_tenant(&pool, "Rate Limit Test Tenant")
        .await
        .expect("tenant");
    let app = create_application(&pool, &tenant.id.to_string(), "rate-limit-app", None, None)
        .await
        .expect("application");

    let write_scopes = vec![
        "sessions:write".into(),
        "sessions:read".into(),
        "members:write".into(),
        "invitations:write".into(),
        "tokens:mint".into(),
    ];

    let key_1 = create_api_key(
        &pool,
        &app.id.to_string(),
        Some(write_scopes.clone()),
        None,
        None,
    )
    .await
    .expect("key 1");

    let key_2 = create_api_key(&pool, &app.id.to_string(), Some(write_scopes), None, None)
        .await
        .expect("key 2");

    let kid = "rl-test-key";
    let generated = generate_signing_key(kid).expect("generate key");
    let signing = parse_private_pem(generated.private_pem()).expect("parse key");

    let mut keyset = DeviceTokenKeys::default();
    keyset.insert_signing_key(kid, signing.clone());

    let token_req_1 = TokenRequest {
        external_subject_id: "rl-user-1".into(),
        installation_id: uuid::Uuid::new_v4().to_string(),
        platform: "ANDROID".into(),
        app_version: None,
        sdk_version: None,
        scopes: Some(vec!["invitations:redeem".into(), "sharing:write".into()]),
        ttl_seconds: None,
    };
    let minted_1 = mint_device_token(
        &pool,
        key_1.credential.as_str(),
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
    .expect("mint token 1");

    let token_req_2 = TokenRequest {
        external_subject_id: "rl-user-2".into(),
        installation_id: uuid::Uuid::new_v4().to_string(),
        platform: "IOS".into(),
        app_version: None,
        sdk_version: None,
        scopes: Some(vec!["invitations:redeem".into(), "sharing:write".into()]),
        ttl_seconds: None,
    };
    let minted_2 = mint_device_token(
        &pool,
        key_1.credential.as_str(),
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
    .expect("mint token 2");

    let raw = [42u8; 32];
    let rotator =
        CryptoRotator::empty().with_active(DataCipher::new("test_cipher", &raw).expect("cipher"));

    let state = SessionApiState {
        pool,
        signing_keys: Arc::new(keyset),
        issuer: "https://geostream.example.com".to_string(),
        leeway_seconds: 60,
        crypto_rotator: Some(Arc::new(rotator)),
        idempotency_retention: Duration::from_secs(3600),
        ephemeral: gs_service::InMemoryEphemeralStore::new(),
        bus: gs_service::InMemoryBus::new(),
        redis_key_prefix: format!("gs:rl:{}", uuid::Uuid::new_v4().simple()),
        stream_ticket: StreamTicketConfig::default(),
        rate_limit_config,
    };

    let router = sessions_router(state);
    let (address, _server) = spawn(router).await;

    RateLimitFixture {
        address,
        _server,
        app_key_1: key_1.credential.to_string(),
        app_key_2: key_2.credential.to_string(),
        device_token_1: minted_1.response.access_token,
        device_token_2: minted_2.response.access_token,
    }
}

#[tokio::test]
async fn test_fr_api_004_device_rate_limit_exceeded() {
    // Configure tight device limit of 3 requests per minute
    let config = RateLimitConfig {
        device_rpm: 3,
        app_rpm: 1000,
        invite_redeem_rpm: 10,
        auth_failure_rpm: 60,
    };
    let fixture = setup_fixture(config).await;
    let client = reqwest::Client::new();

    // Requests 1..=3 from device 1 must succeed
    for i in 1..=3 {
        let resp = client
            .get(format!("{}/v1/sessions", fixture.address))
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", fixture.device_token_1),
            )
            .send()
            .await
            .expect("send");
        assert_eq!(
            resp.status(),
            reqwest::StatusCode::OK,
            "Request {i} for device 1 should succeed"
        );
    }

    // Request 4 from device 1 must be rejected with 429 Too Many Requests
    let resp4 = client
        .get(format!("{}/v1/sessions", fixture.address))
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", fixture.device_token_1),
        )
        .send()
        .await
        .expect("send");

    assert_eq!(resp4.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);

    // Acceptance criterion: Retry-After header present and integer >= 1
    let retry_after_hdr = resp4
        .headers()
        .get(header::RETRY_AFTER)
        .expect("Retry-After header must be present");
    let retry_after: u64 = retry_after_hdr
        .to_str()
        .expect("utf-8")
        .parse()
        .expect("integer");
    assert!(
        (1..=60).contains(&retry_after),
        "Retry-After must be integer >= 1 and <= 60, got {retry_after}"
    );

    // Informative RateLimit-* headers
    assert_eq!(
        resp4
            .headers()
            .get("ratelimit-limit")
            .and_then(|v| v.to_str().ok()),
        Some("3")
    );
    assert_eq!(
        resp4
            .headers()
            .get("ratelimit-remaining")
            .and_then(|v| v.to_str().ok()),
        Some("0")
    );

    // Problem Details body
    let body: Value = json_response(resp4).await;
    assert_eq!(body["status"], 429);
    assert_eq!(body["code"], "RATE_LIMITED");
    assert_eq!(body["type"], "https://geostream.dev/problems/rate_limited");

    // Device 2 must NOT be affected (strict principal isolation)
    let resp_dev2 = client
        .get(format!("{}/v1/sessions", fixture.address))
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", fixture.device_token_2),
        )
        .send()
        .await
        .expect("send");
    assert_eq!(
        resp_dev2.status(),
        reqwest::StatusCode::OK,
        "Device 2 should not be throttled by Device 1's usage"
    );
}

#[tokio::test]
async fn test_fr_api_004_application_rate_limit_exceeded() {
    // Configure tight application limit of 2 requests per minute
    let config = RateLimitConfig {
        device_rpm: 120,
        app_rpm: 2,
        invite_redeem_rpm: 10,
        auth_failure_rpm: 60,
    };
    let fixture = setup_fixture(config).await;
    let client = reqwest::Client::new();

    // Requests 1 and 2 from app key 1 must succeed
    for i in 1..=2 {
        let resp = client
            .get(format!("{}/v1/sessions", fixture.address))
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", fixture.app_key_1),
            )
            .send()
            .await
            .expect("send");
        assert_eq!(
            resp.status(),
            reqwest::StatusCode::OK,
            "Request {i} for app key 1 should succeed"
        );
    }

    // Request 3 from app key 1 must be rejected with 429
    let resp3 = client
        .get(format!("{}/v1/sessions", fixture.address))
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", fixture.app_key_1),
        )
        .send()
        .await
        .expect("send");

    assert_eq!(resp3.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);

    let retry_after_hdr = resp3
        .headers()
        .get(header::RETRY_AFTER)
        .expect("Retry-After header must be present");
    let retry_after: u64 = retry_after_hdr
        .to_str()
        .expect("utf-8")
        .parse()
        .expect("integer");
    assert!((1..=60).contains(&retry_after));

    assert_eq!(
        resp3
            .headers()
            .get("ratelimit-limit")
            .and_then(|v| v.to_str().ok()),
        Some("2")
    );

    let body: Value = json_response(resp3).await;
    assert_eq!(body["status"], 429);
    assert_eq!(body["code"], "RATE_LIMITED");

    // App key 2 must NOT be throttled
    let resp_key2 = client
        .get(format!("{}/v1/sessions", fixture.address))
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", fixture.app_key_2),
        )
        .send()
        .await
        .expect("send");
    assert_eq!(
        resp_key2.status(),
        reqwest::StatusCode::OK,
        "App key 2 should not be affected by App key 1 rate limit"
    );
}

#[tokio::test]
async fn test_fr_api_004_auth_failure_per_ip_rate_limit() {
    // Configure tight auth failure limit of 2 failures per minute per IP
    let config = RateLimitConfig {
        device_rpm: 120,
        app_rpm: 6000,
        invite_redeem_rpm: 10,
        auth_failure_rpm: 2,
    };
    let fixture = setup_fixture(config).await;
    let client = reqwest::Client::new();
    let test_ip = "198.51.100.42";

    // First 2 invalid attempts from test_ip must return 401 UNAUTHENTICATED
    for i in 1..=2 {
        let resp = client
            .get(format!("{}/v1/sessions", fixture.address))
            .header(header::AUTHORIZATION, "Bearer gsk_bad_key.invalidsecret")
            .header("X-Forwarded-For", test_ip)
            .send()
            .await
            .expect("send");
        assert_eq!(
            resp.status(),
            reqwest::StatusCode::UNAUTHORIZED,
            "Auth failure {i} should return 401"
        );
        let body: Value = json_response(resp).await;
        assert_eq!(body["code"], "UNAUTHENTICATED");
    }

    // 3rd invalid attempt from same IP must be throttled with 429 RATE_LIMITED
    let resp3 = client
        .get(format!("{}/v1/sessions", fixture.address))
        .header(header::AUTHORIZATION, "Bearer gsk_bad_key.invalidsecret")
        .header("X-Forwarded-For", test_ip)
        .send()
        .await
        .expect("send");

    assert_eq!(
        resp3.status(),
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        "3rd auth failure from same IP must be throttled with 429"
    );

    let retry_after_hdr = resp3
        .headers()
        .get(header::RETRY_AFTER)
        .expect("Retry-After must be present on 429");
    let retry_after: u64 = retry_after_hdr
        .to_str()
        .expect("utf-8")
        .parse()
        .expect("integer");
    assert!((1..=60).contains(&retry_after));

    let body: Value = json_response(resp3).await;
    assert_eq!(body["status"], 429);
    assert_eq!(body["code"], "RATE_LIMITED");

    // A different IP must NOT be throttled yet
    let other_ip = "198.51.100.99";
    let resp_other = client
        .get(format!("{}/v1/sessions", fixture.address))
        .header(header::AUTHORIZATION, "Bearer gsk_bad_key.invalidsecret")
        .header("X-Forwarded-For", other_ip)
        .send()
        .await
        .expect("send");

    assert_eq!(
        resp_other.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "Different IP should return 401, not 429"
    );
}

#[tokio::test]
async fn test_fr_api_004_invitation_redeem_rate_limit() {
    // Configure tight invite redeem limit of 2 per minute
    let config = RateLimitConfig {
        device_rpm: 120,
        app_rpm: 6000,
        invite_redeem_rpm: 2,
        auth_failure_rpm: 60,
    };
    let fixture = setup_fixture(config).await;
    let client = reqwest::Client::new();

    // 1. Create a session via app key
    let sess_resp = client
        .post(format!("{}/v1/sessions", fixture.address))
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", fixture.app_key_1),
        )
        .header(header::CONTENT_TYPE, "application/json")
        .body(
            json!({
                "mode": "ONE_TO_MANY",
                "expires_in_seconds": 3600
            })
            .to_string(),
        )
        .send()
        .await
        .expect("create session");
    assert_eq!(sess_resp.status(), reqwest::StatusCode::CREATED);
    let sess_body: Value = json_response(sess_resp).await;
    let session_id = sess_body["id"].as_str().expect("session id");

    // 2. Create an invitation
    let inv_resp = client
        .post(format!(
            "{}/v1/sessions/{session_id}/invitations",
            fixture.address
        ))
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", fixture.app_key_1),
        )
        .header(header::CONTENT_TYPE, "application/json")
        .body(
            json!({
                "max_uses": 10,
                "expires_in_seconds": 1800
            })
            .to_string(),
        )
        .send()
        .await
        .expect("create inv");
    assert_eq!(inv_resp.status(), reqwest::StatusCode::CREATED);
    let inv_body: Value = json_response(inv_resp).await;
    let code = inv_body["code"].as_str().expect("code");

    // 3. First attempt to redeem by device 1 succeeds (201)
    let r1 = client
        .post(format!("{}/v1/invitations/redeem", fixture.address))
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", fixture.device_token_1),
        )
        .header(header::CONTENT_TYPE, "application/json")
        .body(json!({ "code": code }).to_string())
        .send()
        .await
        .expect("redeem 1");
    let status1 = r1.status();
    let body1 = json_response(r1).await;
    assert_eq!(
        status1,
        reqwest::StatusCode::CREATED,
        "Response was: {body1:?}"
    );

    // 4. Second attempt by device 1 (subject already member, so 409 Conflict, but consumed a rate check)
    let r2 = client
        .post(format!("{}/v1/invitations/redeem", fixture.address))
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", fixture.device_token_1),
        )
        .header(header::CONTENT_TYPE, "application/json")
        .body(json!({ "code": code }).to_string())
        .send()
        .await
        .expect("redeem 2");
    assert_eq!(r2.status(), reqwest::StatusCode::CONFLICT);

    // 5. Third attempt by device 1 must be rejected by invite_redeem rate limit with 429
    let r3 = client
        .post(format!("{}/v1/invitations/redeem", fixture.address))
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", fixture.device_token_1),
        )
        .header(header::CONTENT_TYPE, "application/json")
        .body(json!({ "code": code }).to_string())
        .send()
        .await
        .expect("redeem 3");

    assert_eq!(r3.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);

    let retry_after_hdr = r3
        .headers()
        .get(header::RETRY_AFTER)
        .expect("Retry-After header present on 429");
    let retry_after: u64 = retry_after_hdr
        .to_str()
        .expect("utf-8")
        .parse()
        .expect("integer");
    assert!((1..=60).contains(&retry_after));

    let body: Value = json_response(r3).await;
    assert_eq!(body["status"], 429);
    assert_eq!(body["code"], "RATE_LIMITED");

    // Device 2 should still be able to attempt redemption without 429
    let r_dev2 = client
        .post(format!("{}/v1/invitations/redeem", fixture.address))
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", fixture.device_token_2),
        )
        .header(header::CONTENT_TYPE, "application/json")
        .body(json!({ "code": code }).to_string())
        .send()
        .await
        .expect("redeem dev2");
    assert_eq!(
        r_dev2.status(),
        reqwest::StatusCode::CREATED,
        "Device 2 must have its own independent redemption rate bucket"
    );
}
