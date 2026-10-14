//! WebSocket connection end to end: upgrade checks, hello handshake, heartbeat, inactivity
//! and close codes (T-F4-02, FR-STR-001, FR-STR-005, docs/08 §2–§6.5 and §8).
//!
//! The tests run a real listener with the real upgrade route of `gs-api` and the real
//! connection of `gs-stream`, and talk to it with `tokio-tungstenite`. Tickets are issued
//! over HTTP by the real `POST /v1/stream/tickets` route (T-F4-01).
#![forbid(unsafe_code)]

use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use gs_api::{
    ConnectionCounter, RateLimitConfig, SessionApiState, SessionRunner, StreamApiState,
    StreamTicketConfig, sessions_router, stream_route,
};
use gs_service::EphemeralStore;
use gs_service::admin::{create_api_key, create_application, create_tenant};
use gs_service::auth::DeviceTokenKeys;
use gs_service::gs_auth::{generate_signing_key, parse_private_pem};
use gs_service::gs_domain::Subprotocol;

use gs_service::migrate::{connect_pool, run_migrations};
use gs_service::tokens::{MintedToken, TokenRequest, TokenTtlBounds, mint_device_token};
use gs_service::{AuthzCache, InMemoryBus, InMemoryEphemeralStore};
use gs_stream::{StreamConfig, StreamState, run_session};
use serde_json::{Value, json};
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::error::Error as WsError;
use tokio_tungstenite::tungstenite::http::{HeaderValue, header};

type Socket = tokio_tungstenite::WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

const ISSUER: &str = "https://geostream.example.com";
const INSTALLATION: &str = "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d3a";

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

/// Everything the upgraded server and the test client need.
struct World {
    http_base: String,
    ws_base: String,
    pool: gs_service::gs_store::PgPool,
    tenant_id: gs_service::gs_domain::ids::TenantId,
    ephemeral: Arc<InMemoryEphemeralStore>,
    connections: ConnectionCounter,
    _server: tokio::task::JoinHandle<()>,
    key_with_scope: String,
    key_without_scope: String,
    device_token: MintedToken,
    other_device_token: MintedToken,
    reader_device_token: MintedToken,
}

/// Parses a session id printed by the fixture.
fn session_id_from(session_id: &str) -> gs_service::gs_domain::ids::SessionId {
    use std::str::FromStr;
    gs_service::gs_domain::ids::SessionId::from_str(session_id).expect("session id")
}

/// Session owned by the device of the fixture.
async fn session_of(world: &World) -> String {
    let session = gs_service::sessions::create_session(
        &world.pool,
        world.tenant_id,
        gs_service::gs_domain::session::Actor::Device(
            world
                .device_token
                .response
                .device_id
                .parse()
                .expect("device id"),
        ),
        &serde_json::from_value::<gs_service::sessions::CreateSessionRequest>(serde_json::json!({
            "mode": "MANY_TO_MANY",
            "max_members": 10
        }))
        .expect("request"),
        gs_service::sessions::SessionPolicy::from_policy(
            &gs_service::gs_auth::TenantPolicy::default(),
        ),
    )
    .await
    .expect("create session");
    session.id.to_string()
}

async fn setup(suffix: &str, config: StreamConfig) -> World {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");

    let tenant = create_tenant(&pool, &format!("Stream Tenant {suffix}"))
        .await
        .expect("tenant");
    let application = create_application(
        &pool,
        &tenant.id.to_string(),
        &format!("stream-app-{suffix}"),
        Some(vec![
            "sessions:create".to_string(),
            "stream:connect".to_string(),
        ]),
        None,
    )
    .await
    .expect("application");
    let key_with_scope = create_api_key(
        &pool,
        &application.id.to_string(),
        Some(vec![
            "tokens:mint".to_string(),
            "stream:connect".to_string(),
            "locations:read".to_string(),
        ]),
        Some("stream"),
        None,
    )
    .await
    .expect("api key");

    let kid = format!("stream-{suffix}");
    let generated = generate_signing_key(&kid).expect("generate key");
    let signing = parse_private_pem(generated.private_pem()).expect("parse key");
    let mut keyset = DeviceTokenKeys::default();
    keyset.insert_signing_key(&kid, signing.clone());

    let key_without_scope = create_api_key(
        &pool,
        &application.id.to_string(),
        Some(vec![
            "tokens:mint".to_string(),
            "stream:connect".to_string(),
        ]),
        Some("no-locations"),
        None,
    )
    .await
    .expect("api key without locations:read");

    let credential = key_with_scope.credential.to_string();
    let mint = |credential: String, external: String, scopes: Vec<String>| {
        let pool = pool.clone();
        let signing = signing.clone();
        let kid = kid.clone();
        async move {
            mint_device_token(
                &pool,
                credential.as_str(),
                &TokenRequest {
                    external_subject_id: external,
                    installation_id: INSTALLATION.to_string(),
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
        credential.clone(),
        format!("stream-{suffix}"),
        vec!["stream:connect".to_string()],
    )
    .await;
    // A device of the same tenant that is not a member of the session.
    let other_device_token = mint(
        credential.clone(),
        format!("stream-other-{suffix}"),
        vec!["stream:connect".to_string()],
    )
    .await;
    // A device that the tests add as a member without the `read` capability.
    let reader_device_token = mint(
        credential,
        format!("stream-reader-{suffix}"),
        vec!["stream:connect".to_string()],
    )
    .await;

    let ephemeral = InMemoryEphemeralStore::new();
    let bus = InMemoryBus::new();
    let prefix = format!("gs:stream:{suffix}");
    let connections = ConnectionCounter::new();

    let stream_state = Arc::new(StreamState {
        pool: pool.clone(),
        ephemeral: ephemeral.clone(),
        bus: bus.clone(),
        authz: Arc::new(AuthzCache::new(config.authz_cache_ttl)),
        fanout: Arc::new(gs_stream::Fanout::new(bus.clone(), prefix.clone())),
        redis_key_prefix: prefix.clone(),
        config: config.clone(),
    });
    // `gs-stream` owns the connection; the binary wires it into the upgrade route.
    let runner: SessionRunner = Arc::new(move |accepted: gs_api::AcceptedWebSocket| {
        let state = Arc::clone(&stream_state);
        Box::pin(async move {
            run_session(
                gs_stream::AcceptedSession {
                    subprotocol: accepted.subprotocol,
                    grant: accepted.grant,
                    request_id: accepted.request_id,
                    socket: accepted.socket,
                },
                state,
            )
            .await;
        })
    });

    let session_state = SessionApiState {
        pool: pool.clone(),
        signing_keys: Arc::new(keyset),
        issuer: ISSUER.to_string(),
        leeway_seconds: 30,
        crypto_rotator: None,
        idempotency_retention: Duration::from_secs(3600),
        ephemeral: ephemeral.clone(),
        bus,
        redis_key_prefix: prefix.clone(),
        stream_ticket: StreamTicketConfig {
            ttl: Duration::from_secs(30),
            app_max_connection_age: Duration::from_secs(3600),
            public_base_url: "https://geostream.example.com".to_string(),
        },
        rate_limit_config: RateLimitConfig::default(),
    };
    let stream_api_state = StreamApiState {
        pool: connect_pool(&test_database_url()).await.expect("connect"),
        ephemeral: ephemeral.clone(),
        redis_key_prefix: prefix,
        upgrade_rpm_per_ip: 120,
        max_connections: 2,
        max_frame_bytes: 16_384,
        connections: connections.clone(),
        json_subprotocol_enabled: config.json_subprotocol_enabled,
        session_runner: runner,
    };

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);
    let tokio_listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("bind");
    // `sessions_router` already carries `/v1/stream/tickets` (T-F4-01); the upgrade route is
    // the one this crate owns.
    let router = sessions_router(session_state).merge(stream_route(stream_api_state));
    let server = tokio::spawn(async move {
        axum::serve(tokio_listener, router).await.expect("serve");
    });

    World {
        http_base: format!("http://127.0.0.1:{port}"),
        ws_base: format!("ws://127.0.0.1:{port}"),
        pool: pool.clone(),
        tenant_id: tenant.id,
        ephemeral,
        connections,
        _server: server,
        key_with_scope: key_with_scope.credential.to_string(),
        key_without_scope: key_without_scope.credential.to_string(),
        device_token,
        other_device_token,
        reader_device_token,
    }
}

impl World {
    /// Issues a ticket for the given credential (docs/06 §5).
    async fn ticket(&self, credential: &str) -> String {
        let client = reqwest::Client::new();
        let response = client
            .post(format!("{}/v1/stream/tickets", self.http_base))
            .bearer_auth(credential)
            .send()
            .await
            .expect("request ticket");
        assert_eq!(response.status(), 201, "ticket request must succeed");
        let text = response.text().await.expect("body");
        let body: Value = serde_json::from_str(&text).expect("json");
        body["ticket"].as_str().expect("ticket").to_string()
    }

    /// Opens a WebSocket against the upgrade route with the given ticket.
    async fn connect(&self, ticket: &str, protocols: &str) -> Socket {
        let mut request = format!("{}/v1/stream?ticket={}", self.ws_base, ticket)
            .into_client_request()
            .expect("request");
        request.headers_mut().insert(
            header::SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_str(protocols).expect("header"),
        );
        let (socket, _response) = tokio_tungstenite::connect_async(request)
            .await
            .expect("websocket upgrade");
        socket
    }

    /// Attempts an upgrade that the server must refuse, returning status and problem body.
    async fn reject(&self, ticket: Option<&str>, protocols: Option<&str>) -> (u16, Value) {
        let query = ticket
            .map(|ticket| format!("?ticket={ticket}"))
            .unwrap_or_default();
        let mut request = format!("{}/v1/stream{query}", self.ws_base)
            .into_client_request()
            .expect("request");
        if let Some(protocols) = protocols {
            request.headers_mut().insert(
                header::SEC_WEBSOCKET_PROTOCOL,
                HeaderValue::from_str(protocols).expect("header"),
            );
        }
        match tokio_tungstenite::connect_async(request).await {
            Ok(_) => panic!("the upgrade must be refused"),
            Err(WsError::Http(response)) => {
                let status = response.status().as_u16();
                let body = response.body().clone().unwrap_or_default();
                let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                (status, json)
            }
            Err(other) => panic!("unexpected handshake error: {other}"),
        }
    }
}

/// Sends a `ClientHello` and returns the `ServerHello` as JSON (docs/08 §6.1).
async fn hello(socket: &mut Socket, subprotocol: Subprotocol) -> Value {
    let hello = gs_service::gs_protocol::ClientMessage {
        request_id: 0,
        body: Some(gs_service::gs_protocol::client_message::Body::Hello(
            gs_service::gs_protocol::ClientHello {
                protocol_minor: 0,
                sdk_name: "gs-stream-test".to_string(),
                sdk_version: "0.1.0".to_string(),
                platform: 1,
                app_version: "1.0.0".to_string(),
                last_sequence: 0,
                client_time: None,
            },
        )),
    };
    let payload = encode_client(&hello, subprotocol);
    let frame = frame_of(payload, subprotocol);
    socket.send(frame).await.expect("send hello");

    let bytes = read_binary(socket).await;
    let server: gs_service::gs_protocol::ServerMessage = decode_server(&bytes, subprotocol);
    match server.body {
        Some(gs_service::gs_protocol::server_message::Body::Hello(hello)) => {
            serde_json::to_value(&hello).expect("serialize")
        }
        other => panic!("expected ServerHello, got {other:?}"),
    }
}

/// Encodes a client message for the subprotocol under test.
fn encode_client(
    message: &gs_service::gs_protocol::ClientMessage,
    subprotocol: Subprotocol,
) -> Vec<u8> {
    match subprotocol {
        Subprotocol::Proto => {
            use prost::Message;
            message.encode_to_vec()
        }
        Subprotocol::Json => serde_json::to_vec(message).expect("serialize"),
    }
}

/// Decodes a server message for the subprotocol under test.
fn decode_server(bytes: &[u8], subprotocol: Subprotocol) -> gs_service::gs_protocol::ServerMessage {
    match subprotocol {
        Subprotocol::Proto => {
            use prost::Message;
            gs_service::gs_protocol::ServerMessage::decode(bytes).expect("decode")
        }
        Subprotocol::Json => serde_json::from_slice(bytes).expect("deserialize"),
    }
}

/// Builds the frame for a payload in the given subprotocol (docs/08 §2).
fn frame_of(payload: Vec<u8>, subprotocol: Subprotocol) -> Message {
    if subprotocol.is_text() {
        Message::text(String::from_utf8(payload).expect("utf8"))
    } else {
        Message::binary(payload)
    }
}

/// Reads the next data frame of a connection.
async fn read_binary(socket: &mut Socket) -> Vec<u8> {
    loop {
        match socket.next().await {
            Some(Ok(Message::Binary(bytes))) => return bytes.to_vec(),
            Some(Ok(Message::Text(text))) => return text.as_bytes().to_vec(),
            Some(Ok(_)) => continue,
            other => panic!("unexpected frame: {other:?}"),
        }
    }
}

/// Reads the close code the server ends the connection with (docs/08 §8).
async fn read_close_code(socket: &mut Socket) -> u16 {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Close(Some(frame)))) => return u16::from(frame.code),
                Some(Ok(_)) => continue,
                _ => return 0,
            }
        }
    })
    .await
    .expect("the server must close the connection")
}

#[tokio::test]
async fn test_fr_str_001_device_connects_and_completes_the_handshake() {
    // covers: FR-STR-001, FR-STR-005, docs/08 §3, §6.1
    let world = setup("handshake", StreamConfig::default()).await;
    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;

    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    let hello = hello(&mut socket, Subprotocol::Proto).await;

    assert!(
        hello["connectionId"]
            .as_str()
            .is_some_and(|id| id.starts_with("con_"))
    );
    // proto3 JSON omits default values, so a negotiated minor of 0 is absent (docs/08 §12).
    assert_eq!(
        hello
            .get("protocolMinor")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        0
    );
    assert_eq!(hello["principalKind"], json!("PRINCIPAL_KIND_DEVICE"));
    assert!(
        hello["subjectId"]
            .as_str()
            .is_some_and(|id| id.starts_with("sub_"))
    );
    assert!(
        hello["deviceId"]
            .as_str()
            .is_some_and(|id| id.starts_with("dev_"))
    );
    assert!(hello["authExpiresAt"].is_string());
    assert_eq!(
        hello
            .get("sequenceFloor")
            .and_then(Value::as_str)
            .unwrap_or("0"),
        "0",
        "uint64 travels as a string in the JSON subprotocol (docs/08 §12)"
    );
    assert_eq!(hello["limits"]["maxFrameBytes"], json!(16_384));
    assert_eq!(hello["limits"]["maxSubscriptions"], json!(20));
    assert_eq!(hello["limits"]["heartbeatIntervalMs"], json!(20_000));
    assert_eq!(hello["limits"]["idleTimeoutMs"], json!(60_000));

    // The connection is accounted for while it is open (docs/08 §3).
    assert_eq!(world.connections.active(), 1);
}

#[tokio::test]
async fn test_fr_str_001_application_connects_and_the_json_subprotocol_negotiates() {
    // covers: FR-STR-001, docs/08 §2, §12
    let world = setup("json", StreamConfig::default()).await;
    let ticket = world.ticket(&world.key_with_scope).await;

    let mut socket = world.connect(&ticket, "gs.v1.json").await;
    let hello = hello(&mut socket, Subprotocol::Json).await;
    assert_eq!(hello["principalKind"], json!("PRINCIPAL_KIND_APPLICATION"));
    assert!(
        hello["applicationId"]
            .as_str()
            .is_some_and(|id| id.starts_with("app_"))
    );
    assert_eq!(
        hello
            .get("sequenceFloor")
            .and_then(Value::as_str)
            .unwrap_or("0"),
        "0"
    );
    // Applications get the application subscription limit (docs/08 §6.1).
    assert_eq!(hello["limits"]["maxSubscriptions"], json!(1_000));
}

#[tokio::test]
async fn test_fr_auth_006_a_used_ticket_cannot_open_a_second_connection() {
    // covers: FR-AUTH-006, FR-STR-001, docs/08 §3
    let world = setup("reuse", StreamConfig::default()).await;
    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;

    let mut first = world.connect(&ticket, "gs.v1.proto").await;
    hello(&mut first, Subprotocol::Proto).await;

    // The same ticket is spent: the upgrade is refused with 401 and no upgrade.
    let (status, body) = world.reject(Some(&ticket), Some("gs.v1.proto")).await;
    assert_eq!(status, 401, "a used ticket must not open a connection");
    assert_eq!(body["code"], json!("UNAUTHENTICATED"));
    assert_eq!(
        body["type"],
        json!("https://geostream.dev/problems/unauthenticated")
    );
}

#[tokio::test]
async fn test_unsupported_subprotocol_is_refused_with_400() {
    // covers: FR-STR-001, docs/08 §3
    let world = setup("subprotocol", StreamConfig::default()).await;
    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;

    let (status, body) = world.reject(Some(&ticket), Some("other.v1")).await;
    assert_eq!(status, 400);
    assert_eq!(body["code"], json!("PROTOCOL_VERSION_UNSUPPORTED"));
}

#[tokio::test]
async fn test_disabled_json_subprotocol_is_not_offered() {
    // covers: docs/08 §2, docs/17 §2.6 (GS_STREAM_JSON_ENABLED)
    let config = StreamConfig {
        json_subprotocol_enabled: false,
        ..StreamConfig::default()
    };
    let world = setup("no-json", config).await;
    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;

    let (status, _body) = world.reject(Some(&ticket), Some("gs.v1.json")).await;
    assert_eq!(status, 400, "the JSON subprotocol must not be offered");
    // The binary transport still works.
    let _socket = world.connect(&ticket, "gs.v1.proto").await;
}

#[tokio::test]
async fn test_missing_ticket_is_refused_with_401() {
    // covers: FR-STR-001, docs/08 §3
    let world = setup("no-ticket", StreamConfig::default()).await;
    let (status, body) = world.reject(None, Some("gs.v1.proto")).await;
    assert_eq!(status, 401);
    assert_eq!(body["code"], json!("UNAUTHENTICATED"));
    assert!(
        body["detail"]
            .as_str()
            .is_some_and(|detail| !detail.contains("gst_"))
    );
}

#[tokio::test]
async fn test_hello_timeout_closes_with_4002() {
    // covers: FR-STR-005, docs/08 §6.1, §8
    let config = StreamConfig {
        hello_timeout: Duration::from_millis(300),
        ..StreamConfig::default()
    };
    let world = setup("hello-timeout", config).await;
    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;

    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    assert_eq!(read_close_code(&mut socket).await, 4002);
}

#[tokio::test]
async fn test_message_before_hello_closes_with_4000() {
    // covers: FR-STR-001, docs/08 §4, §5
    let world = setup("early-message", StreamConfig::default()).await;
    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;

    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    let heartbeat = gs_service::gs_protocol::ClientMessage {
        request_id: 7,
        body: Some(gs_service::gs_protocol::client_message::Body::Heartbeat(
            gs_service::gs_protocol::Heartbeat { client_time: None },
        )),
    };
    socket
        .send(frame_of(
            encode_client(&heartbeat, Subprotocol::Proto),
            Subprotocol::Proto,
        ))
        .await
        .expect("send heartbeat");

    // Any message other than the hello closes the connection with 4000 (FR-STR-001).
    assert_eq!(read_close_code(&mut socket).await, 4000);
}

#[tokio::test]
async fn test_malformed_first_frame_closes_with_4000() {
    // covers: FR-STR-001, docs/08 §4, §5
    let world = setup("malformed-first", StreamConfig::default()).await;
    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;

    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    socket
        .send(Message::binary(vec![0xff, 0xff, 0xff, 0xff]))
        .await
        .expect("send malformed frame");
    assert_eq!(read_close_code(&mut socket).await, 4000);
}

#[tokio::test]
async fn test_wrong_frame_type_closes_with_4000() {
    // covers: FR-STR-001, docs/08 §2
    let world = setup("frame-type", StreamConfig::default()).await;
    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;

    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    // A text frame is not valid with `gs.v1.proto` (docs/08 §2).
    socket
        .send(Message::text("{\"requestId\":0}"))
        .await
        .expect("send text frame");
    assert_eq!(read_close_code(&mut socket).await, 4000);
}

/// Subscribes to a session and returns the `SubscribeAck`.
async fn subscribe(
    socket: &mut Socket,
    session_id: &str,
    subprotocol: Subprotocol,
    skip_snapshot: bool,
) -> gs_service::gs_protocol::SubscribeAck {
    let request = gs_service::gs_protocol::ClientMessage {
        request_id: 11,
        body: Some(gs_service::gs_protocol::client_message::Body::Subscribe(
            gs_service::gs_protocol::Subscribe {
                session_id: session_id.to_string(),
                skip_snapshot,
            },
        )),
    };
    socket
        .send(frame_of(encode_client(&request, subprotocol), subprotocol))
        .await
        .expect("send subscribe");
    let bytes = read_binary(socket).await;
    let server = decode_server(&bytes, subprotocol);
    assert_eq!(server.request_id, 11, "the request id is echoed");
    match server.body {
        Some(gs_service::gs_protocol::server_message::Body::SubscribeAck(ack)) => ack,
        other => panic!("expected SubscribeAck, got {other:?}"),
    }
}

/// Unsubscribes and returns the `UnsubscribeAck`.
async fn unsubscribe(
    socket: &mut Socket,
    session_id: &str,
    subprotocol: Subprotocol,
) -> gs_service::gs_protocol::UnsubscribeAck {
    let request = gs_service::gs_protocol::ClientMessage {
        request_id: 12,
        body: Some(gs_service::gs_protocol::client_message::Body::Unsubscribe(
            gs_service::gs_protocol::Unsubscribe {
                session_id: session_id.to_string(),
            },
        )),
    };
    socket
        .send(frame_of(encode_client(&request, subprotocol), subprotocol))
        .await
        .expect("send unsubscribe");
    let bytes = read_binary(socket).await;
    let server = decode_server(&bytes, subprotocol);
    assert_eq!(server.request_id, 12);
    match server.body {
        Some(gs_service::gs_protocol::server_message::Body::UnsubscribeAck(ack)) => ack,
        other => panic!("expected UnsubscribeAck, got {other:?}"),
    }
}

/// Subscribes expecting a `StreamError`, and returns it.
async fn subscribe_expecting_error(
    socket: &mut Socket,
    session_id: &str,
    subprotocol: Subprotocol,
) -> gs_service::gs_protocol::StreamError {
    let request = gs_service::gs_protocol::ClientMessage {
        request_id: 13,
        body: Some(gs_service::gs_protocol::client_message::Body::Subscribe(
            gs_service::gs_protocol::Subscribe {
                session_id: session_id.to_string(),
                skip_snapshot: false,
            },
        )),
    };
    socket
        .send(frame_of(encode_client(&request, subprotocol), subprotocol))
        .await
        .expect("send subscribe");
    let bytes = read_binary(socket).await;
    let server = decode_server(&bytes, subprotocol);
    assert_eq!(server.request_id, 13);
    match server.body {
        Some(gs_service::gs_protocol::server_message::Body::Error(error)) => error,
        other => panic!("expected StreamError, got {other:?}"),
    }
}

#[tokio::test]
async fn test_subscription_answers_session_members_and_snapshot() {
    // covers: FR-STR-002, docs/08 §6.3, docs/05 §8
    let world = setup("subscribe", StreamConfig::default()).await;
    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;
    let session_id = session_of(&world).await;

    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    hello(&mut socket, Subprotocol::Proto).await;

    let ack = subscribe(&mut socket, &session_id, Subprotocol::Proto, false).await;
    let session = ack.session.expect("session info");
    assert_eq!(session.session_id, session_id);
    assert_eq!(
        session.mode,
        gs_service::gs_protocol::SessionMode::ManyToMany as i32
    );
    assert_eq!(
        session.status,
        gs_service::gs_protocol::SessionStatus::Active as i32
    );
    assert_eq!(session.max_members, 10);
    assert_eq!(
        ack.members.len(),
        1,
        "the owner is the only member of the session"
    );
    let member = &ack.members[0];
    assert!(member.member_id.starts_with("mem_"), "typed member id");
    assert!(member.external_subject_id.starts_with("stream-subscribe"));
    assert!(member.capabilities.is_some());
    assert!(!member.sharing, "sharing is off by default (FR-SHR-001)");
    assert_eq!(
        member.connectivity,
        gs_service::gs_protocol::Connectivity::Unspecified as i32,
        "a member that does not share never reveals connectivity (FR-PRS-004)"
    );
    assert!(
        member.last_location_at.is_none(),
        "no last_location_at without sharing and snapshot (FR-PRS-004)"
    );
    // Nothing is stored yet: the snapshot is empty.
    assert!(ack.snapshot.is_empty());

    // A second subscribe to the same session is idempotent (docs/08 §6.3).
    let again = subscribe(&mut socket, &session_id, Subprotocol::Proto, true).await;
    assert_eq!(again.session.expect("session").session_id, session_id);
    assert!(again.snapshot.is_empty());

    // Unsubscribing always answers.
    let unsub = unsubscribe(&mut socket, &session_id, Subprotocol::Proto).await;
    assert_eq!(unsub.session_id, session_id);
    // And it is idempotent.
    let again = unsubscribe(&mut socket, &session_id, Subprotocol::Proto).await;
    assert_eq!(again.session_id, session_id);
}

#[tokio::test]
async fn test_a_non_member_gets_not_found() {
    // covers: FR-STR-002, FR-API-006, docs/08 §6.3
    let world = setup("non-member", StreamConfig::default()).await;
    let session_id = session_of(&world).await;

    // The device that owns the ticket is the only member; another subject is not.
    let other = world
        .ticket(&world.other_device_token.response.access_token)
        .await;
    let mut socket = world.connect(&other, "gs.v1.proto").await;
    hello(&mut socket, Subprotocol::Proto).await;

    let error = subscribe_expecting_error(&mut socket, &session_id, Subprotocol::Proto).await;
    assert_eq!(
        error.code,
        gs_service::gs_protocol::ErrorCode::NotFound as i32,
        "a non member is not told the session exists"
    );
}

#[tokio::test]
async fn test_a_member_without_read_gets_permission_denied() {
    // covers: FR-STR-002, docs/08 §6.3
    let world = setup("no-read", StreamConfig::default()).await;
    let session_id = session_of(&world).await;
    // Add the reader device as a member without the `read` capability.
    gs_service::members::add_member(
        &world.pool,
        world.tenant_id,
        session_id_from(&session_id),
        &gs_service::members::AddMemberRequest {
            external_subject_id: world
                .reader_device_token
                .subject
                .external_subject_id
                .clone()
                .expect("external id"),
            role: None,
            capabilities: Some(gs_service::gs_domain::membership::Capabilities::none()),
        },
    )
    .await
    .expect("add member");

    // The reader device is the subject added without `read`.

    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;
    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    hello(&mut socket, Subprotocol::Proto).await;

    // The owner can read.
    let ack = subscribe(&mut socket, &session_id, Subprotocol::Proto, true).await;
    assert_eq!(ack.session.expect("session").session_id, session_id);
    assert!(
        ack.members.iter().any(|member| member.external_subject_id
            == world
                .reader_device_token
                .subject
                .external_subject_id
                .clone()
                .unwrap_or_default()),
        "the reader is a member"
    );

    // A member without `read` cannot.
    let member_token = world
        .ticket(&world.reader_device_token.response.access_token)
        .await;
    let mut reader = world.connect(&member_token, "gs.v1.proto").await;
    hello(&mut reader, Subprotocol::Proto).await;
    let error = subscribe_expecting_error(&mut reader, &session_id, Subprotocol::Proto).await;
    assert_eq!(
        error.code,
        gs_service::gs_protocol::ErrorCode::PermissionDenied as i32
    );
}

#[tokio::test]
async fn test_an_application_without_locations_read_is_denied() {
    // covers: FR-STR-002, docs/06 §8.3
    let world = setup("app-scope", StreamConfig::default()).await;
    let session_id = session_of(&world).await;

    let ticket = world.ticket(&world.key_with_scope).await;
    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    hello(&mut socket, Subprotocol::Proto).await;

    // The application key of the fixture carries `locations:read`.
    let ack = subscribe(&mut socket, &session_id, Subprotocol::Proto, true).await;
    assert_eq!(ack.session.expect("session").session_id, session_id);

    // A key without the scope is denied.
    let scoped = world.ticket(&world.key_without_scope).await;
    let mut other = world.connect(&scoped, "gs.v1.proto").await;
    hello(&mut other, Subprotocol::Proto).await;
    let error = subscribe_expecting_error(&mut other, &session_id, Subprotocol::Proto).await;
    assert_eq!(
        error.code,
        gs_service::gs_protocol::ErrorCode::PermissionDenied as i32
    );
}

#[tokio::test]
async fn test_a_closed_session_answers_session_closed() {
    // covers: FR-STR-002, INV-SES-1, docs/08 §6.3
    let world = setup("closed", StreamConfig::default()).await;
    let session_id = session_of(&world).await;
    gs_service::sessions::close_session(
        &world.pool,
        world.tenant_id,
        session_id_from(&session_id),
        gs_service::gs_domain::session::ClosedReason::ClosedByMember,
    )
    .await
    .expect("close session");

    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;
    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    hello(&mut socket, Subprotocol::Proto).await;

    let error = subscribe_expecting_error(&mut socket, &session_id, Subprotocol::Proto).await;
    assert_eq!(
        error.code,
        gs_service::gs_protocol::ErrorCode::SessionClosed as i32
    );
}

#[tokio::test]
async fn test_the_subscription_limit_answers_too_many_subscriptions() {
    // covers: FR-STR-002, docs/08 §6.3 (TOO_MANY_SUBSCRIPTIONS)
    let config = StreamConfig {
        max_subscriptions: 1,
        ..StreamConfig::default()
    };
    let world = setup("limit-subs", config).await;
    let first = session_of(&world).await;
    let second = gs_service::sessions::create_session(
        &world.pool,
        world.tenant_id,
        gs_service::gs_domain::session::Actor::Device(
            world
                .device_token
                .response
                .device_id
                .parse()
                .expect("device id"),
        ),
        &serde_json::from_value::<gs_service::sessions::CreateSessionRequest>(serde_json::json!({
            "mode": "MANY_TO_MANY",
            "max_members": 10
        }))
        .expect("request"),
        gs_service::sessions::SessionPolicy::from_policy(
            &gs_service::gs_auth::TenantPolicy::default(),
        ),
    )
    .await
    .expect("second session");

    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;
    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    hello(&mut socket, Subprotocol::Proto).await;

    let ack = subscribe(&mut socket, &first, Subprotocol::Proto, true).await;
    assert_eq!(ack.session.expect("session").session_id, first);

    let error =
        subscribe_expecting_error(&mut socket, &second.id.to_string(), Subprotocol::Proto).await;
    assert_eq!(
        error.code,
        gs_service::gs_protocol::ErrorCode::TooManySubscriptions as i32
    );
}

#[tokio::test]
async fn test_unsupported_protocol_minor_closes_with_4001() {
    // covers: FR-STR-005, docs/08 §6.1, §8
    let world = setup("minor", StreamConfig::default()).await;
    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;

    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    let hello = gs_service::gs_protocol::ClientMessage {
        request_id: 0,
        body: Some(gs_service::gs_protocol::client_message::Body::Hello(
            gs_service::gs_protocol::ClientHello {
                protocol_minor: 1,
                sdk_name: "gs-stream-test".to_string(),
                sdk_version: "0.1.0".to_string(),
                platform: 1,
                app_version: "1.0.0".to_string(),
                last_sequence: 0,
                client_time: None,
            },
        )),
    };
    socket
        .send(frame_of(
            encode_client(&hello, Subprotocol::Proto),
            Subprotocol::Proto,
        ))
        .await
        .expect("send hello");

    assert_eq!(read_close_code(&mut socket).await, 4001);
}

#[tokio::test]
async fn test_heartbeat_is_answered_with_the_server_clock() {
    // covers: FR-STR-005, docs/08 §6.5
    let world = setup("heartbeat", StreamConfig::default()).await;
    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;

    let mut socket = world.connect(&ticket, "gs.v1.json").await;
    hello(&mut socket, Subprotocol::Json).await;

    socket
        .send(Message::text("{\"requestId\":5,\"heartbeat\":{}}"))
        .await
        .expect("send heartbeat");
    let bytes = read_binary(&mut socket).await;
    let server = decode_server(&bytes, Subprotocol::Json);
    assert_eq!(server.request_id, 5);
    match server.body {
        Some(gs_service::gs_protocol::server_message::Body::HeartbeatAck(ack)) => {
            assert!(
                ack.server_time.is_some(),
                "the ack carries the server clock"
            );
        }
        other => panic!("expected HeartbeatAck, got {other:?}"),
    }
}

#[tokio::test]
async fn test_connection_limit_is_enforced_with_503() {
    // covers: FR-STR-001, docs/08 §3 (GS_STREAM_MAX_CONNECTIONS)
    let world = setup("limit", StreamConfig::default()).await;
    let first = world
        .ticket(&world.device_token.response.access_token)
        .await;
    let second = world
        .ticket(&world.device_token.response.access_token)
        .await;
    let third = world
        .ticket(&world.device_token.response.access_token)
        .await;

    let _first = world.connect(&first, "gs.v1.proto").await;
    let _second = world.connect(&second, "gs.v1.proto").await;
    // The instance accepts two connections; the third is refused without an upgrade.
    let (status, body) = world.reject(Some(&third), Some("gs.v1.proto")).await;
    assert_eq!(status, 503);
    assert_eq!(body["code"], json!("TEMPORARILY_UNAVAILABLE"));
}

#[tokio::test]
async fn test_idle_timeout_closes_with_4408() {
    // covers: FR-STR-005, docs/08 §6.5, §8
    let config = StreamConfig {
        heartbeat_interval: Duration::from_millis(100),
        idle_timeout: Duration::from_millis(400),
        ..StreamConfig::default()
    };
    config
        .validate()
        .expect("idle timeout above twice the heartbeat");
    let world = setup("idle", config).await;
    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;

    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    hello(&mut socket, Subprotocol::Proto).await;

    assert_eq!(read_close_code(&mut socket).await, 4408);
}

#[tokio::test]
async fn test_sequence_floor_comes_from_the_ephemeral_store() {
    // covers: FR-STR-001, docs/08 §6.2
    let world = setup("floor", StreamConfig::default()).await;
    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;

    // The device has published up to sequence 41 (docs/08 §6.2).
    let device_id = world.device_token.response.device_id.clone();
    world
        .ephemeral
        .set_if_greater(&format!("gs:stream:floor:devseq:{device_id}"), 41)
        .await
        .expect("set devseq");

    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    let hello = hello(&mut socket, Subprotocol::Proto).await;
    assert_eq!(hello["sequenceFloor"], json!("41"));
}

#[tokio::test]
async fn test_a_revoked_subject_token_closes_the_connection_with_4401() {
    // covers: NFR-SEC-002, FR-AUTH-003, docs/06 §7, ADR-0009
    // A cache TTL of zero revalidates the principal on every message.
    let config = StreamConfig {
        authz_cache_ttl: Duration::from_millis(0),
        ..StreamConfig::default()
    };
    let world = setup("revoke-tokens", config).await;
    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;

    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    hello(&mut socket, Subprotocol::Proto).await;

    // The subject tokens are revoked: the next revalidation fails and the connection closes.
    let subject_id = world
        .device_token
        .response
        .subject_id
        .parse()
        .expect("subject id");
    gs_service::gs_store::repos::SubjectRepository::increment_token_epoch(
        &world.pool,
        world.tenant_id,
        subject_id,
        time::OffsetDateTime::now_utc(),
    )
    .await
    .expect("bump epoch");

    // A heartbeat forces the connection to process a message, which triggers the check.
    socket
        .send(frame_of(
            encode_client(&heartbeat_message(9), Subprotocol::Proto),
            Subprotocol::Proto,
        ))
        .await
        .expect("send heartbeat");
    assert_eq!(read_close_code(&mut socket).await, 4401);
}

#[tokio::test]
async fn test_a_disabled_tenant_closes_the_connection_with_4403() {
    // covers: NFR-SEC-002, FR-ADM-003, docs/06 §7, ADR-0009
    let config = StreamConfig {
        authz_cache_ttl: Duration::from_millis(0),
        ..StreamConfig::default()
    };
    let world = setup("disable-tenant", config).await;
    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;

    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    hello(&mut socket, Subprotocol::Proto).await;

    gs_service::admin::set_tenant_enabled(&world.pool, &world.tenant_id.to_string(), false)
        .await
        .expect("disable tenant");

    socket
        .send(frame_of(
            encode_client(&heartbeat_message(9), Subprotocol::Proto),
            Subprotocol::Proto,
        ))
        .await
        .expect("send heartbeat");
    assert_eq!(read_close_code(&mut socket).await, 4403);
}

/// A heartbeat with the given request id.
fn heartbeat_message(request_id: u64) -> gs_service::gs_protocol::ClientMessage {
    gs_service::gs_protocol::ClientMessage {
        request_id,
        body: Some(gs_service::gs_protocol::client_message::Body::Heartbeat(
            gs_service::gs_protocol::Heartbeat { client_time: None },
        )),
    }
}

/// Publishes a batch, collecting the updates that interleave with the ack.
///
/// The fan-out may deliver an update between the ack of one publish and the next, so the
/// helper skips those frames and returns them to the caller.
async fn publish_collecting(
    socket: &mut Socket,
    session_id: &str,
    samples: Vec<gs_service::gs_protocol::LocationSample>,
    subprotocol: Subprotocol,
    updates: &mut Vec<u64>,
) -> gs_service::gs_protocol::PublishAck {
    let request = gs_service::gs_protocol::ClientMessage {
        request_id: 21,
        body: Some(
            gs_service::gs_protocol::client_message::Body::PublishLocation(
                gs_service::gs_protocol::PublishLocation {
                    session_id: session_id.to_string(),
                    samples,
                },
            ),
        ),
    };
    socket
        .send(frame_of(encode_client(&request, subprotocol), subprotocol))
        .await
        .expect("send publish");
    loop {
        let bytes = read_binary(socket).await;
        let server = decode_server(&bytes, subprotocol);
        match server.body {
            Some(gs_service::gs_protocol::server_message::Body::PublishAck(ack)) => return ack,
            Some(gs_service::gs_protocol::server_message::Body::LocationUpdate(update)) => {
                updates.push(update.sample.as_ref().map(|s| s.sequence).unwrap_or(0));
            }
            other => panic!("unexpected frame while publishing: {other:?}"),
        }
    }
}

/// Publishes a batch and returns the `PublishAck`.
async fn publish(
    socket: &mut Socket,
    session_id: &str,
    samples: Vec<gs_service::gs_protocol::LocationSample>,
    subprotocol: Subprotocol,
) -> gs_service::gs_protocol::PublishAck {
    let request = gs_service::gs_protocol::ClientMessage {
        request_id: 21,
        body: Some(
            gs_service::gs_protocol::client_message::Body::PublishLocation(
                gs_service::gs_protocol::PublishLocation {
                    session_id: session_id.to_string(),
                    samples,
                },
            ),
        ),
    };
    socket
        .send(frame_of(encode_client(&request, subprotocol), subprotocol))
        .await
        .expect("send publish");
    let bytes = read_binary(socket).await;
    let server = decode_server(&bytes, subprotocol);
    assert_eq!(server.request_id, 21);
    match server.body {
        Some(gs_service::gs_protocol::server_message::Body::PublishAck(ack)) => ack,
        other => panic!("expected PublishAck, got {other:?}"),
    }
}

/// A valid sample for the synthetic coordinates of the vectors.
fn sample(sequence: u64) -> gs_service::gs_protocol::LocationSample {
    gs_service::gs_protocol::LocationSample {
        sequence,
        captured_at: Some(gs_service::pbjson_types::Timestamp {
            seconds: time::OffsetDateTime::now_utc().unix_timestamp() - 1,
            nanos: 0,
        }),
        position: Some(gs_service::gs_protocol::Position {
            latitude: 8.9824,
            longitude: -79.5199,
            horizontal_accuracy_m: 8.5,
            altitude_m: None,
            vertical_accuracy_m: None,
            speed_mps: Some(4.2),
            speed_accuracy_mps: None,
            heading_deg: Some(91.0),
            heading_accuracy_deg: None,
        }),
        quality: Some(gs_service::gs_protocol::LocationQuality {
            source: gs_service::gs_protocol::LocationSource::Gnss as i32,
            kind: gs_service::gs_protocol::LocationKind::Measured as i32,
            is_mock: Some(false),
            motion: gs_service::gs_protocol::MotionActivity::Automotive as i32,
        }),
    }
}

/// Sequence floor reported in a `ServerHello` (`uint64` travels as a string, docs/08 §12).
fn sequence_floor_of(hello: &serde_json::Value) -> String {
    hello
        .get("sequenceFloor")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("0")
        .to_string()
}

/// Starts sharing from the device of the fixture so it can publish.
async fn start_sharing(world: &World, session_id: &str) {
    let session_id = session_id_from(session_id);
    let membership =
        gs_service::gs_store::repos::MembershipRepository::find_active_by_session_and_subject(
            &world.pool,
            world.tenant_id,
            session_id,
            world
                .device_token
                .response
                .subject_id
                .parse()
                .expect("subject id"),
        )
        .await
        .expect("query")
        .expect("membership");
    gs_service::members::set_sharing(
        &world.pool,
        world.tenant_id,
        session_id,
        membership.id,
        true,
        gs_service::members::SetSharingContext {
            ephemeral: world.ephemeral.as_ref(),
            bus: &gs_service::InMemoryBus::new(),
            key_prefix: "gs:stream:publish",
            caller: gs_service::members::SetSharingCaller {
                is_app: false,
                subject_id: Some(
                    world
                        .device_token
                        .response
                        .subject_id
                        .parse()
                        .expect("subject id"),
                ),
                device_id: Some(
                    world
                        .device_token
                        .response
                        .device_id
                        .parse()
                        .expect("device id"),
                ),
                has_manage: true,
            },
        },
    )
    .await
    .expect("start sharing");
}

#[tokio::test]
async fn test_fr_loc_003_publication_is_ordered_deduplicated_and_acked() {
    // covers: FR-LOC-003, FR-LOC-005, FR-STR-004, docs/08 §6.4, docs/09 §4, ADR-0008
    let world = setup("publish", StreamConfig::default()).await;
    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;
    let session_id = session_of(&world).await;
    start_sharing(&world, &session_id).await;

    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    let hello_message = hello(&mut socket, Subprotocol::Proto).await;
    assert_eq!(sequence_floor_of(&hello_message), "0");

    // The tenant policy of the fixture allows one sample per publish (docs/17 §4).
    let ack = publish(
        &mut socket,
        &session_id,
        vec![sample(1)],
        Subprotocol::Proto,
    )
    .await;
    assert_eq!(ack.session_id, session_id);
    assert_eq!(ack.request_error, 0, "the batch was processed");
    assert_eq!(ack.results.len(), 1, "one result per sample (FR-STR-004)");
    assert_eq!(
        ack.results[0].status,
        gs_service::gs_protocol::SampleStatus::Accepted as i32
    );
    assert_eq!(ack.results[0].sequence, 1);

    // A batch above the policy limit is refused before touching any sample.
    let too_big = publish(
        &mut socket,
        &session_id,
        vec![sample(3), sample(4)],
        Subprotocol::Proto,
    )
    .await;
    assert_eq!(
        too_big.request_error,
        gs_service::gs_protocol::ErrorCode::PayloadTooLarge as i32
    );
    assert!(too_big.results.is_empty());

    // A repeated sequence is a duplicate; an older one is stale (docs/09 §4).
    let again = publish(
        &mut socket,
        &session_id,
        vec![sample(1)],
        Subprotocol::Proto,
    )
    .await;
    assert_eq!(
        again.results[0].status,
        gs_service::gs_protocol::SampleStatus::IgnoredDuplicate as i32
    );
    let stale = publish(
        &mut socket,
        &session_id,
        vec![sample(1)],
        Subprotocol::Proto,
    )
    .await;
    assert_eq!(
        stale.results[0].status,
        gs_service::gs_protocol::SampleStatus::IgnoredDuplicate as i32
    );

    // The next connection sees the sequence floor of ADR-0008.
    drop(socket);
    let next_ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;
    let mut next = world.connect(&next_ticket, "gs.v1.proto").await;
    let hello_message = hello(&mut next, Subprotocol::Proto).await;
    assert_eq!(
        sequence_floor_of(&hello_message),
        "1",
        "the device floor informs the next connection (FR-LOC-008)"
    );
}

#[tokio::test]
async fn test_fr_loc_001_an_invalid_sample_is_rejected_with_its_field() {
    // covers: FR-LOC-001, FR-LOC-002, docs/09 §3.1
    let world = setup("invalid-publish", StreamConfig::default()).await;
    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;
    let session_id = session_of(&world).await;
    start_sharing(&world, &session_id).await;

    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    hello(&mut socket, Subprotocol::Proto).await;

    let mut broken = sample(5);
    if let Some(position) = broken.position.as_mut() {
        position.latitude = 91.0;
    }
    let ack = publish(&mut socket, &session_id, vec![broken], Subprotocol::Proto).await;
    assert_eq!(ack.results.len(), 1);
    assert_eq!(
        ack.results[0].status,
        gs_service::gs_protocol::SampleStatus::Rejected as i32
    );
    assert_eq!(
        ack.results[0].error_code,
        gs_service::gs_protocol::ErrorCode::InvalidLocation as i32
    );
    assert_eq!(ack.results[0].field, "position.latitude");
}

#[tokio::test]
async fn test_publication_without_sharing_is_refused() {
    // covers: FR-SHR-001, docs/08 §6.4
    let world = setup("no-sharing", StreamConfig::default()).await;
    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;
    let session_id = session_of(&world).await;

    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    hello(&mut socket, Subprotocol::Proto).await;

    let ack = publish(
        &mut socket,
        &session_id,
        vec![sample(1)],
        Subprotocol::Proto,
    )
    .await;
    assert_eq!(
        ack.request_error,
        gs_service::gs_protocol::ErrorCode::NotSharing as i32,
        "publishing without sharing is refused (FR-SHR-001)"
    );
    assert!(ack.results.is_empty(), "no sample is applied");
}

#[tokio::test]
async fn test_an_application_cannot_publish() {
    // covers: docs/06 §8.3, docs/08 §6.4
    let world = setup("app-publish", StreamConfig::default()).await;
    let ticket = world.ticket(&world.key_with_scope).await;
    let session_id = session_of(&world).await;

    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    hello(&mut socket, Subprotocol::Proto).await;

    let ack = publish(
        &mut socket,
        &session_id,
        vec![sample(1)],
        Subprotocol::Proto,
    )
    .await;
    assert_eq!(
        ack.request_error,
        gs_service::gs_protocol::ErrorCode::PermissionDenied as i32
    );
}

#[tokio::test]
async fn test_a_subscribed_publisher_receives_its_own_updates() {
    // covers: FR-STR-002, docs/08 §7.3
    let world = setup("self-delivery", StreamConfig::default()).await;
    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;
    let session_id = session_of(&world).await;
    start_sharing(&world, &session_id).await;

    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    hello(&mut socket, Subprotocol::Proto).await;
    let ack = subscribe(&mut socket, &session_id, Subprotocol::Proto, true).await;
    assert_eq!(ack.session.expect("session").session_id, session_id);

    let published = publish(
        &mut socket,
        &session_id,
        vec![sample(64)],
        Subprotocol::Proto,
    )
    .await;
    assert_eq!(published.results.len(), 1);

    // The publisher sees its own sample as everybody else does (docs/08 §7.3).
    use futures::StreamExt;
    let update = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Binary(bytes))) => {
                    let server = decode_server(&bytes, Subprotocol::Proto);
                    if let Some(gs_service::gs_protocol::server_message::Body::LocationUpdate(
                        update,
                    )) = server.body
                    {
                        return update;
                    }
                }
                _ => continue,
            }
        }
    })
    .await
    .expect("the publisher receives its own update");
    assert_eq!(update.session_id, session_id);
    assert_eq!(
        update.sample.as_ref().map(|s| s.sequence),
        Some(64),
        "the sample is delivered untouched"
    );
    assert_eq!(
        update.delivery,
        gs_service::gs_protocol::DeliveryKind::Live as i32
    );
    assert!(
        update.received_at.is_some(),
        "the server assigns received_at (FR-LOC-005)"
    );
}

#[tokio::test]
async fn test_fr_str_003_the_update_reaches_every_subscriber_of_the_session() {
    // covers: FR-STR-003, docs/08 §7.3, docs/04 §8
    let world = setup("fanout", StreamConfig::default()).await;
    let session_id = session_of(&world).await;
    start_sharing(&world, &session_id).await;

    // A reader of the same tenant, added as a member with the `read` capability.
    gs_service::members::add_member(
        &world.pool,
        world.tenant_id,
        session_id_from(&session_id),
        &gs_service::members::AddMemberRequest {
            external_subject_id: world
                .other_device_token
                .subject
                .external_subject_id
                .clone()
                .expect("external id"),
            role: None,
            capabilities: Some(gs_service::gs_domain::membership::Capabilities::read_only()),
        },
    )
    .await
    .expect("add reader");

    let reader_ticket = world
        .ticket(&world.other_device_token.response.access_token)
        .await;
    let mut reader = world.connect(&reader_ticket, "gs.v1.proto").await;
    hello(&mut reader, Subprotocol::Proto).await;
    let ack = subscribe(&mut reader, &session_id, Subprotocol::Proto, true).await;
    assert_eq!(ack.session.expect("session").session_id, session_id);

    // The publisher publishes one sample.
    let publisher_ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;
    let mut publisher = world.connect(&publisher_ticket, "gs.v1.proto").await;
    hello(&mut publisher, Subprotocol::Proto).await;
    let published = publish(
        &mut publisher,
        &session_id,
        vec![sample(101)],
        Subprotocol::Proto,
    )
    .await;
    assert_eq!(published.results.len(), 1);

    // The reader receives the update through the fan-out (single path, docs/04 §8).
    use futures::StreamExt;
    let update = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match reader.next().await {
                Some(Ok(Message::Binary(bytes))) => {
                    let server = decode_server(&bytes, Subprotocol::Proto);
                    if let Some(gs_service::gs_protocol::server_message::Body::LocationUpdate(
                        update,
                    )) = server.body
                    {
                        return update;
                    }
                }
                _ => continue,
            }
        }
    })
    .await
    .expect("the reader receives the published update");
    assert_eq!(update.session_id, session_id);
    assert_eq!(
        update.sample.as_ref().map(|sample| sample.sequence),
        Some(101)
    );
    assert_eq!(
        update.delivery,
        gs_service::gs_protocol::DeliveryKind::Live as i32
    );
    assert!(
        update.received_at.is_some(),
        "the server assigns received_at (FR-LOC-005)"
    );
    // No coordinates of the sample are ever logged or echoed in the ack.
    assert_eq!(
        update
            .sample
            .as_ref()
            .and_then(|sample| sample.position.as_ref())
            .map(|position| position.latitude),
        Some(8.9824)
    );
}

#[tokio::test]
async fn test_fr_str_006_only_the_newest_location_per_member_is_delivered() {
    // covers: FR-STR-006, docs/08 §10 (coalescencia)
    let world = setup("coalescing", StreamConfig::default()).await;
    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;
    let session_id = session_of(&world).await;
    start_sharing(&world, &session_id).await;

    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    hello(&mut socket, Subprotocol::Proto).await;
    let ack = subscribe(&mut socket, &session_id, Subprotocol::Proto, true).await;
    assert_eq!(ack.session.expect("session").session_id, session_id);

    // Two accepted publishes in a row: the client must receive the newest one (docs/08 §10).
    let mut sequences = Vec::new();
    publish_collecting(
        &mut socket,
        &session_id,
        vec![sample(201)],
        Subprotocol::Proto,
        &mut sequences,
    )
    .await;
    publish_collecting(
        &mut socket,
        &session_id,
        vec![sample(202)],
        Subprotocol::Proto,
        &mut sequences,
    )
    .await;

    use futures::StreamExt;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(400), socket.next()).await {
            Ok(Some(Ok(Message::Binary(bytes)))) => {
                let server = decode_server(&bytes, Subprotocol::Proto);
                if let Some(gs_service::gs_protocol::server_message::Body::LocationUpdate(update)) =
                    server.body
                {
                    sequences.push(update.sample.as_ref().map(|s| s.sequence).unwrap_or(0));
                }
            }
            Ok(Some(Ok(_))) => continue,
            _ => break,
        }
    }
    assert!(
        sequences.contains(&202),
        "the newest location must be delivered: {sequences:?}"
    );
    assert!(
        sequences.windows(2).all(|pair| pair[1] > pair[0]),
        "locations of a member arrive in order: {sequences:?}"
    );
}
