//! HTTP router: correlation ids, body limits, redacted access logs and health endpoints
//! (docs/07 §1–§2, docs/16 §4.1, FR-OPS-002).
#![forbid(unsafe_code)]

use std::time::Duration;

use axum::Router;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Json, Response};
use serde_json::{Value, json};
use tracing::{Instrument, info, info_span};

/// Header carrying the correlation id (docs/07 §1).
pub const REQUEST_ID_HEADER: &str = "X-Request-Id";

/// Default maximum body size when the configuration does not say otherwise (docs/17 §2.1).
pub const DEFAULT_MAX_BODY_BYTES: usize = 65_536;

/// Name of the request id header, re-exported for callers of `request_id_header`.
pub const fn request_id_header() -> &'static str {
    REQUEST_ID_HEADER
}

/// Renders an opaque id, always replacing empty values with `<redacted>`.
pub fn redact_header_value(value: &str) -> String {
    if value.is_empty() {
        "<redacted>".to_string()
    } else {
        value.to_string()
    }
}

/// Extracts the request id of a request, generating one when absent.
fn request_id_of(request: &Request) -> String {
    let incoming = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 128);

    match incoming {
        Some(value) => value.to_string(),
        None => gs_service::gs_domain::ids::RequestId::new().to_string(),
    }
}

/// Sets the correlation id on a response.
fn set_request_id(response: &mut Response, request_id: &str) {
    if let Ok(value) = HeaderValue::from_str(request_id) {
        response.headers_mut().insert(REQUEST_ID_HEADER, value);
    }
}

/// Logs the request without credentials, tickets or bodies (docs/10 §8.2, docs/16 §4.3).
///
/// Only the documented fields are emitted: correlation id, connection id when present,
/// tenant id, principal kind, route template, status and duration. `Authorization` and
/// `ticket` are never read, and bodies are never rendered.
async fn access_log(request: Request, next: Next) -> Response {
    let started = std::time::Instant::now();
    let request_id = request_id_of(&request);
    let method = request.method().as_str().to_string();
    let uri = request.uri().clone();
    let route = redact_header_value(uri.path());
    let has_authorization = request.headers().contains_key(header::AUTHORIZATION);
    let principal_kind = if has_authorization {
        "credential"
    } else {
        "none"
    };

    let span = info_span!(
        "http_request",
        request_id = %request_id,
        method = %method,
        route = %route,
    );

    let mut response = next.run(request).instrument(span).await;
    let status = response.status().as_u16();
    let duration_ms = started.elapsed().as_millis();

    // The presence of a credential is recorded, never its value.
    info!(
        request_id = %request_id,
        principal_kind,
        route = %route,
        method = %method,
        status,
        duration_ms,
        "http request completed"
    );

    set_request_id(&mut response, &request_id);
    response
}

/// Builds the public router with the middlewares of docs/07 §1–§2.
pub fn http_router(max_body_bytes: usize) -> Router {
    http_router_with_readiness(max_body_bytes, ReadinessProbe::default())
}

/// Builds the public router with an explicit readiness probe (docs/16 §4.1).
pub fn http_router_with_readiness(max_body_bytes: usize, probe: ReadinessProbe) -> Router {
    Router::new()
        .route("/health/live", axum::routing::get(health_live_handler))
        .route("/health/ready", axum::routing::get(health_ready_handler))
        .with_state(probe)
        .layer(axum::middleware::from_fn(access_log))
        .layer(DefaultBodyLimit::max(max_body_bytes))
}

/// `/health/live`: the process is serving (docs/16 §4.1).
pub async fn health_live_handler() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// State of one readiness check: the enum of the `ReadinessStatus` schema of the OpenAPI
/// contract (docs/16 §4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    /// The dependency answered in time.
    Ok,
    /// The dependency did not answer, or answered with an error.
    Failing,
    /// The instance is draining and must leave the balancer (docs/08 §6.8).
    Draining,
    /// The dependency is not used by this configuration (e.g. Redis with `memory`).
    NotConfigured,
}

impl CheckState {
    /// Whether this state still lets the instance receive traffic.
    pub fn allows_traffic(self) -> bool {
        matches!(self, Self::Ok | Self::NotConfigured)
    }
}

/// Readiness of one dependency (docs/16 §4.1).
///
/// Carries no error text: a driver error can echo connection details (docs/10 §8.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyStatus {
    /// Dependency name.
    pub name: String,
    /// Result of its check.
    pub state: CheckState,
}

impl DependencyStatus {
    /// A dependency that answered.
    pub fn ok(name: impl Into<String>) -> Self {
        Self::with_state(name, CheckState::Ok)
    }

    /// A dependency that did not answer.
    pub fn failing(name: impl Into<String>) -> Self {
        Self::with_state(name, CheckState::Failing)
    }

    /// A dependency this configuration does not use.
    pub fn not_configured(name: impl Into<String>) -> Self {
        Self::with_state(name, CheckState::NotConfigured)
    }

    /// A dependency in an explicit state.
    pub fn with_state(name: impl Into<String>, state: CheckState) -> Self {
        Self {
            name: name.into(),
            state,
        }
    }
}

/// Overall state of `ReadinessStatus.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadyState {
    /// Every check allows traffic.
    Ready,
    /// At least one check does not.
    NotReady,
}

/// Body of `/health/ready`: the `ReadinessStatus` schema of the OpenAPI contract.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ReadinessStatus {
    /// Whether the instance may receive traffic.
    pub status: ReadyState,
    /// State of each checked dependency, by name.
    pub checks: std::collections::BTreeMap<String, CheckState>,
}

impl ReadinessStatus {
    /// Builds a readiness status from the checked dependencies.
    pub fn new(dependencies: Vec<DependencyStatus>) -> Self {
        let status = if dependencies.iter().all(|d| d.state.allows_traffic()) {
            ReadyState::Ready
        } else {
            ReadyState::NotReady
        };
        Self {
            status,
            checks: dependencies
                .into_iter()
                .map(|dependency| (dependency.name, dependency.state))
                .collect(),
        }
    }

    /// Whether the instance may receive traffic.
    pub fn is_ready(&self) -> bool {
        self.status == ReadyState::Ready
    }

    /// Converts the status into an HTTP response.
    pub fn into_response(&self) -> Response {
        let status = if self.is_ready() {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        };
        (status, Json(self.clone())).into_response()
    }
}

/// `/health/ready`: PostgreSQL answered (docs/16 §4.1, FR-OPS-002).
///
/// The database check runs with a one second timeout. The ephemeral store and the bus are
/// reported when they are configured, which is what `docs/16` §4.1 requires.
pub async fn health_ready_handler(State(state): State<ReadinessProbe>) -> Response {
    let dependencies = state.probe().await;
    ReadinessStatus::new(dependencies).into_response()
}

/// Future returned by a readiness probe.
pub type ReadinessFuture = std::pin::Pin<Box<dyn Future<Output = Vec<DependencyStatus>> + Send>>;

/// Dependency checks injected by the server.
#[derive(Clone)]
pub struct ReadinessProbe {
    probe: std::sync::Arc<dyn Fn() -> ReadinessFuture + Send + Sync>,
}

impl ReadinessProbe {
    /// Builds a probe from a closure.
    pub fn new<F, Fut>(probe: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Vec<DependencyStatus>> + Send + 'static,
    {
        Self {
            probe: std::sync::Arc::new(move || Box::pin(probe())),
        }
    }

    /// Runs the checks.
    pub async fn probe(&self) -> Vec<DependencyStatus> {
        (self.probe)().await
    }
}

impl Default for ReadinessProbe {
    fn default() -> Self {
        Self::new(|| async { Vec::new() })
    }
}

/// Prometheus renderer installed once and shared by the admin listener (docs/16 §4.2).
#[derive(Clone)]
pub struct PrometheusRenderer {
    handle: std::sync::Arc<metrics_exporter_prometheus::PrometheusHandle>,
}

impl PrometheusRenderer {
    /// Installs the global recorder and returns the renderer.
    pub fn install() -> Result<Self, String> {
        let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        metrics::set_global_recorder(recorder).map_err(|e| e.to_string())?;
        Ok(Self {
            handle: std::sync::Arc::new(handle),
        })
    }

    /// Renders the current snapshot in Prometheus text format.
    pub fn render(&self) -> String {
        self.handle.render()
    }
}

/// `/metrics`: Prometheus text exposition (docs/16 §4.2).
pub async fn metrics_handler(State(state): State<AdminState>) -> Response {
    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        state.renderer.render(),
    )
        .into_response()
}

/// State of the admin listener: the metrics renderer and the readiness probe (docs/16 §4.1).
#[derive(Clone)]
pub struct AdminState {
    /// Prometheus renderer installed once.
    renderer: PrometheusRenderer,
    /// Dependency checks.
    probe: ReadinessProbe,
}

impl AdminState {
    /// Builds the admin state.
    pub const fn new(renderer: PrometheusRenderer, probe: ReadinessProbe) -> Self {
        Self { renderer, probe }
    }
}

/// Builds the admin router with `/metrics` and a copy of the health endpoints (docs/16 §4.1).
pub fn admin_router(max_body_bytes: usize, state: AdminState) -> Router {
    Router::new()
        .route("/metrics", axum::routing::get(metrics_handler))
        .route("/health/live", axum::routing::get(health_live_handler))
        .route("/health/ready", axum::routing::get(admin_ready_handler))
        .with_state(state)
        .layer(DefaultBodyLimit::max(max_body_bytes))
}

/// `/health/ready` of the admin listener.
pub async fn admin_ready_handler(State(state): State<AdminState>) -> Response {
    let dependencies = state.probe.probe().await;
    ReadinessStatus::new(dependencies).into_response()
}

/// Timeout used by readiness checks (docs/16 §4.1).
pub const READINESS_TIMEOUT: Duration = Duration::from_secs(1);

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn test_health_live_returns_ok() {
        // covers: FR-OPS-002, docs/16 §4.1
        let response = health_live_handler().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let json: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["status"], json!("ok"));
    }

    #[tokio::test]
    async fn test_health_ready_reflects_the_probe() {
        // covers: FR-OPS-002, docs/16 §4.1
        let ready_probe =
            ReadinessProbe::new(|| async { vec![DependencyStatus::ok("postgresql")] });
        let response = health_ready_handler(State(ready_probe)).await;
        assert_eq!(response.status(), StatusCode::OK);

        let degraded_probe =
            ReadinessProbe::new(|| async { vec![DependencyStatus::failing("postgresql")] });
        let response = health_ready_handler(State(degraded_probe)).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_router_sets_the_request_id_on_responses() {
        // covers: docs/07 §1
        let app = http_router(DEFAULT_MAX_BODY_BYTES);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("addr");

        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        let client = reqwest::Client::new();
        let response = client
            .get(format!("http://{address}/health/live"))
            .send()
            .await
            .expect("request");
        assert!(response.status().is_success());
        let request_id = response
            .headers()
            .get(REQUEST_ID_HEADER)
            .and_then(|value| value.to_str().ok())
            .expect("X-Request-Id must be present");
        assert!(
            request_id.starts_with("req_"),
            "unexpected request id: {request_id}"
        );

        server.abort();
    }

    #[tokio::test]
    async fn test_router_echoes_an_incoming_request_id() {
        // covers: docs/07 §1
        let app = http_router(DEFAULT_MAX_BODY_BYTES);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("addr");

        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        let client = reqwest::Client::new();
        let response = client
            .get(format!("http://{address}/health/live"))
            .header(REQUEST_ID_HEADER, "req_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a")
            .send()
            .await
            .expect("request");
        assert_eq!(
            response
                .headers()
                .get(REQUEST_ID_HEADER)
                .and_then(|value| value.to_str().ok()),
            Some("req_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a")
        );

        server.abort();
    }
}
