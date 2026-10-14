//! `POST /v1/stream/tickets` over HTTP (T-F4-01, FR-AUTH-006, docs/06 §5, docs/07 §9.1).
//!
//! Every response, errors included, is validated against the contract
//! (`contracts/openapi/geostream.v1.yaml`, docs/18 §5).
#![forbid(unsafe_code)]

mod common;

use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use common::spec::load_spec;
use gs_api::{RateLimitConfig, SessionApiState, StreamTicketConfig, sessions_router};
use gs_service::admin::{create_api_key, create_application, create_tenant};
use gs_service::auth::DeviceTokenKeys;
use gs_service::gs_auth::{generate_signing_key, parse_private_pem};
use gs_service::migrate::{connect_pool, run_migrations};
use gs_service::tokens::{TokenRequest, TokenTtlBounds, mint_device_token};
use serde_json::{Value, json};

const ISSUER: &str = "https://geostream.example.com";
const TICKET_TTL_S: i64 = 30;

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

struct Reply {
    status: u16,
    body: Value,
    content_type: String,
    cache_control: Option<String>,
    request_id: Option<String>,
}

struct World {
    base: String,
    spec: &'static common::spec::Spec,
    _server: tokio::task::JoinHandle<()>,
    key_stream: String,
    key_without_scope: String,
    device_token: String,
    device_without_scope: String,
}

async fn setup(suffix: &str) -> World {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");

    let tenant = create_tenant(&pool, &format!("Ticket Http Tenant {suffix}"))
        .await
        .expect("tenant");
    let application = create_application(
        &pool,
        &tenant.id.to_string(),
        &format!("ticket-http-app-{suffix}"),
        Some(vec![
            "sessions:create".to_string(),
            "stream:connect".to_string(),
        ]),
        None,
    )
    .await
    .expect("application");

    let key_stream = create_api_key(
        &pool,
        &application.id.to_string(),
        Some(vec![
            "tokens:mint".to_string(),
            "sessions:read".to_string(),
            "stream:connect".to_string(),
        ]),
        Some("stream"),
        None,
    )
    .await
    .expect("key with stream:connect");
    let key_without_scope = create_api_key(
        &pool,
        &application.id.to_string(),
        Some(vec!["sessions:read".to_string()]),
        Some("no-scope"),
        None,
    )
    .await
    .expect("key without stream:connect");

    let kid = format!("ticket-http-{suffix}");
    let generated = generate_signing_key(&kid).expect("generate key");
    let signing = parse_private_pem(generated.private_pem()).expect("parse key");
    let mut keyset = DeviceTokenKeys::default();
    keyset.insert_signing_key(&kid, signing.clone());

    let mint_credential = key_stream.credential.to_string();
    let mint = |external: String, scopes: Vec<String>| {
        let pool = pool.clone();
        let signing = signing.clone();
        let kid = kid.clone();
        let credential = mint_credential.clone();
        async move {
            mint_device_token(
                &pool,
                credential.as_str(),
                &TokenRequest {
                    external_subject_id: external,
                    installation_id: "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d3a".to_string(),
                    platform: "ANDROID".to_string(),
                    app_version: None,
                    sdk_version: None,
                    scopes: Some(scopes),
                    ttl_seconds: Some(900),
                },
                TokenTtlBounds {
                    min_seconds: 300,
                    default_seconds: 900,
                    max_seconds: 3600,
                },
                ISSUER,
                &signing,
                &kid,
            )
            .await
            .expect("mint token")
        }
    };

    let device_token = mint(
        format!("ticket-http-{suffix}"),
        vec!["stream:connect".to_string()],
    )
    .await;
    let device_without_scope = mint(
        format!("ticket-http-noscope-{suffix}"),
        vec!["sessions:create".to_string()],
    )
    .await;

    let state = SessionApiState {
        pool,
        signing_keys: Arc::new(keyset),
        issuer: ISSUER.to_string(),
        leeway_seconds: 30,
        crypto_rotator: None,
        idempotency_retention: Duration::from_secs(3600),
        ephemeral: gs_service::InMemoryEphemeralStore::new(),
        bus: gs_service::InMemoryBus::new(),
        redis_key_prefix: format!("gs:ticket:{suffix}"),
        stream_ticket: StreamTicketConfig {
            ttl: Duration::from_secs(30),
            app_max_connection_age: Duration::from_secs(3600),
            public_base_url: "https://geostream.example.com".to_string(),
        },
        rate_limit_config: RateLimitConfig::default(),
    };

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);
    let tokio_listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("bind");
    let server = tokio::spawn(async move {
        axum::serve(tokio_listener, sessions_router(state))
            .await
            .expect("serve");
    });

    World {
        base: format!("http://127.0.0.1:{port}"),
        spec: load_spec(),
        _server: server,
        key_stream: key_stream.credential.to_string(),
        key_without_scope: key_without_scope.credential.to_string(),
        device_token: device_token.response.access_token.clone(),
        device_without_scope: device_without_scope.response.access_token.clone(),
    }
}

impl World {
    /// Calls `POST /v1/stream/tickets` and validates the response against the contract.
    async fn create_ticket(&self, token: Option<&str>) -> Reply {
        let client = reqwest::Client::new();
        let mut request = client.post(format!("{}/v1/stream/tickets", self.base));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.expect("send");
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let cache_control = response
            .headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let text = response.text().await.expect("text");
        let body = if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or_else(|_| panic!("non-JSON body: {text}"))
        };
        self.spec
            .check_response("createStreamTicket", status, &content_type, &body);
        Reply {
            status,
            body,
            content_type,
            cache_control,
            request_id,
        }
    }
}

#[tokio::test]
async fn test_fr_auth_006_application_key_gets_a_single_use_ticket() {
    // covers: FR-AUTH-006, docs/06 §5, docs/07 §9.1
    let world = setup("app").await;
    let reply = world.create_ticket(Some(&world.key_stream)).await;

    assert_eq!(reply.status, 201);
    assert_eq!(reply.content_type, "application/json");
    assert_eq!(reply.cache_control.as_deref(), Some("no-store"));
    assert!(
        reply
            .request_id
            .as_deref()
            .is_some_and(|id| id.starts_with("req_")),
        "X-Request-Id must be present: {:?}",
        reply.request_id
    );

    let ticket = reply.body["ticket"].as_str().expect("ticket");
    assert!(ticket.starts_with("gst_"), "ticket: {ticket}");
    assert_eq!(ticket.len(), 47);

    // `stream_url` is the documented WebSocket URL without the query string.
    assert_eq!(
        reply.body["stream_url"],
        json!("wss://geostream.example.com/v1/stream")
    );

    // `expires_at` is `now + GS_TICKET_TTL_S`.
    let expires_at = reply.body["expires_at"].as_str().expect("expires_at");
    let parsed =
        time::OffsetDateTime::parse(expires_at, &time::format_description::well_known::Rfc3339)
            .expect("RFC 3339 expires_at");
    let remaining = parsed - time::OffsetDateTime::now_utc();
    assert!(
        remaining <= time::Duration::seconds(TICKET_TTL_S)
            && remaining > time::Duration::seconds(TICKET_TTL_S - 5),
        "unexpected remaining life: {remaining:?}"
    );

    // Every call issues a different ticket.
    let second = world.create_ticket(Some(&world.key_stream)).await;
    assert_eq!(second.status, 201);
    assert_ne!(second.body["ticket"], reply.body["ticket"]);
}

#[tokio::test]
async fn test_fr_auth_006_device_token_gets_a_ticket() {
    // covers: FR-AUTH-006, docs/06 §5
    let world = setup("device").await;
    let reply = world.create_ticket(Some(&world.device_token)).await;
    assert_eq!(reply.status, 201);
    assert!(
        reply.body["ticket"]
            .as_str()
            .is_some_and(|t| t.starts_with("gst_"))
    );
}

#[tokio::test]
async fn test_missing_credential_is_unauthorized() {
    // covers: docs/08 §3, docs/07 §5
    let world = setup("anon").await;
    let reply = world.create_ticket(None).await;
    assert_eq!(reply.status, 401);
    assert_eq!(reply.body["code"], json!("UNAUTHENTICATED"));
    assert!(
        reply.content_type.starts_with("application/problem+json"),
        "errors must be Problem Details: {}",
        reply.content_type
    );
    assert!(reply.body["request_id"].is_string());
}

#[tokio::test]
async fn test_principals_without_stream_connect_are_forbidden() {
    // covers: FR-AUTH-006, docs/06 §8.2 (`x-gs-auth`)
    let world = setup("scope").await;

    let application = world.create_ticket(Some(&world.key_without_scope)).await;
    assert_eq!(application.status, 403);
    assert_eq!(application.body["code"], json!("INSUFFICIENT_SCOPE"));

    let device = world.create_ticket(Some(&world.device_without_scope)).await;
    assert_eq!(device.status, 403);
    assert_eq!(device.body["code"], json!("INSUFFICIENT_SCOPE"));
}
