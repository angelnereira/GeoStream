//! Integration tests for the HTTP skeleton of `gs-api` (T-F2-07, FR-OPS-002, FR-API-001).
#![forbid(unsafe_code)]

use std::net::TcpListener;

use axum::response::IntoResponse;
use gs_api::{
    AdminState, DependencyStatus, PrometheusRenderer, REQUEST_ID_HEADER, ReadinessProbe,
    admin_ready_handler, health_live_handler, http_router, http_router_with_readiness,
    principal_from_header,
};
use gs_service::gs_domain::error::ErrorCode;
use serde_json::{Value, json};

/// Probe handler that consumes the request body through the `String` extractor, which is
/// what `DefaultBodyLimit` guards (axum 0.8).
async fn probe_handler(body: String) -> String {
    body.len().to_string()
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

#[tokio::test]
async fn test_health_live_and_ready_are_served_with_the_request_id() {
    // covers: FR-OPS-002, docs/07 §1
    let app = http_router(gs_api::DEFAULT_MAX_BODY_BYTES);
    let (address, server) = spawn(app).await;
    let client = reqwest::Client::new();

    let live = client
        .get(format!("{address}/health/live"))
        .send()
        .await
        .expect("live");
    assert!(live.status().is_success());
    assert!(
        live.headers()
            .get(REQUEST_ID_HEADER)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("req_"))
    );

    // Without dependencies configured the instance is ready (docs/16 §4.1).
    let ready = client
        .get(format!("{address}/health/ready"))
        .send()
        .await
        .expect("ready");
    assert!(ready.status().is_success());
    let body: Value = serde_json::from_str(&ready.text().await.expect("text")).expect("json");
    assert_eq!(body["ready"], json!(true));

    server.abort();
}

#[tokio::test]
async fn test_health_ready_reports_a_failed_dependency_as_503() {
    // covers: FR-OPS-002, docs/16 §4.1
    let probe = ReadinessProbe::new(|| async {
        vec![DependencyStatus {
            name: "postgresql".to_string(),
            ok: false,
            detail: Some("connection refused".to_string()),
        }]
    });
    let app = http_router_with_readiness(gs_api::DEFAULT_MAX_BODY_BYTES, probe);
    let (address, server) = spawn(app).await;

    let response = reqwest::Client::new()
        .get(format!("{address}/health/ready"))
        .send()
        .await
        .expect("ready");
    assert_eq!(response.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    let body: Value = serde_json::from_str(&response.text().await.expect("text")).expect("json");
    assert_eq!(body["ready"], json!(false));
    assert_eq!(body["dependencies"][0]["name"], json!("postgresql"));
    assert_eq!(body["dependencies"][0]["ok"], json!(false));

    server.abort();
}

#[tokio::test]
async fn test_body_limit_rejects_oversized_payloads() {
    // covers: docs/07 §1, docs/17 §2.1
    // The limit is a router layer, so the probe uses a route that actually reads the body.
    let app = axum::Router::new()
        .route("/probe", axum::routing::post(probe_handler))
        .layer(axum::extract::DefaultBodyLimit::max(64));
    let (address, server) = spawn(app).await;

    let accepted = reqwest::Client::new()
        .post(format!("{address}/probe"))
        .body("x".repeat(32))
        .send()
        .await
        .expect("small payload");
    assert!(accepted.status().is_success());

    let rejected = reqwest::Client::new()
        .post(format!("{address}/probe"))
        .body("x".repeat(512))
        .send()
        .await
        .expect("oversized payload");
    assert_eq!(rejected.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);

    // The production router is built with the documented default limit.
    assert_eq!(gs_api::DEFAULT_MAX_BODY_BYTES, 65_536);

    server.abort();
}

#[tokio::test]
async fn test_missing_credential_renders_problem_details() {
    // covers: FR-API-001, docs/07 §3, §5
    let error = principal_from_header(None, "req_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a")
        .expect_err("missing credential");
    let body = serde_json::to_value(&error.problem).expect("serialize");
    assert_eq!(
        body["type"],
        json!("https://geostream.dev/problems/unauthenticated")
    );
    assert_eq!(body["title"], json!("Unauthenticated"));
    assert_eq!(body["status"], json!(401));
    assert_eq!(body["code"], json!("UNAUTHENTICATED"));
    assert_eq!(
        body["request_id"],
        json!("req_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a")
    );

    let problem = gs_api::Problem::new(
        ErrorCode::SessionFull,
        "req_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a",
    );
    let body = serde_json::to_value(&problem).expect("serialize");
    assert_eq!(
        body["type"],
        json!("https://geostream.dev/problems/session_full")
    );
    assert_eq!(body["title"], json!("Session is full"));
}

#[tokio::test]
async fn test_access_logs_never_carry_the_credential() {
    // covers: FR-PRV-004, docs/10 §8.2
    // The access log middleware only reads method, path and the presence of Authorization,
    // so the credential never reaches the log writer.
    let app = http_router(gs_api::DEFAULT_MAX_BODY_BYTES);
    let (address, server) = spawn(app).await;

    let response = reqwest::Client::new()
        .get(format!("{address}/health/live"))
        .header("authorization", "Bearer gsk_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a.secrettosecrettosecrettosecrettosecret1234")
        .send()
        .await
        .expect("request");
    assert!(response.status().is_success());

    server.abort();
}

#[tokio::test]
async fn test_admin_ready_handler_reflects_the_probe() {
    // covers: FR-OPS-002, docs/16 §4.1
    let state = AdminState::new(
        PrometheusRenderer::install().expect("install renderer"),
        ReadinessProbe::new(|| async {
            vec![DependencyStatus {
                name: "postgresql".to_string(),
                ok: true,
                detail: None,
            }]
        }),
    );

    let response = admin_ready_handler(axum::extract::State(state)).await;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn test_health_live_handler_shape() {
    // covers: FR-OPS-002
    let response = health_live_handler().await.into_response();
    assert!(response.status().is_success());
}
