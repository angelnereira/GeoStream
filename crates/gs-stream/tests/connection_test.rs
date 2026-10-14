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
use gs_service::{InMemoryBus, InMemoryEphemeralStore};
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
    ephemeral: Arc<InMemoryEphemeralStore>,
    connections: ConnectionCounter,
    _server: tokio::task::JoinHandle<()>,
    key_with_scope: String,
    device_token: MintedToken,
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

    let credential = key_with_scope.credential.to_string();
    let device_token = mint_device_token(
        &pool,
        credential.as_str(),
        &TokenRequest {
            external_subject_id: format!("stream-{suffix}"),
            installation_id: INSTALLATION.to_string(),
            platform: "ANDROID".to_string(),
            app_version: None,
            sdk_version: None,
            scopes: Some(vec!["stream:connect".to_string()]),
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
    .expect("mint token");

    let ephemeral = InMemoryEphemeralStore::new();
    let bus = InMemoryBus::new();
    let prefix = format!("gs:stream:{suffix}");
    let connections = ConnectionCounter::new();

    let stream_state = Arc::new(StreamState {
        pool: pool.clone(),
        ephemeral: ephemeral.clone(),
        bus: bus.clone(),
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
        ephemeral,
        connections,
        _server: server,
        key_with_scope: key_with_scope.credential.to_string(),
        device_token,
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

#[tokio::test]
async fn test_ready_connection_answers_unimplemented_operations_with_a_protocol_error() {
    // covers: FR-STR-005, docs/08 §4
    let world = setup("unknown-op", StreamConfig::default()).await;
    let ticket = world
        .ticket(&world.device_token.response.access_token)
        .await;

    let mut socket = world.connect(&ticket, "gs.v1.proto").await;
    hello(&mut socket, Subprotocol::Proto).await;

    // `Subscribe` belongs to T-F4-04: until it lands the client gets an explicit error and
    // the connection continues (docs/08 §4).
    let subscribe = gs_service::gs_protocol::ClientMessage {
        request_id: 3,
        body: Some(gs_service::gs_protocol::client_message::Body::Subscribe(
            gs_service::gs_protocol::Subscribe {
                session_id: "ses_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a".to_string(),
                skip_snapshot: false,
            },
        )),
    };
    socket
        .send(frame_of(
            encode_client(&subscribe, Subprotocol::Proto),
            Subprotocol::Proto,
        ))
        .await
        .expect("send subscribe");

    let bytes = read_binary(&mut socket).await;
    let server = decode_server(&bytes, Subprotocol::Proto);
    assert_eq!(server.request_id, 3);
    match server.body {
        Some(gs_service::gs_protocol::server_message::Body::Error(error)) => {
            assert_eq!(
                error.code,
                gs_service::gs_protocol::ErrorCode::ProtocolError as i32
            );
        }
        other => panic!("expected StreamError, got {other:?}"),
    }
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
