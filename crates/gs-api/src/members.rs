//! Membership REST handlers (docs/05 §5, docs/07 §9.3, FR-MEM-001..007, FR-SES-007).
#![forbid(unsafe_code)]

use std::str::FromStr;

use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use serde_json::{Value, json};

use gs_service::gs_domain::error::{AsErrorCode, ErrorCode};
use gs_service::gs_domain::ids::{MemberId, SessionId};
use gs_service::gs_domain::pagination::PageCursor;
use gs_service::idempotency::{
    IdempotencyContext, IdempotencyDecision, begin as idempotency_begin,
    finish as idempotency_finish,
};
use gs_service::members::{
    AddMemberRequest, MemberResource, MemberUseCaseError, MembershipRepository, UpdateCaller,
    UpdateMemberRequest, add_member, get_member, get_member_for_device_me, leave_session,
    list_members, revoke_member, update_member,
};

use crate::pagination::parse_pagination_query;
use crate::problem::{Problem, ProblemResponse};
use crate::router::REQUEST_ID_HEADER;
use crate::security::request_id_from;
use crate::sessions::{CallerPrincipal, SessionApiState, authenticate_caller};

/// Builds the members routes unattached to state.
pub fn members_routes() -> axum::Router<SessionApiState> {
    axum::Router::new()
        .route(
            "/v1/sessions/{session_id}/members",
            axum::routing::post(add_member_handler).get(list_members_handler),
        )
        .route(
            "/v1/sessions/{session_id}/members/{member_id}",
            axum::routing::get(get_member_handler)
                .patch(update_member_handler)
                .delete(revoke_member_handler),
        )
        .route(
            "/v1/sessions/{session_id}/leave",
            axum::routing::post(leave_session_handler),
        )
}

/// Builds the members router with state (docs/07 §9.3).
pub fn members_router(state: SessionApiState) -> axum::Router {
    members_routes().with_state(state)
}

/// Converts a MemberUseCaseError into an HTTP response using standard Problem JSON.
fn member_error_to_response(err: MemberUseCaseError, request_id: &str) -> Response {
    let status = match err.error_code() {
        ErrorCode::ValidationFailed => StatusCode::UNPROCESSABLE_ENTITY,
        ErrorCode::InsufficientScope => StatusCode::FORBIDDEN,
        ErrorCode::PermissionDenied => StatusCode::FORBIDDEN,
        ErrorCode::NotFound => StatusCode::NOT_FOUND,
        ErrorCode::PreconditionFailed => StatusCode::PRECONDITION_FAILED,
        ErrorCode::Conflict | ErrorCode::SessionClosed | ErrorCode::SessionFull => {
            StatusCode::CONFLICT
        }
        ErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };

    let problem = Problem::new(err.error_code(), request_id).with_detail(err.to_string());
    (status, ProblemResponse::new(problem)).into_response()
}

/// `POST /v1/sessions/{session_id}/members` (OpenAPI `addMember`).
pub async fn add_member_handler(
    State(state): State<SessionApiState>,
    Path(raw_session_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<AddMemberRequest>, axum::extract::rejection::JsonRejection>,
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

    // Only application callers are allowed (OpenAPI: x-gs-auth: { application: 'members:write', device: forbidden })
    let app = match caller {
        CallerPrincipal::Application(ref app) => app,
        CallerPrincipal::Device(_) => {
            return ProblemResponse::new(
                Problem::new(ErrorCode::PermissionDenied, &request_id)
                    .with_detail("adding members directly is only allowed for application callers"),
            )
            .into_response();
        }
    };

    if !app.api_key.scopes.iter().any(|s| s == "members:write") {
        return ProblemResponse::new(
            Problem::new(ErrorCode::InsufficientScope, &request_id)
                .with_detail("application requires members:write scope"),
        )
        .into_response();
    }

    let Json(request) = match body {
        Ok(b) => b,
        Err(_) => {
            return ProblemResponse::new(
                Problem::new(ErrorCode::ValidationFailed, &request_id)
                    .with_detail("request body must be a valid AddMemberRequest JSON"),
            )
            .into_response();
        }
    };

    let tenant_id = caller.tenant_id();

    // Idempotency processing
    let idempotency_key = headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let json_body = serde_json::to_value(&request).unwrap_or(Value::Null);

    let idemp_ctx = idempotency_key.as_deref().map(|key| IdempotencyContext {
        tenant_id,
        principal_id: caller.principal_id(),
        key,
        method: "POST",
        route: "/v1/sessions/members",
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

    match add_member(&state.pool, tenant_id, session_id, &request).await {
        Ok(member) => {
            let etag_val = member.etag();
            let response_body = serde_json::to_value(&member).unwrap_or(Value::Null);

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
        Err(err) => member_error_to_response(err, &request_id),
    }
}

/// `GET /v1/sessions/{session_id}/members` (OpenAPI `listMembers`).
pub async fn list_members_handler(
    State(state): State<SessionApiState>,
    Path(raw_session_id): Path<String>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
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

    // Check authorization
    match caller {
        CallerPrincipal::Application(ref app) => {
            if !app.api_key.scopes.iter().any(|s| s == "sessions:read") {
                return ProblemResponse::new(
                    Problem::new(ErrorCode::InsufficientScope, &request_id)
                        .with_detail("application requires sessions:read scope"),
                )
                .into_response();
            }
        }
        CallerPrincipal::Device(ref dev) => {
            // Device must be active member with read or manage capability (OpenAPI: read_or_manage)
            match MembershipRepository::find_active_by_session_and_subject(
                &state.pool,
                tenant_id,
                session_id,
                dev.subject.id,
            )
            .await
            {
                Ok(Some(mem)) => {
                    if !mem.capabilities.read && !mem.capabilities.manage {
                        return ProblemResponse::new(
                            Problem::new(ErrorCode::PermissionDenied, &request_id)
                                .with_detail("member lacks read or manage capability"),
                        )
                        .into_response();
                    }
                }
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
            }
        }
    }

    let page_req = match parse_pagination_query(raw_query.as_deref()) {
        Ok(p) => p,
        Err(err) => return err.into_response(),
    };

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

    match list_members(
        &state.pool,
        tenant_id,
        session_id,
        page_req.limit,
        cursor_params,
    )
    .await
    {
        Ok(mut page) => {
            let limit = page_req.limit as usize;
            let has_more = page.members.len() > limit;
            if has_more {
                page.members.truncate(limit);
            }

            let next_cursor = if has_more {
                page.members.last().map(|m| {
                    PageCursor::new(
                        m.membership.joined_at,
                        m.membership.id.into_uuid().as_u128(),
                        0,
                    )
                    .encode()
                })
            } else {
                None
            };

            let resources: Vec<MemberResource> = page
                .members
                .iter()
                .map(|m| MemberResource::from_member_with_subject(m, false, None))
                .collect();

            let body = json!({
                "data": resources,
                "next_cursor": next_cursor,
            });

            let mut resp = (StatusCode::OK, Json(body)).into_response();
            if let Ok(v) = axum::http::HeaderValue::from_str(&request_id) {
                resp.headers_mut().insert(REQUEST_ID_HEADER, v);
            }
            resp
        }
        Err(err) => member_error_to_response(err, &request_id),
    }
}

/// `GET /v1/sessions/{session_id}/members/{member_id}` (OpenAPI `getMember`).
pub async fn get_member_handler(
    State(state): State<SessionApiState>,
    Path((raw_session_id, raw_member_id)): Path<(String, String)>,
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

    // Handle "me" path parameter
    if raw_member_id == "me" {
        let dev = match caller {
            CallerPrincipal::Device(ref dev) => dev,
            CallerPrincipal::Application(_) => {
                // docs/07 §9.3: "me solo para dispositivos (aplicación + me → 404)"
                return ProblemResponse::new(
                    Problem::new(ErrorCode::NotFound, &request_id)
                        .with_detail("'me' is only supported for device callers"),
                )
                .into_response();
            }
        };

        match get_member_for_device_me(&state.pool, tenant_id, session_id, dev.subject.id).await {
            Ok(member) => {
                let etag_val = member.etag();
                let mut resp = (StatusCode::OK, Json(member)).into_response();
                if let Ok(v) = axum::http::HeaderValue::from_str(&etag_val) {
                    resp.headers_mut().insert(header::ETAG, v);
                }
                if let Ok(v) = axum::http::HeaderValue::from_str(&request_id) {
                    resp.headers_mut().insert(REQUEST_ID_HEADER, v);
                }
                resp
            }
            Err(err) => member_error_to_response(err, &request_id),
        }
    } else {
        let target_member_id = match MemberId::from_str(&raw_member_id) {
            Ok(id) => id,
            Err(_) => {
                return ProblemResponse::new(
                    Problem::new(ErrorCode::NotFound, &request_id)
                        .with_detail(format!("member '{raw_member_id}' not found")),
                )
                .into_response();
            }
        };

        // Capability checks: read_or_manage_or_self
        match caller {
            CallerPrincipal::Application(ref app) => {
                if !app.api_key.scopes.iter().any(|s| s == "sessions:read") {
                    return ProblemResponse::new(
                        Problem::new(ErrorCode::InsufficientScope, &request_id)
                            .with_detail("application requires sessions:read scope"),
                    )
                    .into_response();
                }
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
                            Problem::new(ErrorCode::Internal, &request_id)
                                .with_detail(err.to_string()),
                        )
                        .into_response();
                    }
                };

                let can_view = caller_mem.id == target_member_id
                    || caller_mem.capabilities.read
                    || caller_mem.capabilities.manage;

                if !can_view {
                    return ProblemResponse::new(
                        Problem::new(ErrorCode::PermissionDenied, &request_id)
                            .with_detail("member lacks read or manage capability"),
                    )
                    .into_response();
                }
            }
        }

        match get_member(&state.pool, tenant_id, session_id, target_member_id).await {
            Ok(member) => {
                let etag_val = member.etag();
                let mut resp = (StatusCode::OK, Json(member)).into_response();
                if let Ok(v) = axum::http::HeaderValue::from_str(&etag_val) {
                    resp.headers_mut().insert(header::ETAG, v);
                }
                if let Ok(v) = axum::http::HeaderValue::from_str(&request_id) {
                    resp.headers_mut().insert(REQUEST_ID_HEADER, v);
                }
                resp
            }
            Err(err) => member_error_to_response(err, &request_id),
        }
    }
}

/// `PATCH /v1/sessions/{session_id}/members/{member_id}` (OpenAPI `updateMember`).
pub async fn update_member_handler(
    State(state): State<SessionApiState>,
    Path((raw_session_id, raw_member_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Result<Json<UpdateMemberRequest>, axum::extract::rejection::JsonRejection>,
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

    let target_member_id = if raw_member_id == "me" {
        match caller {
            CallerPrincipal::Device(ref dev) => {
                match MembershipRepository::find_active_by_session_and_subject(
                    &state.pool,
                    tenant_id,
                    session_id,
                    dev.subject.id,
                )
                .await
                {
                    Ok(Some(m)) => m.id,
                    Ok(None) => {
                        return ProblemResponse::new(
                            Problem::new(ErrorCode::NotFound, &request_id)
                                .with_detail("caller is not an active member of this session"),
                        )
                        .into_response();
                    }
                    Err(err) => {
                        return ProblemResponse::new(
                            Problem::new(ErrorCode::Internal, &request_id)
                                .with_detail(err.to_string()),
                        )
                        .into_response();
                    }
                }
            }
            CallerPrincipal::Application(_) => {
                return ProblemResponse::new(
                    Problem::new(ErrorCode::NotFound, &request_id)
                        .with_detail("'me' is only supported for device callers"),
                )
                .into_response();
            }
        }
    } else {
        match MemberId::from_str(&raw_member_id) {
            Ok(id) => id,
            Err(_) => {
                return ProblemResponse::new(
                    Problem::new(ErrorCode::NotFound, &request_id)
                        .with_detail(format!("member '{raw_member_id}' not found")),
                )
                .into_response();
            }
        }
    };

    let Json(request) = match body {
        Ok(b) => b,
        Err(_) => {
            return ProblemResponse::new(
                Problem::new(ErrorCode::ValidationFailed, &request_id)
                    .with_detail("request body must be a valid UpdateMemberRequest JSON"),
            )
            .into_response();
        }
    };

    // Parse If-Match header
    let if_match_val = headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(str::trim);

    let expected_version: Option<u64> = match if_match_val {
        Some(val) => {
            let clean = val.trim_start_matches("W/").trim_matches('"');
            match clean.parse() {
                Ok(v) => Some(v),
                Err(_) => {
                    return ProblemResponse::new(
                        Problem::new(ErrorCode::PreconditionFailed, &request_id)
                            .with_detail(format!("invalid If-Match header: '{val}'")),
                    )
                    .into_response();
                }
            }
        }
        None => None,
    };

    // Verify caller permissions
    let device_mem;
    let update_caller = match caller {
        CallerPrincipal::Application(ref app) => {
            if !app.api_key.scopes.iter().any(|s| s == "members:write") {
                return ProblemResponse::new(
                    Problem::new(ErrorCode::InsufficientScope, &request_id)
                        .with_detail("application requires members:write scope"),
                )
                .into_response();
            }
            UpdateCaller::Application
        }
        CallerPrincipal::Device(ref dev) => {
            if request.role.is_some() {
                return ProblemResponse::new(
                    Problem::new(ErrorCode::PermissionDenied, &request_id)
                        .with_detail("role modifications are only allowed for application callers"),
                )
                .into_response();
            }

            let mem = match MembershipRepository::find_active_by_session_and_subject(
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

            device_mem = mem;
            UpdateCaller::Device(&device_mem)
        }
    };

    match update_member(
        &state.pool,
        tenant_id,
        session_id,
        target_member_id,
        expected_version,
        &request,
        update_caller,
    )
    .await
    {
        Ok(member) => {
            let etag_val = member.etag();
            let mut resp = (StatusCode::OK, Json(member)).into_response();
            if let Ok(v) = axum::http::HeaderValue::from_str(&etag_val) {
                resp.headers_mut().insert(header::ETAG, v);
            }
            if let Ok(v) = axum::http::HeaderValue::from_str(&request_id) {
                resp.headers_mut().insert(REQUEST_ID_HEADER, v);
            }
            resp
        }
        Err(err) => member_error_to_response(err, &request_id),
    }
}

/// `DELETE /v1/sessions/{session_id}/members/{member_id}` (OpenAPI `revokeMember`).
pub async fn revoke_member_handler(
    State(state): State<SessionApiState>,
    Path((raw_session_id, raw_member_id)): Path<(String, String)>,
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

    let target_member_id = match MemberId::from_str(&raw_member_id) {
        Ok(id) => id,
        Err(_) => {
            return ProblemResponse::new(
                Problem::new(ErrorCode::NotFound, &request_id)
                    .with_detail(format!("member '{raw_member_id}' not found")),
            )
            .into_response();
        }
    };

    let tenant_id = caller.tenant_id();

    // Verify caller permissions (application with members:write or device with manage)
    match caller {
        CallerPrincipal::Application(ref app) => {
            if !app.api_key.scopes.iter().any(|s| s == "members:write") {
                return ProblemResponse::new(
                    Problem::new(ErrorCode::InsufficientScope, &request_id)
                        .with_detail("application requires members:write scope"),
                )
                .into_response();
            }
        }
        CallerPrincipal::Device(ref dev) => {
            match MembershipRepository::find_active_by_session_and_subject(
                &state.pool,
                tenant_id,
                session_id,
                dev.subject.id,
            )
            .await
            {
                Ok(Some(mem)) => {
                    if !mem.capabilities.manage {
                        return ProblemResponse::new(
                            Problem::new(ErrorCode::PermissionDenied, &request_id)
                                .with_detail("member lacks manage capability"),
                        )
                        .into_response();
                    }
                }
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
            }
        }
    }

    match revoke_member(&state.pool, tenant_id, session_id, target_member_id).await {
        Ok(()) => {
            let mut resp = StatusCode::NO_CONTENT.into_response();
            if let Ok(v) = axum::http::HeaderValue::from_str(&request_id) {
                resp.headers_mut().insert(REQUEST_ID_HEADER, v);
            }
            resp
        }
        Err(err) => member_error_to_response(err, &request_id),
    }
}

/// `POST /v1/sessions/{session_id}/leave` (OpenAPI `leaveSession`).
pub async fn leave_session_handler(
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

    // Device callers only (OpenAPI: x-gs-auth: { application: forbidden, device: none })
    let dev = match caller {
        CallerPrincipal::Device(ref dev) => dev,
        CallerPrincipal::Application(_) => {
            return ProblemResponse::new(
                Problem::new(ErrorCode::PermissionDenied, &request_id)
                    .with_detail("leave is only allowed for device callers"),
            )
            .into_response();
        }
    };

    let tenant_id = caller.tenant_id();

    match leave_session(&state.pool, tenant_id, session_id, dev.subject.id).await {
        Ok(()) => {
            let mut resp = StatusCode::NO_CONTENT.into_response();
            if let Ok(v) = axum::http::HeaderValue::from_str(&request_id) {
                resp.headers_mut().insert(REQUEST_ID_HEADER, v);
            }
            resp
        }
        Err(err) => member_error_to_response(err, &request_id),
    }
}
