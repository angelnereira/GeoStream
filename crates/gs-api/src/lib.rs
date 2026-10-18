//! REST router, middlewares and Problem Details (docs/07 §1–§5, FR-OPS-002, FR-API-001).
//!
//! This is the HTTP skeleton of F2: the router with correlation ids, Problem Details with the
//! fixed titles of `docs/07` §5, principal extraction from `Authorization: Bearer`, body
//! limits, redacted access logging and the health endpoints. Credential verification against
//! PostgreSQL and the `/v1` operations arrive with the service layer of F3.
#![forbid(unsafe_code)]

pub mod analytics;
pub mod edge;
pub mod history;
pub mod ingest;
pub mod invitations;
pub mod members;
pub mod pagination;
pub mod pools;
pub mod problem;
pub mod rate_limit;
pub mod router;
pub mod security;
pub mod sessions;
pub mod stream;
pub mod stream_tickets;
pub mod subjects;
pub mod tokens;
pub mod tools;

// Storage access stays behind `gs-service` (docs/04 §4.1); these re-exports exist so the
// binary can reach the repositories through the same facade it already used before the
// router existed.
pub use gs_service;
pub use gs_service::admin;
pub use gs_service::migrate;

pub use analytics::{AnalyticsState, analytics_router};
pub use edge::{EdgeState, edge_router};
pub use history::{HistoryState, history_router};
pub use ingest::{IngestState, ingest_router};
pub use invitations::invitations_routes;
pub use members::members_router;
pub use pagination::{PaginationQuery, PaginationQueryError, parse_pagination_query};
pub use pools::{PoolsLimits, PoolsState, pools_router};
pub use problem::{
    DETAIL_MAX_LEN, FieldError, PROBLEM_TYPE_PREFIX, Problem, ProblemResponse, TitleCatalog,
    problem_type_of, title_for,
};
pub use rate_limit::{
    METRIC_RATE_LIMITED_TOTAL, RateLimitConfig, RateLimitExceeded, RateLimitInfo, SCOPE_AUTH_IP,
    SCOPE_INVITE_REDEEM, SCOPE_REST_APP, SCOPE_REST_DEVICE, apply_rate_limit_headers,
    check_rate_limit, extract_client_ip, rate_limit_problem_response,
    record_auth_failure_and_check,
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
pub use sessions::{SessionApiState, StreamTicketConfig, sessions_router};
pub use stream::{
    AcceptedWebSocket, ConnectionCounter, SessionFuture, SessionRunner, StreamApiState,
    TICKET_PARAM, stream_route, stream_upgrade_handler,
};
pub use stream_tickets::{
    StreamTicketResponse, create_stream_ticket_handler, stream_tickets_routes,
};
pub use subjects::subjects_routes;
pub use tokens::{SigningKeyRef, TokenState, mint_token_handler, v1_router};
pub use tools::{
    AssignmentRequest, AssignmentResponse, EtaRequest, EtaResponse, ToolsLimits, ToolsState,
    tools_router,
};
