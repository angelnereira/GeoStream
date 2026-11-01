//! `/v1/console/*`: registration, sign-in and management of organizations, projects and API
//! keys (ADR-0036, OpenAPI tag `Console`, FR-CON-001 to FR-CON-012).
//!
//! The session token is the only credential here (`Authorization: Bearer gcs_…`); identity is
//! never read from the payload. Every response carries `Cache-Control: no-store`: sessions and
//! key secrets must not sit in any cache. Nothing in this module logs an e-mail, a code, a
//! session token or a key secret.
#![forbid(unsafe_code)]

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use gs_service::console::{ConsoleAuth, ConsoleError};
use gs_service::console_workspace::{self as workspace, ProjectUpdate, WorkspaceError};
use gs_service::gs_domain::error::{AsErrorCode, ErrorCode};
use gs_service::gs_domain::ids::{AccountId, ApiKeyId, ApplicationId, TenantId};
use gs_service::gs_store::{
    ApiKey, Application, ConsoleAccount, ConsoleRole, OrganizationMembership,
};
use gs_service::idempotency::{
    IdempotencyContext, IdempotencyDecision, StoredResponse, begin as idempotency_begin,
    finish as idempotency_finish,
};
use gs_service::migrate::PgPool;
use gs_service::{BusBackend, EphemeralBackend};

use crate::problem::{FieldError, Problem, ProblemResponse};
use crate::rate_limit::{check_rate_limit, extract_client_ip, rate_limit_problem_response};
use crate::security::request_id_from;
use crate::{REQUEST_ID_HEADER, RateLimitConfig};
use gs_service::gs_auth::crypto::CryptoRotator;

/// Rate limit scope of the unauthenticated sign-in routes, per client IP.
const SCOPE_CONSOLE_IP: &str = "console_ip";
/// Rate limit scope of the authenticated routes, per account.
const SCOPE_CONSOLE_ACCOUNT: &str = "console_account";
/// Requests per minute and account on the authenticated routes.
const ACCOUNT_RPM: u32 = 120;

/// State of the console routes.
#[derive(Clone)]
pub struct ConsoleApiState {
    /// PostgreSQL pool.
    pub pool: PgPool,
    /// Sign-in service (codes, sessions).
    pub auth: Arc<ConsoleAuth>,
    /// Counters for rate limiting.
    pub ephemeral: Arc<EphemeralBackend>,
    /// Bus used to invalidate credentials when a project is disabled or a key is revoked.
    pub bus: Arc<BusBackend>,
    /// Ephemeral key prefix (`GS_REDIS_KEY_PREFIX`).
    pub redis_key_prefix: String,
    /// `GS_CONSOLE_SIGNUP_RPM_PER_IP`.
    pub signup_rpm_per_ip: u32,
    /// `GS_CONSOLE_MAX_ORGS_PER_ACCOUNT`.
    pub max_orgs_per_account: u32,
    /// Cipher for stored idempotent responses; without it `Idempotency-Key` is not honoured.
    pub crypto_rotator: Option<Arc<CryptoRotator>>,
    /// How long an idempotent response is kept.
    pub idempotency_retention: Duration,
    /// Shared rate limit configuration (`auth_failure_rpm` is unused here).
    pub rate_limit_config: RateLimitConfig,
}

/// Mounts `/v1/console/*`.
pub fn console_router(state: ConsoleApiState) -> Router {
    Router::new()
        .route("/v1/console/auth/code", post(request_code))
        .route("/v1/console/auth/verify", post(verify_code))
        .route("/v1/console/auth/logout", post(logout))
        .route("/v1/console/me", get(get_me).delete(delete_me))
        .route(
            "/v1/console/organizations",
            get(list_organizations).post(create_organization),
        )
        .route(
            "/v1/console/organizations/{organization_id}",
            get(get_organization),
        )
        .route(
            "/v1/console/organizations/{organization_id}/projects",
            get(list_projects).post(create_project),
        )
        .route(
            "/v1/console/organizations/{organization_id}/projects/{project_id}",
            get(get_project).patch(update_project),
        )
        .route(
            "/v1/console/organizations/{organization_id}/projects/{project_id}/keys",
            get(list_keys).post(create_key),
        )
        .route(
            "/v1/console/organizations/{organization_id}/projects/{project_id}/keys/{key_id}",
            axum::routing::delete(revoke_key),
        )
        .layer(axum::middleware::map_response(no_store))
        .with_state(state)
}

/// Sessions and key secrets must not be cached by anything between the server and the person.
async fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

// ---- wire types -----------------------------------------------------------------------------

fn rfc3339(at: OffsetDateTime) -> String {
    at.format(&Rfc3339).unwrap_or_default()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CodeRequest {
    email: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifyRequest {
    email: String,
    code: String,
    #[serde(default)]
    accept_terms: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateOrganizationRequest {
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateProjectRequest {
    name: String,
    allowed_device_scopes: Option<Vec<String>>,
    allowed_origins: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateProjectRequest {
    name: Option<String>,
    status: Option<String>,
    allowed_device_scopes: Option<Vec<String>>,
    allowed_origins: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateKeyRequest {
    label: String,
    scopes: Vec<String>,
    expires_in_days: Option<u16>,
}

#[derive(Serialize)]
struct AccountBody {
    id: String,
    email: String,
    status: &'static str,
    created_at: String,
}

impl AccountBody {
    fn from(account: &ConsoleAccount) -> Self {
        Self {
            id: account.id.to_string(),
            email: account.email.clone(),
            status: account.status.as_str(),
            created_at: rfc3339(account.created_at),
        }
    }
}

#[derive(Serialize)]
struct OrganizationBody {
    id: String,
    name: String,
    role: &'static str,
    created_at: String,
}

impl OrganizationBody {
    fn from(membership: &OrganizationMembership) -> Self {
        Self {
            id: membership.tenant.id.to_string(),
            name: membership.tenant.name.clone(),
            role: membership.role.as_str(),
            created_at: rfc3339(membership.tenant.created_at),
        }
    }
}

#[derive(Serialize)]
struct ProjectBody {
    id: String,
    organization_id: String,
    name: String,
    status: &'static str,
    allowed_device_scopes: Vec<String>,
    allowed_origins: Vec<String>,
    created_at: String,
    updated_at: String,
}

impl ProjectBody {
    fn from(app: &Application) -> Self {
        Self {
            id: app.id.to_string(),
            organization_id: app.tenant_id.to_string(),
            name: app.name.clone(),
            status: app.status.as_str(),
            allowed_device_scopes: app.allowed_device_scopes.clone(),
            allowed_origins: app.allowed_origins.clone(),
            created_at: rfc3339(app.created_at),
            updated_at: rfc3339(app.updated_at),
        }
    }
}

#[derive(Serialize)]
struct KeyBody {
    id: String,
    label: String,
    prefix: String,
    scopes: Vec<String>,
    created_at: String,
    expires_at: Option<String>,
    revoked_at: Option<String>,
}

impl KeyBody {
    fn from(key: &ApiKey) -> Self {
        // `gsk_` plus the first hex digits of the key id: enough to recognise it, useless to use.
        let simple = key.id.into_uuid().simple().to_string();
        Self {
            id: key.id.to_string(),
            label: key.label.clone().unwrap_or_default(),
            prefix: format!("gsk_{}", &simple[..8]),
            scopes: key.scopes.clone(),
            created_at: rfc3339(key.created_at),
            expires_at: key.expires_at.map(rfc3339),
            revoked_at: key.revoked_at.map(rfc3339),
        }
    }
}

// ---- errors ---------------------------------------------------------------------------------

fn problem(code: ErrorCode, request_id: &str) -> Response {
    ProblemResponse::new(Problem::new(code, request_id)).into_response()
}

fn validation(request_id: &str, field: &str) -> Response {
    ProblemResponse::new(
        Problem::new(ErrorCode::ValidationFailed, request_id).with_errors(vec![FieldError {
            field: field.to_string(),
            reason: "invalid_format".to_string(),
        }]),
    )
    .into_response()
}

fn console_error(error: &ConsoleError, request_id: &str) -> Response {
    match error {
        ConsoleError::InvalidEmail => validation(request_id, "email"),
        ConsoleError::InvalidCodeFormat => validation(request_id, "code"),
        ConsoleError::TermsRequired => ProblemResponse::new(
            Problem::new(ErrorCode::ValidationFailed, request_id).with_errors(vec![FieldError {
                field: "accept_terms".to_string(),
                reason: "required".to_string(),
            }]),
        )
        .into_response(),
        other => problem(other.error_code(), request_id),
    }
}

fn workspace_error(error: &WorkspaceError, request_id: &str) -> Response {
    match error {
        WorkspaceError::Validation(field) => validation(request_id, field),
        WorkspaceError::LimitReached => ProblemResponse::new(
            Problem::new(ErrorCode::RateLimited, request_id)
                .with_detail("organization limit reached for this account"),
        )
        .into_response(),
        other => problem(other.error_code(), request_id),
    }
}

fn json_body<T>(
    body: Result<Json<T>, axum::extract::rejection::JsonRejection>,
    request_id: &str,
) -> Result<T, Box<Response>> {
    body.map(|Json(value)| value)
        .map_err(|_| Box::new(validation(request_id, "body")))
}

fn with_request_id(mut response: Response, request_id: &str) -> Response {
    if let Ok(value) = HeaderValue::from_str(request_id) {
        response.headers_mut().insert(REQUEST_ID_HEADER, value);
    }
    response
}

// ---- authentication and limits --------------------------------------------------------------

struct Caller {
    token: String,
    account: ConsoleAccount,
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ")?.trim();
    (!token.is_empty()).then(|| token.to_string())
}

/// Per-IP limit of the unauthenticated routes.
async fn limit_ip(
    state: &ConsoleApiState,
    headers: &HeaderMap,
    request_id: &str,
) -> Option<Response> {
    let ip = extract_client_ip(headers);
    check_rate_limit(
        state.ephemeral.as_ref(),
        &state.redis_key_prefix,
        SCOPE_CONSOLE_IP,
        &ip,
        state.signup_rpm_per_ip,
        OffsetDateTime::now_utc(),
    )
    .await
    .err()
    .map(|exceeded| {
        rate_limit_problem_response(
            exceeded.retry_after,
            exceeded.limit,
            SCOPE_CONSOLE_IP,
            request_id,
        )
    })
}

/// Session check plus the per-account limit.
async fn authenticate(
    state: &ConsoleApiState,
    headers: &HeaderMap,
    request_id: &str,
) -> Result<Caller, Box<Response>> {
    let Some(token) = bearer(headers) else {
        return Err(Box::new(problem(ErrorCode::Unauthenticated, request_id)));
    };
    let account = state
        .auth
        .authenticate(&token)
        .await
        .map_err(|error| Box::new(console_error(&error, request_id)))?;
    check_rate_limit(
        state.ephemeral.as_ref(),
        &state.redis_key_prefix,
        SCOPE_CONSOLE_ACCOUNT,
        &account.id.to_string(),
        ACCOUNT_RPM,
        OffsetDateTime::now_utc(),
    )
    .await
    .map_err(|exceeded| {
        Box::new(rate_limit_problem_response(
            exceeded.retry_after,
            exceeded.limit,
            SCOPE_CONSOLE_ACCOUNT,
            request_id,
        ))
    })?;
    Ok(Caller { token, account })
}

macro_rules! caller_or_return {
    ($state:expr, $headers:expr, $request_id:expr) => {
        match authenticate(&$state, &$headers, &$request_id).await {
            Ok(caller) => caller,
            Err(response) => return *response,
        }
    };
}

fn org_id(raw: &str) -> Result<TenantId, WorkspaceError> {
    workspace::parse_id(raw)
}

fn project_id(raw: &str) -> Result<ApplicationId, WorkspaceError> {
    workspace::parse_id(raw)
}

// ---- sign-in routes -------------------------------------------------------------------------

async fn request_code(
    State(state): State<ConsoleApiState>,
    headers: HeaderMap,
    body: Result<Json<CodeRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request_id = request_id_from(&headers);
    if let Some(limited) = limit_ip(&state, &headers, &request_id).await {
        return limited;
    }
    let request = match json_body(body, &request_id) {
        Ok(request) => request,
        Err(response) => return *response,
    };
    let response = match state.auth.request_code(&request.email).await {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({"accepted": true})),
        )
            .into_response(),
        Err(error) => console_error(&error, &request_id),
    };
    with_request_id(response, &request_id)
}

async fn verify_code(
    State(state): State<ConsoleApiState>,
    headers: HeaderMap,
    body: Result<Json<VerifyRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request_id = request_id_from(&headers);
    if let Some(limited) = limit_ip(&state, &headers, &request_id).await {
        return limited;
    }
    let request = match json_body(body, &request_id) {
        Ok(request) => request,
        Err(response) => return *response,
    };
    let response = match state
        .auth
        .verify_code(&request.email, &request.code, request.accept_terms)
        .await
    {
        Ok(opened) => Json(serde_json::json!({
            "token": opened.token.as_str(),
            "expires_at": rfc3339(opened.expires_at),
            "account": AccountBody::from(&opened.account),
            "created": opened.created,
        }))
        .into_response(),
        Err(error) => console_error(&error, &request_id),
    };
    with_request_id(response, &request_id)
}

async fn logout(State(state): State<ConsoleApiState>, headers: HeaderMap) -> Response {
    let request_id = request_id_from(&headers);
    let caller = caller_or_return!(state, headers, request_id);
    let response = match state.auth.logout(&caller.token).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => console_error(&error, &request_id),
    };
    with_request_id(response, &request_id)
}

async fn get_me(State(state): State<ConsoleApiState>, headers: HeaderMap) -> Response {
    let request_id = request_id_from(&headers);
    let caller = caller_or_return!(state, headers, request_id);
    with_request_id(
        Json(AccountBody::from(&caller.account)).into_response(),
        &request_id,
    )
}

async fn delete_me(State(state): State<ConsoleApiState>, headers: HeaderMap) -> Response {
    let request_id = request_id_from(&headers);
    let caller = caller_or_return!(state, headers, request_id);
    let response = match state.auth.delete_account(caller.account.id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => console_error(&error, &request_id),
    };
    with_request_id(response, &request_id)
}

// ---- idempotency ----------------------------------------------------------------------------

/// What a creating request is, for the idempotency record.
struct CreateTarget<'a> {
    account: AccountId,
    /// Plays the tenant role of the idempotency table.
    scope: TenantId,
    route: &'a str,
    request: &'a Value,
}

/// Runs a creating operation under `Idempotency-Key` when the header and a cipher are present.
///
/// `scope` plays the tenant role of the idempotency table: the organization for projects and
/// keys, the account itself for organizations (which have no tenant yet). A repeated request
/// replays the stored answer, including the one-time key secret, encrypted at rest.
async fn run_create<F>(
    state: &ConsoleApiState,
    headers: &HeaderMap,
    request_id: &str,
    target: CreateTarget<'_>,
    work: F,
) -> Response
where
    F: std::future::Future<Output = StoredResponse>,
{
    let CreateTarget {
        account,
        scope,
        route,
        request,
    } = target;
    let key = headers
        .get("Idempotency-Key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim);
    let (Some(key), Some(rotator)) = (key, state.crypto_rotator.as_ref()) else {
        return render(work.await, request_id, false);
    };
    if key.is_empty()
        || key.len() > 255
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b':' | b'.'))
    {
        return validation(request_id, "Idempotency-Key");
    }
    let context = IdempotencyContext {
        tenant_id: scope,
        principal_id: account.into_uuid(),
        key,
        method: "POST",
        route,
        body: request,
    };
    match idempotency_begin(&state.pool, &context, state.idempotency_retention, rotator).await {
        Ok(IdempotencyDecision::Process) => {
            let stored = work.await;
            if stored.status >= 500 {
                let _ = gs_service::idempotency::discard(&state.pool, &context).await;
            } else {
                let _ = idempotency_finish(&state.pool, &context, &stored, rotator).await;
            }
            render(stored, request_id, false)
        }
        Ok(IdempotencyDecision::Replay(stored)) => render(stored, request_id, true),
        Ok(IdempotencyDecision::InProgress) => problem(ErrorCode::Conflict, request_id),
        Ok(IdempotencyDecision::KeyReused) => problem(ErrorCode::IdempotencyKeyReused, request_id),
        Err(_) => problem(ErrorCode::Internal, request_id),
    }
}

fn render(stored: StoredResponse, request_id: &str, replayed: bool) -> Response {
    let status = StatusCode::from_u16(stored.status).unwrap_or(StatusCode::OK);
    let mut response = (status, Json(stored.body)).into_response();
    if stored.status >= 400 {
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
    }
    if replayed {
        response.headers_mut().insert(
            axum::http::HeaderName::from_static("idempotent-replayed"),
            HeaderValue::from_static("true"),
        );
    }
    with_request_id(response, request_id)
}

/// The stored form of a failed operation: the problem body with its status.
fn failure(error: &WorkspaceError, request_id: &str) -> StoredResponse {
    let mut problem = Problem::new(error.error_code(), request_id);
    if let WorkspaceError::Validation(field) = error {
        problem = problem.with_errors(vec![FieldError {
            field: (*field).to_string(),
            reason: "invalid_format".to_string(),
        }]);
    }
    StoredResponse {
        status: problem.status,
        body: serde_json::to_value(&problem).unwrap_or(Value::Null),
    }
}

fn created(body: impl Serialize) -> StoredResponse {
    StoredResponse {
        status: StatusCode::CREATED.as_u16(),
        body: serde_json::to_value(body).unwrap_or(Value::Null),
    }
}

// ---- organizations --------------------------------------------------------------------------

async fn list_organizations(State(state): State<ConsoleApiState>, headers: HeaderMap) -> Response {
    let request_id = request_id_from(&headers);
    let caller = caller_or_return!(state, headers, request_id);
    let response = match workspace::list_organizations(&state.pool, caller.account.id).await {
        Ok(list) => Json(serde_json::json!({
            "data": list.iter().map(OrganizationBody::from).collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(error) => workspace_error(&error, &request_id),
    };
    with_request_id(response, &request_id)
}

async fn create_organization(
    State(state): State<ConsoleApiState>,
    headers: HeaderMap,
    body: Result<Json<CreateOrganizationRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = caller_or_return!(state, headers, request_id);
    let request = match json_body(body, &request_id) {
        Ok(request) => request,
        Err(response) => return *response,
    };
    let account = caller.account.id;
    let scope = TenantId::from_uuid(account.into_uuid());
    let wire = serde_json::json!({"name": request.name});
    let max = i64::from(state.max_orgs_per_account);
    run_create(
        &state,
        &headers,
        &request_id,
        CreateTarget {
            account,
            scope,
            route: "/v1/console/organizations",
            request: &wire,
        },
        async {
            match workspace::create_organization(&state.pool, account, &request.name, max).await {
                Ok(created_org) => created(OrganizationBody::from(&created_org)),
                Err(error) => failure(&error, &request_id),
            }
        },
    )
    .await
}

async fn get_organization(
    State(state): State<ConsoleApiState>,
    headers: HeaderMap,
    Path(organization_id): Path<String>,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = caller_or_return!(state, headers, request_id);
    let result = async {
        let organization = org_id(&organization_id)?;
        workspace::get_organization(&state.pool, caller.account.id, organization).await
    }
    .await;
    let response = match result {
        Ok(membership) => Json(OrganizationBody::from(&membership)).into_response(),
        Err(error) => workspace_error(&error, &request_id),
    };
    with_request_id(response, &request_id)
}

// ---- projects -------------------------------------------------------------------------------

async fn list_projects(
    State(state): State<ConsoleApiState>,
    headers: HeaderMap,
    Path(organization_id): Path<String>,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = caller_or_return!(state, headers, request_id);
    let result = async {
        let organization = org_id(&organization_id)?;
        workspace::list_projects(&state.pool, caller.account.id, organization).await
    }
    .await;
    let response = match result {
        Ok(list) => Json(serde_json::json!({
            "data": list.iter().map(ProjectBody::from).collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(error) => workspace_error(&error, &request_id),
    };
    with_request_id(response, &request_id)
}

async fn create_project(
    State(state): State<ConsoleApiState>,
    headers: HeaderMap,
    Path(organization_id): Path<String>,
    body: Result<Json<CreateProjectRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = caller_or_return!(state, headers, request_id);
    let request = match json_body(body, &request_id) {
        Ok(request) => request,
        Err(response) => return *response,
    };
    let organization = match org_id(&organization_id) {
        Ok(id) => id,
        Err(error) => return with_request_id(workspace_error(&error, &request_id), &request_id),
    };
    let account = caller.account.id;
    let wire = serde_json::json!({
        "name": request.name,
        "allowed_device_scopes": request.allowed_device_scopes,
        "allowed_origins": request.allowed_origins,
    });
    let route = format!("/v1/console/organizations/{organization}/projects");
    run_create(
        &state,
        &headers,
        &request_id,
        CreateTarget {
            account,
            scope: organization,
            route: &route,
            request: &wire,
        },
        async {
            match workspace::create_project(
                &state.pool,
                account,
                organization,
                &request.name,
                request.allowed_device_scopes.clone(),
                request.allowed_origins.clone(),
            )
            .await
            {
                Ok(app) => created(ProjectBody::from(&app)),
                Err(error) => failure(&error, &request_id),
            }
        },
    )
    .await
}

async fn get_project(
    State(state): State<ConsoleApiState>,
    headers: HeaderMap,
    Path((organization_id, project)): Path<(String, String)>,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = caller_or_return!(state, headers, request_id);
    let result = async {
        let organization = org_id(&organization_id)?;
        let project = project_id(&project)?;
        workspace::get_project(&state.pool, caller.account.id, organization, project).await
    }
    .await;
    let response = match result {
        Ok(app) => Json(ProjectBody::from(&app)).into_response(),
        Err(error) => workspace_error(&error, &request_id),
    };
    with_request_id(response, &request_id)
}

async fn update_project(
    State(state): State<ConsoleApiState>,
    headers: HeaderMap,
    Path((organization_id, project)): Path<(String, String)>,
    body: Result<Json<UpdateProjectRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = caller_or_return!(state, headers, request_id);
    let request = match json_body(body, &request_id) {
        Ok(request) => request,
        Err(response) => return *response,
    };
    let enabled = match request.status.as_deref() {
        None => None,
        Some("ACTIVE") => Some(true),
        Some("DISABLED") => Some(false),
        Some(_) => return with_request_id(validation(&request_id, "status"), &request_id),
    };
    if request.name.is_none()
        && enabled.is_none()
        && request.allowed_device_scopes.is_none()
        && request.allowed_origins.is_none()
    {
        return with_request_id(validation(&request_id, "body"), &request_id);
    }
    let result = async {
        let organization = org_id(&organization_id)?;
        let project = project_id(&project)?;
        workspace::update_project(
            &state.pool,
            caller.account.id,
            organization,
            project,
            ProjectUpdate {
                name: request.name,
                enabled,
                allowed_device_scopes: request.allowed_device_scopes,
                allowed_origins: request.allowed_origins,
            },
            Some(state.bus.as_ref()),
            Some(state.redis_key_prefix.as_str()),
        )
        .await
    }
    .await;
    let response = match result {
        Ok(app) => Json(ProjectBody::from(&app)).into_response(),
        Err(error) => workspace_error(&error, &request_id),
    };
    with_request_id(response, &request_id)
}

// ---- API keys -------------------------------------------------------------------------------

async fn list_keys(
    State(state): State<ConsoleApiState>,
    headers: HeaderMap,
    Path((organization_id, project)): Path<(String, String)>,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = caller_or_return!(state, headers, request_id);
    let result = async {
        let organization = org_id(&organization_id)?;
        let project = project_id(&project)?;
        workspace::list_api_keys(&state.pool, caller.account.id, organization, project).await
    }
    .await;
    let response = match result {
        Ok(list) => Json(serde_json::json!({
            "data": list.iter().map(KeyBody::from).collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(error) => workspace_error(&error, &request_id),
    };
    with_request_id(response, &request_id)
}

async fn create_key(
    State(state): State<ConsoleApiState>,
    headers: HeaderMap,
    Path((organization_id, project)): Path<(String, String)>,
    body: Result<Json<CreateKeyRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = caller_or_return!(state, headers, request_id);
    let request = match json_body(body, &request_id) {
        Ok(request) => request,
        Err(response) => return *response,
    };
    let (organization, project) = match (org_id(&organization_id), project_id(&project)) {
        (Ok(organization), Ok(project)) => (organization, project),
        _ => {
            return with_request_id(
                workspace_error(&WorkspaceError::NotFound, &request_id),
                &request_id,
            );
        }
    };
    let account = caller.account.id;
    let wire = serde_json::json!({
        "label": request.label,
        "scopes": request.scopes,
        "expires_in_days": request.expires_in_days,
    });
    let route = format!("/v1/console/organizations/{organization}/projects/{project}/keys");
    run_create(
        &state,
        &headers,
        &request_id,
        CreateTarget {
            account,
            scope: organization,
            route: &route,
            request: &wire,
        },
        async {
            match workspace::create_api_key(
                &state.pool,
                account,
                organization,
                project,
                &request.label,
                request.scopes.clone(),
                request.expires_in_days,
            )
            .await
            {
                // The only place the secret exists outside the caller's hands.
                Ok(key) => created(serde_json::json!({
                    "key": KeyBody::from(&key.key),
                    "secret": key.credential.as_str(),
                })),
                Err(error) => failure(&error, &request_id),
            }
        },
    )
    .await
}

async fn revoke_key(
    State(state): State<ConsoleApiState>,
    headers: HeaderMap,
    Path((organization_id, project, key)): Path<(String, String, String)>,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = caller_or_return!(state, headers, request_id);
    let result = async {
        let organization = org_id(&organization_id)?;
        let project = project_id(&project)?;
        let key: ApiKeyId = workspace::parse_id(&key)?;
        workspace::revoke_api_key(
            &state.pool,
            caller.account.id,
            organization,
            project,
            key,
            Some(state.bus.as_ref()),
            Some(state.redis_key_prefix.as_str()),
        )
        .await
    }
    .await;
    let response = match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => workspace_error(&error, &request_id),
    };
    with_request_id(response, &request_id)
}

/// Whether the role can manage projects and keys. Exposed for tests of the role matrix.
pub const fn can_manage(role: ConsoleRole) -> bool {
    role.at_least(ConsoleRole::Admin)
}
