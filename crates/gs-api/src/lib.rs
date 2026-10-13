//! REST router, middlewares and Problem Details (docs/07 §1–§5, FR-OPS-002, FR-API-001).
//!
//! This is the HTTP skeleton of F2: the router with correlation ids, Problem Details with the
//! fixed titles of `docs/07` §5, principal extraction from `Authorization: Bearer`, body
//! limits, redacted access logging and the health endpoints. Credential verification against
//! PostgreSQL and the `/v1` operations arrive with the service layer of F3.
#![forbid(unsafe_code)]

pub mod problem;
pub mod router;
pub mod security;
pub mod tokens;

// Storage access stays behind `gs-service` (docs/04 §4.1); these re-exports exist so the
// binary can reach the repositories through the same facade it already used before the
// router existed.
pub use gs_service::admin;
pub use gs_service::migrate;

pub use problem::{
    DETAIL_MAX_LEN, FieldError, PROBLEM_TYPE_PREFIX, Problem, ProblemResponse, TitleCatalog,
    problem_type_of, title_for,
};
pub use router::{
    AdminState, DEFAULT_MAX_BODY_BYTES, DependencyStatus, PrometheusRenderer, READINESS_TIMEOUT,
    REQUEST_ID_HEADER, ReadinessProbe, ReadinessStatus, admin_ready_handler, admin_router,
    health_live_handler, health_ready_handler, http_router, http_router_with_readiness,
    metrics_handler, redact_header_value, request_id_header,
};
pub use security::{
    CredentialKind, Principal, PrincipalError, extract_principal, principal_from_header,
    request_id_from,
};
pub use tokens::{SigningKeyRef, TokenState, mint_token_handler, v1_router};
