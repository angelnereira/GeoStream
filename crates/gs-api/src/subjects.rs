//! Handlers for subjects and devices (docs/06 §7, docs/07 §9.5, FR-AUTH-005).
#![forbid(unsafe_code)]

use std::str::FromStr;

use axum::Json;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use gs_service::Bus;
use gs_service::gs_domain::error::{AsErrorCode, ErrorCode};
use gs_service::gs_domain::ids::{DeviceId, ExternalSubjectId, SubjectId};
use gs_service::subjects::{
    DeactivateDeviceParams, SubjectListResource, SubjectUseCaseError, deactivate_device,
    find_subject_by_external_id, get_device, get_subject, revoke_subject_tokens,
};
use time::OffsetDateTime;

use crate::problem::{Problem, ProblemResponse};
use crate::router::REQUEST_ID_HEADER;
use crate::security::request_id_from;
use crate::sessions::{CallerPrincipal, SessionApiState, authenticate_caller};

/// Builds routes for subjects and devices (`docs/07` §9.5).
pub fn subjects_routes() -> axum::Router<SessionApiState> {
    axum::Router::new()
        .route("/v1/subjects", axum::routing::get(find_subjects_handler))
        .route(
            "/v1/subjects/{subject_id}",
            axum::routing::get(get_subject_handler),
        )
        .route(
            "/v1/subjects/{subject_id}/revoke-tokens",
            axum::routing::post(revoke_subject_tokens_handler),
        )
        .route(
            "/v1/devices/{device_id}",
            axum::routing::get(get_device_handler).delete(deactivate_device_handler),
        )
}

/// Helper to attach mandatory response headers (`Cache-Control: no-store` and `X-Request-Id`).
fn with_standard_headers(mut response: Response, request_id: &str) -> Response {
    if let Ok(v) = axum::http::HeaderValue::from_str(request_id) {
        response.headers_mut().insert(REQUEST_ID_HEADER, v);
    }
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

/// Maps a SubjectUseCaseError to a ProblemResponse.
fn subject_error_to_response(error: SubjectUseCaseError, request_id: &str) -> Response {
    let code = error.error_code();
    let detail = error.to_string();
    let status = match code {
        ErrorCode::NotFound => StatusCode::NOT_FOUND,
        ErrorCode::InsufficientScope | ErrorCode::PermissionDenied => StatusCode::FORBIDDEN,
        ErrorCode::ValidationFailed => StatusCode::UNPROCESSABLE_ENTITY,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    let problem = Problem::new(code, request_id).with_detail(detail);
    let resp = (status, ProblemResponse::new(problem)).into_response();
    with_standard_headers(resp, request_id)
}

/// Simple percent-decode for query parameter values.
fn percent_decode(value: &str) -> String {
    if !value.contains('%') {
        return value.to_string();
    }
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// `GET /v1/subjects?external_subject_id=...` (OpenAPI `findSubjects`).
pub async fn find_subjects_handler(
    State(state): State<SessionApiState>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };

    // Check caller authorization (x-gs-auth: { application: 'subjects:manage', device: forbidden })
    match caller {
        CallerPrincipal::Application(ref app) => {
            if !app.api_key.scopes.iter().any(|s| s == "subjects:manage") {
                let problem = Problem::new(ErrorCode::InsufficientScope, &request_id)
                    .with_detail("application requires subjects:manage scope");
                return with_standard_headers(
                    (StatusCode::FORBIDDEN, ProblemResponse::new(problem)).into_response(),
                    &request_id,
                );
            }
        }
        CallerPrincipal::Device(_) => {
            let problem = Problem::new(ErrorCode::PermissionDenied, &request_id)
                .with_detail("devices cannot search subjects");
            return with_standard_headers(
                (StatusCode::FORBIDDEN, ProblemResponse::new(problem)).into_response(),
                &request_id,
            );
        }
    }

    let tenant_id = caller.tenant_id();

    // Parse and validate query string (docs/07 §1, docs/09 §2)
    let query_str = raw_query.unwrap_or_default();
    if query_str.is_empty() {
        let problem = Problem::new(ErrorCode::ValidationFailed, &request_id)
            .with_detail("missing required query parameter 'external_subject_id'");
        return with_standard_headers(
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                ProblemResponse::new(problem),
            )
                .into_response(),
            &request_id,
        );
    }

    let mut external_subject_id: Option<String> = None;
    for pair in query_str.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        match key {
            "external_subject_id" => {
                if external_subject_id.is_some() {
                    let problem = Problem::new(ErrorCode::ValidationFailed, &request_id)
                        .with_detail("duplicate parameter 'external_subject_id'");
                    return with_standard_headers(
                        (
                            StatusCode::UNPROCESSABLE_ENTITY,
                            ProblemResponse::new(problem),
                        )
                            .into_response(),
                        &request_id,
                    );
                }
                external_subject_id = Some(percent_decode(value));
            }
            unknown => {
                let problem = Problem::new(ErrorCode::ValidationFailed, &request_id)
                    .with_detail(format!("unknown query parameter '{unknown}'"));
                return with_standard_headers(
                    (
                        StatusCode::UNPROCESSABLE_ENTITY,
                        ProblemResponse::new(problem),
                    )
                        .into_response(),
                    &request_id,
                );
            }
        }
    }

    let raw_ext_id = match external_subject_id {
        Some(id) if !id.is_empty() => id,
        _ => {
            let problem = Problem::new(ErrorCode::ValidationFailed, &request_id)
                .with_detail("missing required query parameter 'external_subject_id'");
            return with_standard_headers(
                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    ProblemResponse::new(problem),
                )
                    .into_response(),
                &request_id,
            );
        }
    };

    if let Err(e) = ExternalSubjectId::new(&raw_ext_id) {
        let problem = Problem::new(ErrorCode::ValidationFailed, &request_id)
            .with_detail(format!("invalid external_subject_id: {e}"));
        return with_standard_headers(
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                ProblemResponse::new(problem),
            )
                .into_response(),
            &request_id,
        );
    }

    match find_subject_by_external_id(&state.pool, tenant_id, &raw_ext_id).await {
        Ok(maybe_subject) => {
            let data = match maybe_subject {
                Some(sub) => vec![sub],
                None => Vec::new(),
            };
            let list = SubjectListResource {
                data,
                next_cursor: None,
            };
            with_standard_headers(Json(list).into_response(), &request_id)
        }
        Err(e) => subject_error_to_response(e, &request_id),
    }
}

/// `GET /v1/subjects/{subject_id}` (OpenAPI `getSubject`).
pub async fn get_subject_handler(
    State(state): State<SessionApiState>,
    Path(raw_subject_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };

    match caller {
        CallerPrincipal::Application(ref app) => {
            if !app.api_key.scopes.iter().any(|s| s == "subjects:manage") {
                let problem = Problem::new(ErrorCode::InsufficientScope, &request_id)
                    .with_detail("application requires subjects:manage scope");
                return with_standard_headers(
                    (StatusCode::FORBIDDEN, ProblemResponse::new(problem)).into_response(),
                    &request_id,
                );
            }
        }
        CallerPrincipal::Device(_) => {
            let problem = Problem::new(ErrorCode::PermissionDenied, &request_id)
                .with_detail("devices cannot fetch subjects directly");
            return with_standard_headers(
                (StatusCode::FORBIDDEN, ProblemResponse::new(problem)).into_response(),
                &request_id,
            );
        }
    }

    let tenant_id = caller.tenant_id();

    let subject_id = match SubjectId::from_str(&raw_subject_id) {
        Ok(id) => id,
        Err(_) => {
            let problem = Problem::new(ErrorCode::NotFound, &request_id)
                .with_detail(format!("subject '{raw_subject_id}' not found"));
            return with_standard_headers(
                (StatusCode::NOT_FOUND, ProblemResponse::new(problem)).into_response(),
                &request_id,
            );
        }
    };

    match get_subject(&state.pool, tenant_id, subject_id).await {
        Ok(sub) => with_standard_headers(Json(sub).into_response(), &request_id),
        Err(e) => subject_error_to_response(e, &request_id),
    }
}

/// `POST /v1/subjects/{subject_id}/revoke-tokens` (OpenAPI `revokeSubjectTokens`).
pub async fn revoke_subject_tokens_handler(
    State(state): State<SessionApiState>,
    Path(raw_subject_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };

    match caller {
        CallerPrincipal::Application(ref app) => {
            if !app.api_key.scopes.iter().any(|s| s == "subjects:manage") {
                let problem = Problem::new(ErrorCode::InsufficientScope, &request_id)
                    .with_detail("application requires subjects:manage scope");
                return with_standard_headers(
                    (StatusCode::FORBIDDEN, ProblemResponse::new(problem)).into_response(),
                    &request_id,
                );
            }
        }
        CallerPrincipal::Device(_) => {
            let problem = Problem::new(ErrorCode::PermissionDenied, &request_id)
                .with_detail("devices cannot revoke subject tokens");
            return with_standard_headers(
                (StatusCode::FORBIDDEN, ProblemResponse::new(problem)).into_response(),
                &request_id,
            );
        }
    }

    let tenant_id = caller.tenant_id();

    let subject_id = match SubjectId::from_str(&raw_subject_id) {
        Ok(id) => id,
        Err(_) => {
            let problem = Problem::new(ErrorCode::NotFound, &request_id)
                .with_detail(format!("subject '{raw_subject_id}' not found"));
            return with_standard_headers(
                (StatusCode::NOT_FOUND, ProblemResponse::new(problem)).into_response(),
                &request_id,
            );
        }
    };

    let now = OffsetDateTime::now_utc();
    match revoke_subject_tokens(&state.pool, tenant_id, subject_id, now).await {
        Ok(sub) => {
            let topic = format!("{}:bus:global", state.redis_key_prefix);
            let payload = serde_json::to_vec(&serde_json::json!({
                "event": "SubjectTokensRevoked",
                "tenant_id": tenant_id.to_string(),
                "subject_id": subject_id.to_string(),
                "token_epoch": sub.token_epoch,
                "at": now.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
            }))
            .unwrap_or_default();
            let _ = state.bus.publish(&topic, &payload).await;
            with_standard_headers(Json(sub).into_response(), &request_id)
        }
        Err(e) => subject_error_to_response(e, &request_id),
    }
}

/// `GET /v1/devices/{device_id}` (OpenAPI `getDevice`).
pub async fn get_device_handler(
    State(state): State<SessionApiState>,
    Path(raw_device_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };

    let caller_subject_id_if_device = match caller {
        CallerPrincipal::Application(ref app) => {
            if !app.api_key.scopes.iter().any(|s| s == "subjects:manage") {
                let problem = Problem::new(ErrorCode::InsufficientScope, &request_id)
                    .with_detail("application requires subjects:manage scope");
                return with_standard_headers(
                    (StatusCode::FORBIDDEN, ProblemResponse::new(problem)).into_response(),
                    &request_id,
                );
            }
            None
        }
        CallerPrincipal::Device(ref dev) => Some(dev.subject.id),
    };

    let tenant_id = caller.tenant_id();

    let device_id = match DeviceId::from_str(&raw_device_id) {
        Ok(id) => id,
        Err(_) => {
            let problem = Problem::new(ErrorCode::NotFound, &request_id)
                .with_detail(format!("device '{raw_device_id}' not found"));
            return with_standard_headers(
                (StatusCode::NOT_FOUND, ProblemResponse::new(problem)).into_response(),
                &request_id,
            );
        }
    };

    match get_device(
        &state.pool,
        tenant_id,
        device_id,
        caller_subject_id_if_device,
    )
    .await
    {
        Ok(dev) => with_standard_headers(Json(dev).into_response(), &request_id),
        Err(e) => subject_error_to_response(e, &request_id),
    }
}

/// `DELETE /v1/devices/{device_id}` (OpenAPI `deactivateDevice`).
pub async fn deactivate_device_handler(
    State(state): State<SessionApiState>,
    Path(raw_device_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };

    let caller_device_id_if_device = match caller {
        CallerPrincipal::Application(ref app) => {
            if !app.api_key.scopes.iter().any(|s| s == "subjects:manage") {
                let problem = Problem::new(ErrorCode::InsufficientScope, &request_id)
                    .with_detail("application requires subjects:manage scope");
                return with_standard_headers(
                    (StatusCode::FORBIDDEN, ProblemResponse::new(problem)).into_response(),
                    &request_id,
                );
            }
            None
        }
        CallerPrincipal::Device(ref dev) => Some(dev.device.id),
    };

    let tenant_id = caller.tenant_id();

    let device_id = match DeviceId::from_str(&raw_device_id) {
        Ok(id) => id,
        Err(_) => {
            let problem = Problem::new(ErrorCode::NotFound, &request_id)
                .with_detail(format!("device '{raw_device_id}' not found"));
            return with_standard_headers(
                (StatusCode::NOT_FOUND, ProblemResponse::new(problem)).into_response(),
                &request_id,
            );
        }
    };

    let now = OffsetDateTime::now_utc();
    let params = DeactivateDeviceParams {
        pool: &state.pool,
        ephemeral: &*state.ephemeral,
        bus: &*state.bus,
        tenant_id,
        device_id,
        caller_device_id: caller_device_id_if_device,
        key_prefix: &state.redis_key_prefix,
        now,
    };

    match deactivate_device(params).await {
        Ok(()) => with_standard_headers(StatusCode::NO_CONTENT.into_response(), &request_id),
        Err(e) => subject_error_to_response(e, &request_id),
    }
}
