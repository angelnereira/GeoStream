//! Invitation endpoints (docs/05 §6, docs/07 §9.4, FR-INV-001..003).
#![forbid(unsafe_code)]

use std::str::FromStr;

use axum::Json;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use time::OffsetDateTime;

use gs_service::gs_domain::error::{AsErrorCode, ErrorCode};
use gs_service::gs_domain::ids::{InvitationId, SessionId};
use gs_service::gs_store::repos::MembershipRepository;
use gs_service::idempotency::{
    IdempotencyContext, IdempotencyDecision, StoredResponse, begin as idempotency_begin,
    discard as idempotency_discard, finish as idempotency_finish,
};
use gs_service::invitations::{
    CreateInvitationCaller, CreateInvitationRequest, InvitationUseCaseError, ListInvitationsCaller,
    RedeemInvitationParams, RedeemInvitationRequest, RevokeInvitationCaller, create_invitation,
    list_invitations, redeem_invitation, revoke_invitation,
};

use crate::pagination::parse_pagination_query;
use crate::problem::{Problem, ProblemResponse};
use crate::router::REQUEST_ID_HEADER;
use crate::security::request_id_from;
use crate::sessions::{CallerPrincipal, SessionApiState, authenticate_caller};

/// Maps an `InvitationUseCaseError` to an HTTP `Response`.
fn invitation_error_to_response(err: InvitationUseCaseError, request_id: &str) -> Response {
    let status = match err.error_code() {
        ErrorCode::ValidationFailed => StatusCode::UNPROCESSABLE_ENTITY,
        ErrorCode::InsufficientScope | ErrorCode::PermissionDenied => StatusCode::FORBIDDEN,
        ErrorCode::NotFound | ErrorCode::InvitationInvalid => StatusCode::NOT_FOUND,
        ErrorCode::RateLimited => {
            if let InvitationUseCaseError::RateLimited { retry_after } = err {
                let problem =
                    Problem::new(ErrorCode::RateLimited, request_id).with_detail(err.to_string());
                let mut resp =
                    (StatusCode::TOO_MANY_REQUESTS, ProblemResponse::new(problem)).into_response();
                if let Ok(v) = axum::http::HeaderValue::from_str(&retry_after.to_string()) {
                    resp.headers_mut().insert(header::RETRY_AFTER, v);
                }
                if let Ok(v) = axum::http::HeaderValue::from_str(request_id) {
                    resp.headers_mut().insert(REQUEST_ID_HEADER, v);
                }
                return resp;
            }
            StatusCode::TOO_MANY_REQUESTS
        }
        ErrorCode::Conflict | ErrorCode::SessionClosed | ErrorCode::SessionFull => {
            StatusCode::CONFLICT
        }
        ErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };

    let problem = Problem::new(err.error_code(), request_id).with_detail(err.to_string());
    let mut resp = (status, ProblemResponse::new(problem)).into_response();
    if let Ok(v) = axum::http::HeaderValue::from_str(request_id) {
        resp.headers_mut().insert(REQUEST_ID_HEADER, v);
    }
    resp
}

/// Builds routes for invitations (`docs/07` §9.4).
pub fn invitations_routes() -> axum::Router<SessionApiState> {
    axum::Router::new()
        .route(
            "/v1/sessions/{session_id}/invitations",
            axum::routing::post(create_invitation_handler).get(list_invitations_handler),
        )
        .route(
            "/v1/sessions/{session_id}/invitations/{invitation_id}",
            axum::routing::delete(revoke_invitation_handler),
        )
        .route(
            "/v1/invitations/redeem",
            axum::routing::post(redeem_invitation_handler),
        )
}

/// `POST /v1/sessions/{session_id}/invitations` (OpenAPI `createInvitation`).
pub async fn create_invitation_handler(
    State(state): State<SessionApiState>,
    Path(raw_session_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<CreateInvitationRequest>, axum::extract::rejection::JsonRejection>,
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

    // Check caller authorization (x-gs-auth: { application: 'invitations:write', device: none }, x-gs-capability: invite)
    let invitation_caller = match caller {
        CallerPrincipal::Application(ref app) => {
            if !app.api_key.scopes.iter().any(|s| s == "invitations:write") {
                return ProblemResponse::new(
                    Problem::new(ErrorCode::InsufficientScope, &request_id)
                        .with_detail("application requires invitations:write scope"),
                )
                .into_response();
            }
            CreateInvitationCaller::Application
        }
        CallerPrincipal::Device(ref dev) => {
            let caller_mem = match MembershipRepository::find_active_by_session_and_subject(
                &state.pool,
                tenant_id,
                session_id,
                dev.subject.id,
            )
            .await
            {
                Ok(Some(m)) => m,
                Ok(None) => {
                    return ProblemResponse::new(
                        Problem::new(ErrorCode::NotFound, &request_id)
                            .with_detail("caller is not an active member of this session"),
                    )
                    .into_response();
                }
                Err(err) => {
                    return ProblemResponse::new(
                        Problem::new(ErrorCode::Internal, &request_id).with_detail(err.to_string()),
                    )
                    .into_response();
                }
            };

            if !caller_mem.capabilities.invite {
                return ProblemResponse::new(
                    Problem::new(ErrorCode::PermissionDenied, &request_id)
                        .with_detail("caller lacks invite capability"),
                )
                .into_response();
            }

            CreateInvitationCaller::Device {
                device_id: dev.device.id,
                subject_id: dev.subject.id,
                member_id: caller_mem.id,
                role: caller_mem.role,
                capabilities: caller_mem.capabilities,
            }
        }
    };

    let Json(request) = match body {
        Ok(b) => b,
        Err(_) => {
            return ProblemResponse::new(
                Problem::new(ErrorCode::ValidationFailed, &request_id)
                    .with_detail("request body must be a valid CreateInvitationRequest JSON"),
            )
            .into_response();
        }
    };

    // Idempotency processing (docs/07 §6)
    let idempotency_key = headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim);

    let json_body = serde_json::to_value(&request).unwrap_or(Value::Null);
    let route = format!("/v1/sessions/{raw_session_id}/invitations");

    let idemp_ctx = idempotency_key.map(|key| IdempotencyContext {
        tenant_id,
        principal_id: caller.principal_id(),
        key,
        method: "POST",
        route: &route,
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
    let now = OffsetDateTime::now_utc();

    match create_invitation(
        &state.pool,
        tenant_id,
        session_id,
        &request,
        invitation_caller,
        &tenant_policy,
        now,
    )
    .await
    {
        Ok(created) => {
            let json_created = serde_json::to_value(&created).unwrap_or(Value::Null);
            if let (Some(ctx), Some(rotator)) = (&idemp_ctx, &state.crypto_rotator) {
                let stored = StoredResponse {
                    status: StatusCode::CREATED.as_u16(),
                    body: json_created.clone(),
                };
                let _ = idempotency_finish(&state.pool, ctx, &stored, rotator).await;
            }

            let mut resp = (StatusCode::CREATED, Json(created)).into_response();
            if let Ok(v) = axum::http::HeaderValue::from_str(&request_id) {
                resp.headers_mut().insert(REQUEST_ID_HEADER, v);
            }
            resp
        }
        Err(err) => {
            if let Some(ctx) = &idemp_ctx {
                let status = match err.error_code() {
                    ErrorCode::ValidationFailed => StatusCode::UNPROCESSABLE_ENTITY,
                    ErrorCode::InsufficientScope | ErrorCode::PermissionDenied => {
                        StatusCode::FORBIDDEN
                    }
                    ErrorCode::NotFound => StatusCode::NOT_FOUND,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                if status.is_server_error() {
                    let _ = idempotency_discard(&state.pool, ctx).await;
                } else if let Some(rotator) = &state.crypto_rotator {
                    let problem_body = json!({
                        "type": format!("https://geostream.example.com/errors/{}", err.error_code().as_str().to_ascii_lowercase().replace('_', "-")),
                        "title": err.error_code().as_str(),
                        "status": status.as_u16(),
                        "code": err.error_code().as_str(),
                        "detail": err.to_string(),
                        "request_id": request_id,
                    });
                    let stored = StoredResponse {
                        status: status.as_u16(),
                        body: problem_body,
                    };
                    let _ = idempotency_finish(&state.pool, ctx, &stored, rotator).await;
                }
            }

            invitation_error_to_response(err, &request_id)
        }
    }
}

/// `GET /v1/sessions/{session_id}/invitations` (OpenAPI `listInvitations`).
pub async fn list_invitations_handler(
    State(state): State<SessionApiState>,
    Path(raw_session_id): Path<String>,
    RawQuery(raw_query): RawQuery,
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

    // Check caller authorization (manage_or_invite_own)
    let list_caller = match caller {
        CallerPrincipal::Application(ref app) => {
            if !app.api_key.scopes.iter().any(|s| s == "invitations:write") {
                return ProblemResponse::new(
                    Problem::new(ErrorCode::InsufficientScope, &request_id)
                        .with_detail("application requires invitations:write scope"),
                )
                .into_response();
            }
            ListInvitationsCaller::Application
        }
        CallerPrincipal::Device(ref dev) => {
            let caller_mem = match MembershipRepository::find_active_by_session_and_subject(
                &state.pool,
                tenant_id,
                session_id,
                dev.subject.id,
            )
            .await
            {
                Ok(Some(m)) => m,
                Ok(None) => {
                    return ProblemResponse::new(
                        Problem::new(ErrorCode::NotFound, &request_id)
                            .with_detail("caller is not an active member of this session"),
                    )
                    .into_response();
                }
                Err(err) => {
                    return ProblemResponse::new(
                        Problem::new(ErrorCode::Internal, &request_id).with_detail(err.to_string()),
                    )
                    .into_response();
                }
            };

            ListInvitationsCaller::Device {
                member_id: caller_mem.id,
                capabilities: caller_mem.capabilities,
            }
        }
    };

    let page_req = match parse_pagination_query(raw_query.as_deref()) {
        Ok(p) => p,
        Err(err) => return err.into_response(),
    };
    let now = OffsetDateTime::now_utc();

    match list_invitations(
        &state.pool,
        tenant_id,
        session_id,
        page_req.limit,
        page_req.cursor,
        list_caller,
        now,
    )
    .await
    {
        Ok(list) => {
            let mut resp = (StatusCode::OK, Json(list)).into_response();
            if let Ok(v) = axum::http::HeaderValue::from_str(&request_id) {
                resp.headers_mut().insert(REQUEST_ID_HEADER, v);
            }
            resp
        }
        Err(err) => invitation_error_to_response(err, &request_id),
    }
}

/// `DELETE /v1/sessions/{session_id}/invitations/{invitation_id}` (OpenAPI `revokeInvitation`).
pub async fn revoke_invitation_handler(
    State(state): State<SessionApiState>,
    Path((raw_session_id, raw_invitation_id)): Path<(String, String)>,
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

    let invitation_id = match InvitationId::from_str(&raw_invitation_id) {
        Ok(id) => id,
        Err(_) => {
            return ProblemResponse::new(
                Problem::new(ErrorCode::NotFound, &request_id)
                    .with_detail(format!("invitation '{raw_invitation_id}' not found")),
            )
            .into_response();
        }
    };

    let tenant_id = caller.tenant_id();

    // Check caller authorization (manage_or_issuer)
    let revoke_caller = match caller {
        CallerPrincipal::Application(ref app) => {
            if !app.api_key.scopes.iter().any(|s| s == "invitations:write") {
                return ProblemResponse::new(
                    Problem::new(ErrorCode::InsufficientScope, &request_id)
                        .with_detail("application requires invitations:write scope"),
                )
                .into_response();
            }
            RevokeInvitationCaller::Application
        }
        CallerPrincipal::Device(ref dev) => {
            let caller_mem = match MembershipRepository::find_active_by_session_and_subject(
                &state.pool,
                tenant_id,
                session_id,
                dev.subject.id,
            )
            .await
            {
                Ok(Some(m)) => m,
                Ok(None) => {
                    return ProblemResponse::new(
                        Problem::new(ErrorCode::NotFound, &request_id)
                            .with_detail("caller is not an active member of this session"),
                    )
                    .into_response();
                }
                Err(err) => {
                    return ProblemResponse::new(
                        Problem::new(ErrorCode::Internal, &request_id).with_detail(err.to_string()),
                    )
                    .into_response();
                }
            };

            RevokeInvitationCaller::Device {
                device_id: dev.device.id,
                member_id: caller_mem.id,
                capabilities: caller_mem.capabilities,
            }
        }
    };

    let now = OffsetDateTime::now_utc();

    match revoke_invitation(
        &state.pool,
        tenant_id,
        session_id,
        invitation_id,
        revoke_caller,
        now,
    )
    .await
    {
        Ok(()) => {
            let mut resp = StatusCode::NO_CONTENT.into_response();
            if let Ok(v) = axum::http::HeaderValue::from_str(&request_id) {
                resp.headers_mut().insert(REQUEST_ID_HEADER, v);
            }
            resp
        }
        Err(err) => invitation_error_to_response(err, &request_id),
    }
}

/// `POST /v1/invitations/redeem` (OpenAPI `redeemInvitation`).
pub async fn redeem_invitation_handler(
    State(state): State<SessionApiState>,
    headers: HeaderMap,
    body: Result<Json<RedeemInvitationRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };

    // Device only (OpenAPI: x-gs-auth: { application: forbidden, device: 'invitations:redeem' })
    let dev = match caller {
        CallerPrincipal::Device(ref dev) => dev,
        CallerPrincipal::Application(_) => {
            return ProblemResponse::new(
                Problem::new(ErrorCode::PermissionDenied, &request_id)
                    .with_detail("redeem is only allowed for device callers"),
            )
            .into_response();
        }
    };

    // Verify invitations:redeem scope
    if !dev.claims.scp.iter().any(|s| s == "invitations:redeem") {
        return ProblemResponse::new(
            Problem::new(ErrorCode::InsufficientScope, &request_id)
                .with_detail("device token requires invitations:redeem scope"),
        )
        .into_response();
    }

    let Json(request) = match body {
        Ok(b) => b,
        Err(_) => {
            return ProblemResponse::new(
                Problem::new(ErrorCode::ValidationFailed, &request_id)
                    .with_detail("request body must be a valid RedeemInvitationRequest JSON"),
            )
            .into_response();
        }
    };

    let tenant_id = caller.tenant_id();

    // Idempotency processing (docs/07 §6)
    let idempotency_key = headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim);

    let json_body = serde_json::to_value(&request).unwrap_or(Value::Null);
    let route = "/v1/invitations/redeem";

    let idemp_ctx = idempotency_key.map(|key| IdempotencyContext {
        tenant_id,
        principal_id: caller.principal_id(),
        key,
        method: "POST",
        route,
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

    let now = OffsetDateTime::now_utc();
    let rate_limit_rpm = 10u32; // GS_INVITE_REDEEM_RPM default

    match redeem_invitation(RedeemInvitationParams {
        pool: &state.pool,
        ephemeral: state.ephemeral.as_ref(),
        key_prefix: &state.redis_key_prefix,
        tenant_id,
        code: &request.code,
        device_id: dev.device.id,
        subject_id: dev.subject.id,
        rate_limit_rpm,
        now,
    })
    .await
    {
        Ok(redeemed) => {
            let etag_val = redeemed.member.etag();

            let json_redeemed = serde_json::to_value(&redeemed).unwrap_or(Value::Null);
            if let (Some(ctx), Some(rotator)) = (&idemp_ctx, &state.crypto_rotator) {
                let stored = StoredResponse {
                    status: StatusCode::CREATED.as_u16(),
                    body: json_redeemed.clone(),
                };
                let _ = idempotency_finish(&state.pool, ctx, &stored, rotator).await;
            }

            let mut resp = (StatusCode::CREATED, Json(redeemed)).into_response();
            if let Ok(v) = axum::http::HeaderValue::from_str(&etag_val) {
                resp.headers_mut().insert(header::ETAG, v);
            }
            if let Ok(v) = axum::http::HeaderValue::from_str(&request_id) {
                resp.headers_mut().insert(REQUEST_ID_HEADER, v);
            }
            resp
        }
        Err(err) => {
            if let Some(ctx) = &idemp_ctx {
                let status = match err.error_code() {
                    ErrorCode::ValidationFailed => StatusCode::UNPROCESSABLE_ENTITY,
                    ErrorCode::InsufficientScope | ErrorCode::PermissionDenied => {
                        StatusCode::FORBIDDEN
                    }
                    ErrorCode::NotFound | ErrorCode::InvitationInvalid => StatusCode::NOT_FOUND,
                    ErrorCode::Conflict | ErrorCode::SessionFull => StatusCode::CONFLICT,
                    ErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                if status.is_server_error() {
                    let _ = idempotency_discard(&state.pool, ctx).await;
                } else if let Some(rotator) = &state.crypto_rotator {
                    let problem_body = json!({
                        "type": format!("https://geostream.example.com/errors/{}", err.error_code().as_str().to_ascii_lowercase().replace('_', "-")),
                        "title": err.error_code().as_str(),
                        "status": status.as_u16(),
                        "code": err.error_code().as_str(),
                        "detail": err.to_string(),
                        "request_id": request_id,
                    });
                    let stored = StoredResponse {
                        status: status.as_u16(),
                        body: problem_body,
                    };
                    let _ = idempotency_finish(&state.pool, ctx, &stored, rotator).await;
                }
            }

            invitation_error_to_response(err, &request_id)
        }
    }
}
