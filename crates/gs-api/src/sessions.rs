//! Session REST handlers (docs/05 §4, docs/07 §9.2, FR-SES-001..006).
#![forbid(unsafe_code)]

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use serde_json::{Value, json};

use gs_service::auth::{
    ApplicationPrincipal, DeviceTokenKeys, VerifiedPrincipal, require_scope,
    verify_application_principal, verify_device_token_principal,
};
use gs_service::gs_auth::crypto::CryptoRotator;
use gs_service::gs_domain::error::{AsErrorCode, ErrorCode};
use gs_service::gs_domain::ids::{SessionId, TenantId};
use gs_service::gs_domain::pagination::PageCursor;
use gs_service::gs_domain::session::{Actor, ClosedReason, SessionStatus};
use gs_service::idempotency::{
    IdempotencyContext, IdempotencyDecision, begin as idempotency_begin,
    finish as idempotency_finish,
};
use gs_service::migrate::PgPool;
use gs_service::sessions::{
    CreateSessionRequest, SessionFilters, SessionPage, SessionPolicy, SessionResource,
    SessionUseCaseError, UpdateSessionRequest, close_session_with_bus, create_session, get_session,
    get_session_snapshot, list_sessions_for_application, list_sessions_for_subject,
    update_session_with_bus, verify_device_active_member, verify_device_manage_capability,
    verify_device_read_capability,
};

use crate::pagination::parse_pagination_query;
use crate::problem::{Problem, ProblemResponse};
use crate::rate_limit::{
    RateLimitConfig, SCOPE_REST_APP, SCOPE_REST_DEVICE, check_rate_limit, extract_client_ip,
    rate_limit_problem_response, record_auth_failure_and_check,
};
use crate::router::REQUEST_ID_HEADER;
use crate::security::{principal_from_header, request_id_from};

use gs_service::{InMemoryBus, InMemoryEphemeralStore};
use time::OffsetDateTime;

/// State for the sessions endpoints.
#[derive(Clone)]
pub struct SessionApiState {
    /// PostgreSQL connection pool.
    pub pool: PgPool,
    /// Keyset for verifying device tokens.
    pub signing_keys: Arc<DeviceTokenKeys>,
    /// Token issuer (`GS_JWT_ISSUER`).
    pub issuer: String,
    /// JWT clock leeway in seconds.
    pub leeway_seconds: u64,
    /// Optional cipher for encrypting idempotent responses at rest.
    pub crypto_rotator: Option<Arc<CryptoRotator>>,
    /// Idempotency retention duration.
    pub idempotency_retention: Duration,
    /// Ephemeral store for snapshots and counters (docs/11 §3).
    pub ephemeral: Arc<InMemoryEphemeralStore>,
    /// Bus for control event broadcasting (docs/04 §6.1).
    pub bus: Arc<InMemoryBus>,
    /// Ephemeral key prefix (`GS_REDIS_KEY_PREFIX`, default `gs`).
    pub redis_key_prefix: String,
    /// Stream ticket parameters (docs/06 §5, docs/17 §2.4 and §2.6).
    pub stream_ticket: StreamTicketConfig,
    /// Rate limit configuration (docs/07 §8, docs/17 §2.5).
    pub rate_limit_config: RateLimitConfig,
}

/// Stream ticket configuration: ticket TTL, application connection age and the public
/// base URL that builds `stream_url` (docs/06 §5, docs/17 §2.4 and §2.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamTicketConfig {
    /// Life of a ticket (`GS_TICKET_TTL_S`).
    pub ttl: Duration,
    /// Maximum life of an application connection (`GS_STREAM_APP_MAX_CONNECTION_AGE_S`).
    pub app_max_connection_age: Duration,
    /// Public base URL of the deployment (`GS_PUBLIC_BASE_URL`).
    pub public_base_url: String,
}

impl Default for StreamTicketConfig {
    /// Defaults of `docs/17` §2.4 and §2.6.
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(30),
            app_max_connection_age: Duration::from_secs(3600),
            public_base_url: "http://localhost:8080".to_string(),
        }
    }
}

/// Principal authenticated for a session request.
pub(crate) enum CallerPrincipal {
    Application(Box<ApplicationPrincipal>),
    Device(Box<VerifiedPrincipal>),
}

impl CallerPrincipal {
    pub(crate) fn tenant_id(&self) -> TenantId {
        match self {
            Self::Application(app) => app.tenant.id,
            Self::Device(dev) => dev.tenant.id,
        }
    }

    pub(crate) fn principal_id(&self) -> uuid::Uuid {
        match self {
            Self::Application(app) => app.api_key.id.into_uuid(),
            Self::Device(dev) => dev.device.id.into_uuid(),
        }
    }
}

/// Authenticates the caller principal from `Authorization: Bearer` and enforces REST rate limits (docs/07 §8, FR-API-004).
pub(crate) async fn authenticate_caller(
    state: &SessionApiState,
    headers: &HeaderMap,
    request_id: &str,
) -> Result<CallerPrincipal, Box<Response>> {
    let client_ip = extract_client_ip(headers);
    let header_value = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let principal = match principal_from_header(header_value, request_id) {
        Ok(p) => p,
        Err(resp) => {
            if let Some(rl_resp) = record_auth_failure_and_check(
                state.ephemeral.as_ref(),
                &state.redis_key_prefix,
                &client_ip,
                state.rate_limit_config.auth_failure_rpm,
                request_id,
            )
            .await
            {
                return Err(Box::new(rl_resp));
            }
            return Err(Box::new(resp.into_response()));
        }
    };

    let raw_token = principal.credential();
    if raw_token.starts_with("gsk_") {
        match verify_application_principal(&state.pool, raw_token).await {
            Ok(app) => {
                let key_id = app.api_key.id.to_string();
                let now = OffsetDateTime::now_utc();
                match check_rate_limit(
                    state.ephemeral.as_ref(),
                    &state.redis_key_prefix,
                    SCOPE_REST_APP,
                    &key_id,
                    state.rate_limit_config.app_rpm,
                    now,
                )
                .await
                {
                    Ok(_info) => Ok(CallerPrincipal::Application(Box::new(app))),
                    Err(exceeded) => {
                        let resp = rate_limit_problem_response(
                            exceeded.retry_after,
                            exceeded.limit,
                            SCOPE_REST_APP,
                            request_id,
                        );
                        Err(Box::new(resp))
                    }
                }
            }
            Err(err) => {
                if err.error_code() == ErrorCode::Unauthenticated
                    && let Some(rl_resp) = record_auth_failure_and_check(
                        state.ephemeral.as_ref(),
                        &state.redis_key_prefix,
                        &client_ip,
                        state.rate_limit_config.auth_failure_rpm,
                        request_id,
                    )
                    .await
                {
                    return Err(Box::new(rl_resp));
                }
                let problem =
                    Problem::new(err.error_code(), request_id).with_detail(err.to_string());
                Err(Box::new(ProblemResponse::new(problem).into_response()))
            }
        }
    } else {
        match verify_device_token_principal(
            &state.pool,
            raw_token,
            &state.signing_keys,
            &state.issuer,
            state.leeway_seconds,
        )
        .await
        {
            Ok(dev) => {
                let device_id = dev.device.id.to_string();
                let now = OffsetDateTime::now_utc();
                match check_rate_limit(
                    state.ephemeral.as_ref(),
                    &state.redis_key_prefix,
                    SCOPE_REST_DEVICE,
                    &device_id,
                    state.rate_limit_config.device_rpm,
                    now,
                )
                .await
                {
                    Ok(_info) => Ok(CallerPrincipal::Device(Box::new(dev))),
                    Err(exceeded) => {
                        let resp = rate_limit_problem_response(
                            exceeded.retry_after,
                            exceeded.limit,
                            SCOPE_REST_DEVICE,
                            request_id,
                        );
                        Err(Box::new(resp))
                    }
                }
            }
            Err(err) => {
                if err.error_code() == ErrorCode::Unauthenticated
                    && let Some(rl_resp) = record_auth_failure_and_check(
                        state.ephemeral.as_ref(),
                        &state.redis_key_prefix,
                        &client_ip,
                        state.rate_limit_config.auth_failure_rpm,
                        request_id,
                    )
                    .await
                {
                    return Err(Box::new(rl_resp));
                }
                let problem =
                    Problem::new(err.error_code(), request_id).with_detail(err.to_string());
                Err(Box::new(ProblemResponse::new(problem).into_response()))
            }
        }
    }
}

/// Maps a SessionUseCaseError to a ProblemResponse.
fn session_error_to_response(error: SessionUseCaseError, request_id: &str) -> Response {
    let code = error.error_code();
    let detail = error.to_string();
    ProblemResponse::new(Problem::new(code, request_id).with_detail(detail)).into_response()
}

/// Builds the sessions and members router (docs/07 §9.2, §9.3).
pub fn sessions_router(state: SessionApiState) -> axum::Router {
    axum::Router::new()
        .route(
            "/v1/sessions",
            axum::routing::post(create_session_handler).get(list_sessions_handler),
        )
        .route(
            "/v1/sessions/{session_id}",
            axum::routing::get(get_session_handler).patch(update_session_handler),
        )
        .route(
            "/v1/sessions/{session_id}/close",
            axum::routing::post(close_session_handler),
        )
        .route(
            "/v1/sessions/{session_id}/snapshot",
            axum::routing::get(get_snapshot_handler),
        )
        .merge(crate::members::members_routes())
        .merge(crate::invitations::invitations_routes())
        .merge(crate::subjects::subjects_routes())
        .merge(crate::stream_tickets::stream_tickets_routes())
        .with_state(state)
}

/// `POST /v1/sessions` (OpenAPI `createSession`).
pub async fn create_session_handler(
    State(state): State<SessionApiState>,
    headers: HeaderMap,
    body: Result<Json<CreateSessionRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };

    let Json(request) = match body {
        Ok(b) => b,
        Err(_) => {
            return ProblemResponse::new(
                Problem::new(ErrorCode::ValidationFailed, &request_id)
                    .with_detail("request body must be a valid CreateSessionRequest JSON"),
            )
            .into_response();
        }
    };

    let tenant_id = caller.tenant_id();

    // Check scope permissions (docs/06 §8.2)
    let actor = match &caller {
        CallerPrincipal::Application(app) => {
            if !app.api_key.scopes.iter().any(|s| s == "sessions:write") {
                return ProblemResponse::new(
                    Problem::new(ErrorCode::InsufficientScope, &request_id)
                        .with_detail("application requires sessions:write scope"),
                )
                .into_response();
            }
            Actor::Application(app.application.id)
        }
        CallerPrincipal::Device(dev) => {
            if let Err(err) = require_scope(dev, "sessions:create") {
                return ProblemResponse::new(
                    Problem::new(err.error_code(), &request_id).with_detail(err.to_string()),
                )
                .into_response();
            }
            Actor::Device(dev.device.id)
        }
    };

    // Idempotency support (docs/07 §6)
    let idempotency_key = headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim);

    let json_body = serde_json::to_value(&request).unwrap_or(Value::Null);

    let idemp_ctx = idempotency_key.map(|key| IdempotencyContext {
        tenant_id,
        principal_id: caller.principal_id(),
        key,
        method: "POST",
        route: "/v1/sessions",
        body: &json_body,
    });

    if let (Some(ctx), Some(rotator)) = (&idemp_ctx, &state.crypto_rotator) {
        match idempotency_begin(&state.pool, ctx, state.idempotency_retention, rotator).await {
            Ok(IdempotencyDecision::Replay(stored)) => {
                let mut resp = (
                    StatusCode::from_u16(stored.status).unwrap_or(StatusCode::OK),
                    Json(stored.body),
                )
                    .into_response();
                resp.headers_mut().insert(
                    axum::http::HeaderName::from_static("idempotent-replayed"),
                    axum::http::HeaderValue::from_static("true"),
                );
                if let Ok(v) = axum::http::HeaderValue::from_str(&request_id) {
                    resp.headers_mut().insert(REQUEST_ID_HEADER, v);
                }
                return resp;
            }
            Ok(IdempotencyDecision::InProgress) => {
                return ProblemResponse::new(
                    Problem::new(ErrorCode::Conflict, &request_id)
                        .with_detail("concurrent request with same idempotency key in progress"),
                )
                .into_response();
            }
            Ok(IdempotencyDecision::KeyReused) => {
                return ProblemResponse::new(
                    Problem::new(ErrorCode::ValidationFailed, &request_id)
                        .with_detail("idempotency key reused with different body"),
                )
                .into_response();
            }
            Ok(IdempotencyDecision::Process) => {}
            Err(err) => {
                return ProblemResponse::new(
                    Problem::new(err.error_code(), &request_id).with_detail(err.to_string()),
                )
                .into_response();
            }
        }
    }

    let tenant_policy =
        gs_service::admin::get_effective_policy(&state.pool, &tenant_id.to_string())
            .await
            .unwrap_or_default();
    let policy = SessionPolicy::from_policy(&tenant_policy);

    match create_session(&state.pool, tenant_id, actor, &request, policy).await {
        Ok(session) => {
            let resource =
                match SessionResource::from_session_with_counts(&state.pool, &session).await {
                    Ok(r) => r,
                    Err(_) => SessionResource::from_session(&session),
                };

            let etag_val = resource.etag();
            let response_body = serde_json::to_value(&resource).unwrap_or(Value::Null);

            if let (Some(ctx), Some(rotator)) = (&idemp_ctx, &state.crypto_rotator) {
                let stored = gs_service::idempotency::StoredResponse {
                    status: StatusCode::CREATED.as_u16(),
                    body: response_body.clone(),
                };
                let _ = idempotency_finish(&state.pool, ctx, &stored, rotator).await;
            }

            let mut resp = (StatusCode::CREATED, Json(response_body)).into_response();
            if let Ok(v) = axum::http::HeaderValue::from_str(&etag_val) {
                resp.headers_mut().insert(header::ETAG, v);
            }
            if let Ok(v) = axum::http::HeaderValue::from_str(&request_id) {
                resp.headers_mut().insert(REQUEST_ID_HEADER, v);
            }
            resp
        }
        Err(err) => session_error_to_response(err, &request_id),
    }
}

/// `GET /v1/sessions` (OpenAPI `listSessions`).
pub async fn list_sessions_handler(
    State(state): State<SessionApiState>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };

    let page_req = match parse_pagination_query(raw_query.as_deref()) {
        Ok(p) => p,
        Err(err) => return err.into_response(),
    };

    // Extract query filter params
    let mut status_filter: Option<SessionStatus> = None;
    let mut ext_sub_filter: Option<String> = None;

    if let Some(query_str) = raw_query.as_deref() {
        for pair in query_str.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                if k == "status" {
                    if let Ok(s) = SessionStatus::from_str(v) {
                        status_filter = Some(s);
                    } else {
                        return ProblemResponse::new(
                            Problem::new(ErrorCode::ValidationFailed, &request_id)
                                .with_detail(format!("invalid session status filter: '{v}'")),
                        )
                        .into_response();
                    }
                } else if k == "external_subject_id" {
                    ext_sub_filter = Some(v.to_string());
                }
            }
        }
    }

    let tenant_id = caller.tenant_id();

    let cursor_params = if let Some(ref c) = page_req.cursor {
        match PageCursor::decode(c) {
            Ok(decoded) => Some((decoded.created_at, uuid::Uuid::from_u128(decoded.id))),
            Err(_) => {
                return ProblemResponse::new(
                    Problem::new(ErrorCode::ValidationFailed, &request_id)
                        .with_detail("malformed pagination cursor"),
                )
                .into_response();
            }
        }
    } else {
        None
    };

    match caller {
        CallerPrincipal::Device(dev) => {
            // docs/07 §9.2: "dispositivo: sesiones ACTIVE y no expiradas donde su subject es miembro activo
            // (los filtros status, external_subject_id → 422 si los envía un dispositivo)"
            if status_filter.is_some() || ext_sub_filter.is_some() {
                return ProblemResponse::new(
                    Problem::new(ErrorCode::ValidationFailed, &request_id)
                        .with_detail("filters 'status' and 'external_subject_id' are not allowed for device callers"),
                )
                .into_response();
            }

            match list_sessions_for_subject(
                &state.pool,
                tenant_id,
                dev.subject.id,
                page_req.limit,
                cursor_params,
            )
            .await
            {
                Ok(page) => {
                    render_session_page(&state.pool, page, page_req.limit, 0, &request_id).await
                }
                Err(err) => session_error_to_response(err, &request_id),
            }
        }
        CallerPrincipal::Application(app) => {
            if !app.api_key.scopes.iter().any(|s| s == "sessions:read") {
                return ProblemResponse::new(
                    Problem::new(ErrorCode::InsufficientScope, &request_id)
                        .with_detail("application requires sessions:read scope"),
                )
                .into_response();
            }

            let filters = SessionFilters {
                status: status_filter,
                external_subject_id: ext_sub_filter,
            };

            match list_sessions_for_application(
                &state.pool,
                tenant_id,
                &filters,
                page_req.limit,
                cursor_params,
            )
            .await
            {
                Ok(page) => {
                    render_session_page(&state.pool, page, page_req.limit, 0, &request_id).await
                }
                Err(err) => session_error_to_response(err, &request_id),
            }
        }
    }
}

/// Renders a SessionPage into the OpenAPI `SessionList` shape.
async fn render_session_page(
    pool: &PgPool,
    mut page: SessionPage,
    limit: u32,
    filter_fp: u64,
    request_id: &str,
) -> Response {
    let has_more = page.sessions.len() > limit as usize;
    if has_more {
        page.sessions.truncate(limit as usize);
    }

    let next_cursor = if has_more {
        page.sessions
            .last()
            .map(|s| PageCursor::new(s.created_at, s.id.into_uuid().as_u128(), filter_fp).encode())
    } else {
        None
    };

    let mut resources = Vec::with_capacity(page.sessions.len());
    for s in &page.sessions {
        let res = match SessionResource::from_session_with_counts(pool, s).await {
            Ok(r) => r,
            Err(_) => SessionResource::from_session(s),
        };
        resources.push(res);
    }

    let body = json!({
        "data": resources,
        "next_cursor": next_cursor,
    });

    let mut resp = (StatusCode::OK, Json(body)).into_response();
    if let Ok(v) = axum::http::HeaderValue::from_str(request_id) {
        resp.headers_mut().insert(REQUEST_ID_HEADER, v);
    }
    resp
}

/// `GET /v1/sessions/{session_id}` (OpenAPI `getSession`).
pub async fn get_session_handler(
    State(state): State<SessionApiState>,
    Path(raw_session_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };

    let session_id = match SessionId::from_str(&raw_session_id) {
        Ok(id) => id,
        Err(_) => {
            return ProblemResponse::new(
                Problem::new(ErrorCode::NotFound, &request_id)
                    .with_detail(format!("session '{raw_session_id}' not found")),
            )
            .into_response();
        }
    };

    let tenant_id = caller.tenant_id();

    // Device check: must be active member (docs/07 §9.2)
    if let CallerPrincipal::Device(ref dev) = caller {
        if let Err(err) =
            verify_device_active_member(&state.pool, tenant_id, session_id, dev.subject.id).await
        {
            return session_error_to_response(err, &request_id);
        }
    } else if let CallerPrincipal::Application(ref app) = caller
        && !app.api_key.scopes.iter().any(|s| s == "sessions:read")
    {
        return ProblemResponse::new(
            Problem::new(ErrorCode::InsufficientScope, &request_id)
                .with_detail("application requires sessions:read scope"),
        )
        .into_response();
    }

    match get_session(&state.pool, tenant_id, session_id).await {
        Ok(session) => {
            let resource =
                match SessionResource::from_session_with_counts(&state.pool, &session).await {
                    Ok(r) => r,
                    Err(_) => SessionResource::from_session(&session),
                };

            let etag_val = resource.etag();
            let mut resp = (StatusCode::OK, Json(resource)).into_response();
            if let Ok(v) = axum::http::HeaderValue::from_str(&etag_val) {
                resp.headers_mut().insert(header::ETAG, v);
            }
            if let Ok(v) = axum::http::HeaderValue::from_str(&request_id) {
                resp.headers_mut().insert(REQUEST_ID_HEADER, v);
            }
            resp
        }
        Err(err) => session_error_to_response(err, &request_id),
    }
}

/// `PATCH /v1/sessions/{session_id}` (OpenAPI `updateSession`).
pub async fn update_session_handler(
    State(state): State<SessionApiState>,
    Path(raw_session_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<UpdateSessionRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };

    let session_id = match SessionId::from_str(&raw_session_id) {
        Ok(id) => id,
        Err(_) => {
            return ProblemResponse::new(
                Problem::new(ErrorCode::NotFound, &request_id)
                    .with_detail(format!("session '{raw_session_id}' not found")),
            )
            .into_response();
        }
    };

    let Json(request) = match body {
        Ok(b) => b,
        Err(_) => {
            return ProblemResponse::new(
                Problem::new(ErrorCode::ValidationFailed, &request_id)
                    .with_detail("request body must be a valid UpdateSessionRequest JSON"),
            )
            .into_response();
        }
    };

    // Parse If-Match header (docs/07 §9.2)
    let if_match_val = headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(str::trim);

    let expected_version: u64 = match if_match_val {
        Some(val) => {
            let clean = val.trim_start_matches("W/").trim_matches('"');
            match clean.parse() {
                Ok(v) => v,
                Err(_) => {
                    return ProblemResponse::new(
                        Problem::new(ErrorCode::PreconditionFailed, &request_id)
                            .with_detail("invalid If-Match header format"),
                    )
                    .into_response();
                }
            }
        }
        None => {
            return ProblemResponse::new(
                Problem::new(ErrorCode::PreconditionFailed, &request_id)
                    .with_detail("missing required If-Match header"),
            )
            .into_response();
        }
    };

    let tenant_id = caller.tenant_id();

    // Check manage capability or sessions:write scope
    match &caller {
        CallerPrincipal::Device(dev) => {
            if let Err(err) =
                verify_device_manage_capability(&state.pool, tenant_id, session_id, dev.subject.id)
                    .await
            {
                return session_error_to_response(err, &request_id);
            }
        }
        CallerPrincipal::Application(app) => {
            if !app.api_key.scopes.iter().any(|s| s == "sessions:write") {
                return ProblemResponse::new(
                    Problem::new(ErrorCode::InsufficientScope, &request_id)
                        .with_detail("application requires sessions:write scope"),
                )
                .into_response();
            }
        }
    }

    let tenant_policy =
        gs_service::admin::get_effective_policy(&state.pool, &tenant_id.to_string())
            .await
            .unwrap_or_default();
    let policy = SessionPolicy::from_policy(&tenant_policy);
    let actor = match &caller {
        CallerPrincipal::Device(dev) => Actor::Device(dev.device.id),
        CallerPrincipal::Application(app) => Actor::Application(app.application.id),
    };

    match update_session_with_bus(
        &state.pool,
        state.bus.as_ref(),
        &state.redis_key_prefix,
        tenant_id,
        session_id,
        Some(expected_version),
        &request,
        policy,
        Some(actor),
    )
    .await
    {
        Ok(session) => {
            let resource =
                match SessionResource::from_session_with_counts(&state.pool, &session).await {
                    Ok(r) => r,
                    Err(_) => SessionResource::from_session(&session),
                };

            let etag_val = resource.etag();
            let mut resp = (StatusCode::OK, Json(resource)).into_response();
            if let Ok(v) = axum::http::HeaderValue::from_str(&etag_val) {
                resp.headers_mut().insert(header::ETAG, v);
            }
            if let Ok(v) = axum::http::HeaderValue::from_str(&request_id) {
                resp.headers_mut().insert(REQUEST_ID_HEADER, v);
            }
            resp
        }
        Err(err) => session_error_to_response(err, &request_id),
    }
}

/// `POST /v1/sessions/{session_id}/close` (OpenAPI `closeSession`).
pub async fn close_session_handler(
    State(state): State<SessionApiState>,
    Path(raw_session_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };

    let session_id = match SessionId::from_str(&raw_session_id) {
        Ok(id) => id,
        Err(_) => {
            return ProblemResponse::new(
                Problem::new(ErrorCode::NotFound, &request_id)
                    .with_detail(format!("session '{raw_session_id}' not found")),
            )
            .into_response();
        }
    };

    let tenant_id = caller.tenant_id();

    let (reason, actor) = match &caller {
        CallerPrincipal::Device(dev) => {
            if let Err(err) =
                verify_device_manage_capability(&state.pool, tenant_id, session_id, dev.subject.id)
                    .await
            {
                return session_error_to_response(err, &request_id);
            }
            (ClosedReason::ClosedByMember, Actor::Device(dev.device.id))
        }
        CallerPrincipal::Application(app) => {
            if !app.api_key.scopes.iter().any(|s| s == "sessions:write") {
                return ProblemResponse::new(
                    Problem::new(ErrorCode::InsufficientScope, &request_id)
                        .with_detail("application requires sessions:write scope"),
                )
                .into_response();
            }
            (
                ClosedReason::ClosedByApplication,
                Actor::Application(app.application.id),
            )
        }
    };

    match close_session_with_bus(
        &state.pool,
        state.ephemeral.as_ref(),
        state.bus.as_ref(),
        &state.redis_key_prefix,
        tenant_id,
        session_id,
        reason,
        Some(actor),
    )
    .await
    {
        Ok(session) => {
            let resource =
                match SessionResource::from_session_with_counts(&state.pool, &session).await {
                    Ok(r) => r,
                    Err(_) => SessionResource::from_session(&session),
                };

            let mut resp = (StatusCode::OK, Json(resource)).into_response();
            if let Ok(v) = axum::http::HeaderValue::from_str(&request_id) {
                resp.headers_mut().insert(REQUEST_ID_HEADER, v);
            }
            resp
        }
        Err(err) => session_error_to_response(err, &request_id),
    }
}

/// `GET /v1/sessions/{session_id}/snapshot` (OpenAPI `getSnapshot`, FR-INT-002, docs/07 §9.6).
pub async fn get_snapshot_handler(
    State(state): State<SessionApiState>,
    Path(raw_session_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };

    let session_id = match SessionId::from_str(&raw_session_id) {
        Ok(id) => id,
        Err(_) => {
            return ProblemResponse::new(
                Problem::new(ErrorCode::NotFound, &request_id)
                    .with_detail(format!("session '{raw_session_id}' not found")),
            )
            .into_response();
        }
    };

    let tenant_id = caller.tenant_id();

    // Authorization:
    // Device check: must be active member with read capability (docs/07 §9.6, FR-INT-002)
    if let CallerPrincipal::Device(ref dev) = caller {
        if let Err(err) =
            verify_device_read_capability(&state.pool, tenant_id, session_id, dev.subject.id).await
        {
            return session_error_to_response(err, &request_id);
        }
    } else if let CallerPrincipal::Application(ref app) = caller
        && !app.api_key.scopes.iter().any(|s| s == "locations:read")
    {
        return ProblemResponse::new(
            Problem::new(ErrorCode::InsufficientScope, &request_id)
                .with_detail("application requires locations:read scope"),
        )
        .into_response();
    }

    let now = OffsetDateTime::now_utc();
    match get_session_snapshot(
        &state.pool,
        state.ephemeral.as_ref(),
        &state.redis_key_prefix,
        tenant_id,
        session_id,
        now,
    )
    .await
    {
        Ok(snapshot) => {
            let mut resp = (StatusCode::OK, Json(snapshot)).into_response();
            resp.headers_mut().insert(
                header::CACHE_CONTROL,
                axum::http::HeaderValue::from_static("no-store"),
            );
            if let Ok(v) = axum::http::HeaderValue::from_str(&request_id) {
                resp.headers_mut().insert(REQUEST_ID_HEADER, v);
            }
            resp
        }
        Err(err) => session_error_to_response(err, &request_id),
    }
}
