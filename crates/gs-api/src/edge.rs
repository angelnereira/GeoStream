//! Tags endpoints (`docs/25` §2.2, OpenAPI `createTag`/`listTags`/`getTag`, E8).
//!
//! Scope `tags:manage`, tenant policy `edge_allowed`. The request and the
//! response **never** carry the `EIK`: the installation key lives in the tag and
//! in the owner device, and the server neither receives nor stores it
//! (INV-EDGE-1).
#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use gs_service::gs_domain::error::ErrorCode;
use gs_service::gs_domain::ids::{SubjectId, TagId, TenantId};
use gs_service::gs_store::repos::{Tag, TagRepository, TenantRepository};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::sessions::{CallerPrincipal, SessionApiState, authenticate_caller};
use crate::{Problem, ProblemResponse, request_id_from};

/// State for the edge/tags endpoints.
#[derive(Clone)]
pub struct EdgeState {
    /// Shared authentication/rate-limit state.
    pub sessions: SessionApiState,
}

/// Mounts `/v1/tags`.
pub fn edge_router(state: EdgeState) -> axum::Router {
    axum::Router::new()
        .route("/v1/tags", axum::routing::post(create_tag).get(list_tags))
        .route(
            "/v1/tags/{tag_id}",
            axum::routing::get(get_tag).delete(delete_tag),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateTagRequest {
    pub model: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListTagsQuery {
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct TagDto {
    pub id: String,
    pub model: String,
    pub name: Option<String>,
    pub state: String,
    pub separated_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct TagList {
    pub data: Vec<TagDto>,
    pub next_cursor: Option<String>,
}

impl From<Tag> for TagDto {
    fn from(t: Tag) -> Self {
        let fmt = |i: OffsetDateTime| i.format(&Rfc3339).unwrap_or_default();
        Self {
            id: t.id.to_string(),
            model: t.model,
            name: t.name,
            state: t.state,
            separated_at: t.separated_at.map(fmt),
            created_at: fmt(t.created_at),
            updated_at: fmt(t.updated_at),
        }
    }
}

/// The authenticated caller of a tag operation.
enum TagCaller {
    /// A device owns the tags it registers.
    Device(TenantId, SubjectId),
    /// An application administers every tag of the tenant.
    Application(TenantId),
}

impl TagCaller {
    fn tenant_id(&self) -> TenantId {
        match self {
            Self::Device(t, _) | Self::Application(t) => *t,
        }
    }
}

/// Authenticates a `tags:manage` caller and enforces `edge_allowed`.
async fn authorize(
    state: &EdgeState,
    headers: &HeaderMap,
    request_id: &str,
) -> Result<TagCaller, Response> {
    let caller = match authenticate_caller(&state.sessions, headers, request_id).await {
        Ok(c) => c,
        Err(resp) => return Err(*resp),
    };
    let caller = match &caller {
        CallerPrincipal::Application(app) => {
            if !app.api_key.scopes.iter().any(|s| s == "tags:manage") {
                return Err(problem(
                    ErrorCode::InsufficientScope,
                    request_id,
                    "application requires tags:manage scope",
                ));
            }
            TagCaller::Application(app.tenant.id)
        }
        CallerPrincipal::Device(dev) => {
            if !dev.claims.has_scope("tags:manage") {
                return Err(problem(
                    ErrorCode::InsufficientScope,
                    request_id,
                    "device requires tags:manage scope",
                ));
            }
            TagCaller::Device(dev.tenant.id, dev.subject.id)
        }
    };
    let tenant_id = caller.tenant_id();
    let tenant = match TenantRepository::get_by_id(&state.sessions.pool, tenant_id).await {
        Ok(Some(t)) => t,
        Ok(None) => return Err(problem(ErrorCode::NotFound, request_id, "tenant not found")),
        Err(_) => {
            return Err(problem(
                ErrorCode::Internal,
                request_id,
                "failed to load tenant",
            ));
        }
    };
    let policy = gs_service::gs_auth::TenantPolicy::from_json_value(tenant.policy.clone())
        .unwrap_or_default();
    if !policy.edge_allowed {
        return Err(problem(
            ErrorCode::FeatureDisabled,
            request_id,
            "edge is disabled for this tenant",
        ));
    }
    Ok(caller)
}

async fn create_tag(
    State(state): State<EdgeState>,
    headers: HeaderMap,
    body: Result<Json<CreateTagRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authorize(&state, &headers, &request_id).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    // Tag ownership requires a subject. An application has none in the contract
    // (`CreateTagRequest` has no owner field), so it registers tags through the
    // subject's device. This is documented as a pending contract clarification.
    let (tenant_id, owner) = match caller {
        TagCaller::Device(t, s) => (t, s),
        TagCaller::Application(_) => {
            return problem(
                ErrorCode::PermissionDenied,
                &request_id,
                "tag ownership requires a subject; register tags from the owner device",
            );
        }
    };
    let body = match body {
        Ok(Json(b)) => b,
        Err(_) => {
            return problem(
                ErrorCode::ValidationFailed,
                &request_id,
                "body must be a CreateTagRequest",
            );
        }
    };
    if body.model.is_empty() || body.model.len() > 100 {
        return problem(
            ErrorCode::ValidationFailed,
            &request_id,
            "model out of range",
        );
    }
    if let Some(name) = &body.name
        && (name.is_empty() || name.len() > 100)
    {
        return problem(
            ErrorCode::ValidationFailed,
            &request_id,
            "name out of range",
        );
    }

    let now = OffsetDateTime::now_utc();
    let tag = Tag {
        id: TagId::new(),
        tenant_id,
        owner_subject_id: owner,
        model: body.model,
        name: body.name,
        state: "ACTIVE".to_string(),
        separated_at: None,
        created_at: now,
        updated_at: now,
    };
    if TagRepository::create(&state.sessions.pool, &tag)
        .await
        .is_err()
    {
        return problem(ErrorCode::Internal, &request_id, "failed to create tag");
    }
    let mut response = (StatusCode::CREATED, Json(TagDto::from(tag))).into_response();
    if let Ok(value) = axum::http::HeaderValue::from_str(&request_id) {
        response
            .headers_mut()
            .insert(crate::REQUEST_ID_HEADER, value);
    }
    response
}

async fn list_tags(
    State(state): State<EdgeState>,
    Query(query): Query<ListTagsQuery>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authorize(&state, &headers, &request_id).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if let Some(state) = &query.state
        && !matches!(state.as_str(), "ACTIVE" | "SEPARATED" | "REMOVED")
    {
        return problem(ErrorCode::ValidationFailed, &request_id, "invalid state");
    }
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let listed = match caller {
        TagCaller::Device(tenant_id, owner) => {
            TagRepository::list_by_owner(
                &state.sessions.pool,
                tenant_id,
                owner,
                query.state.as_deref(),
                limit,
            )
            .await
        }
        TagCaller::Application(tenant_id) => {
            TagRepository::list_by_tenant(
                &state.sessions.pool,
                tenant_id,
                query.state.as_deref(),
                limit,
            )
            .await
        }
    };
    let tags = match listed {
        Ok(t) => t,
        Err(_) => return problem(ErrorCode::Internal, &request_id, "failed to list tags"),
    };
    json_ok(
        &request_id,
        TagList {
            data: tags.into_iter().map(TagDto::from).collect(),
            next_cursor: None,
        },
    )
}

async fn get_tag(
    State(state): State<EdgeState>,
    Path(tag_id_raw): Path<String>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authorize(&state, &headers, &request_id).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let tenant_id = caller.tenant_id();
    let tag_id = match TagId::from_str(&tag_id_raw) {
        Ok(id) => id,
        Err(_) => return problem(ErrorCode::ValidationFailed, &request_id, "malformed tag id"),
    };
    match TagRepository::get(&state.sessions.pool, tenant_id, tag_id).await {
        Ok(Some(tag)) => json_ok(&request_id, TagDto::from(tag)),
        Ok(None) => problem(ErrorCode::NotFound, &request_id, "tag not found"),
        Err(_) => problem(ErrorCode::Internal, &request_id, "failed to load tag"),
    }
}

/// `DELETE /v1/tags/{tag_id}`: idempotent soft delete (`state = REMOVED`).
async fn delete_tag(
    State(state): State<EdgeState>,
    Path(tag_id_raw): Path<String>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authorize(&state, &headers, &request_id).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let tenant_id = caller.tenant_id();
    let tag_id = match TagId::from_str(&tag_id_raw) {
        Ok(id) => id,
        Err(_) => return problem(ErrorCode::ValidationFailed, &request_id, "malformed tag id"),
    };
    // A device may only remove its own tags.
    if let TagCaller::Device(_, owner) = caller {
        match TagRepository::get(&state.sessions.pool, tenant_id, tag_id).await {
            Ok(Some(tag)) if tag.owner_subject_id == owner => {}
            Ok(Some(_)) => {
                return problem(
                    ErrorCode::PermissionDenied,
                    &request_id,
                    "a device may only remove its own tags",
                );
            }
            Ok(None) => return problem(ErrorCode::NotFound, &request_id, "tag not found"),
            Err(_) => return problem(ErrorCode::Internal, &request_id, "failed to load tag"),
        }
    }
    let now = OffsetDateTime::now_utc();
    match TagRepository::set_state(
        &state.sessions.pool,
        tenant_id,
        tag_id,
        "REMOVED",
        None,
        now,
    )
    .await
    {
        Ok(_) => {}
        Err(_) => return problem(ErrorCode::Internal, &request_id, "failed to remove tag"),
    }
    // Idempotent: 204 whether or not it changed.
    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Ok(value) = axum::http::HeaderValue::from_str(&request_id) {
        response
            .headers_mut()
            .insert(crate::REQUEST_ID_HEADER, value);
    }
    response
}

fn problem(code: ErrorCode, request_id: &str, detail: &str) -> Response {
    let problem = Problem::new(code, request_id).with_detail(detail);
    ProblemResponse::new(problem).into_response()
}

fn json_ok<T: Serialize>(request_id: &str, body: T) -> Response {
    let mut response = (StatusCode::OK, Json(body)).into_response();
    if let Ok(value) = axum::http::HeaderValue::from_str(request_id) {
        response
            .headers_mut()
            .insert(crate::REQUEST_ID_HEADER, value);
    }
    response
}
