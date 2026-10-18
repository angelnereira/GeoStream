//! `POST /v1/tools/eta` and `POST /v1/tools/assignments` (docs/23 §4–§5,
//! OpenAPI `estimateEta` / `assignBatch`).
//!
//! Both require the `tools:use` scope and the tenant policy `tools_allowed`;
//! without the policy the answer is `403 FEATURE_DISABLED` (`22`/`23`). With
//! `source_preference = PROVIDER` and no provider configured the answer is
//! `503 PROVIDER_UNAVAILABLE`: **never** a silent fallback to our own estimate.
#![forbid(unsafe_code)]

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use gs_service::gs_domain::error::ErrorCode;
use gs_service::gs_store::repos::TenantRepository;
use serde::{Deserialize, Serialize};

use crate::sessions::{CallerPrincipal, SessionApiState, authenticate_caller};
use crate::{Problem, ProblemResponse, request_id_from};

/// Default matrix limit when the configuration does not say otherwise (`17` §4).
pub const DEFAULT_ETA_MAX_MATRIX_CELLS: u64 = 625;
/// Default assignment size limit (`17` §4).
pub const DEFAULT_ASSIGNMENT_MAX_SIZE: u64 = 500;

/// Tools endpoint configuration.
#[derive(Debug, Clone, Copy)]
pub struct ToolsLimits {
    /// `eta_max_matrix_cells`.
    pub eta_max_matrix_cells: u64,
    /// `assignment_max_size`.
    pub assignment_max_size: u64,
}

impl Default for ToolsLimits {
    fn default() -> Self {
        Self {
            eta_max_matrix_cells: DEFAULT_ETA_MAX_MATRIX_CELLS,
            assignment_max_size: DEFAULT_ASSIGNMENT_MAX_SIZE,
        }
    }
}

/// State for the tools endpoints: the sessions state plus the limits.
#[derive(Clone)]
pub struct ToolsState {
    /// Shared authentication/rate-limit state.
    pub sessions: SessionApiState,
    /// Limits from `17` §4.
    pub limits: ToolsLimits,
}

/// Mounts `/v1/tools/*`.
pub fn tools_router(state: ToolsState) -> axum::Router {
    axum::Router::new()
        .route("/v1/tools/eta", axum::routing::post(eta_handler))
        .route(
            "/v1/tools/assignments",
            axum::routing::post(assignments_handler),
        )
        .with_state(state)
}

// --- Contract types (mirror the OpenAPI schemas) --------------------------

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TravelMode {
    Driving,
    Motorcycle,
    Cycling,
    Walking,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SourcePreference {
    Provider,
    Learned,
    Estimated,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub struct Point {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EtaRequest {
    pub origins: Vec<Point>,
    pub destinations: Vec<Point>,
    pub mode: TravelMode,
    #[serde(default)]
    pub source_preference: Option<SourcePreference>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EtaResponse {
    /// Seconds; `null` means "no known route" and is never rendered as 0.
    pub matrix: Vec<Vec<Option<f64>>>,
    pub source: SourcePreference,
    pub mode: TravelMode,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Worker {
    pub id: String,
    pub position: Point,
    #[serde(default = "one")]
    pub capacity: u32,
    #[serde(default)]
    pub cost_per_unit: f64,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub id: String,
    pub position: Point,
    #[serde(default = "one")]
    pub demand: u32,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentAlgorithm {
    #[default]
    Hungarian,
    MinCostFlow,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssignmentRequest {
    pub workers: Vec<Worker>,
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub algorithm: AssignmentAlgorithm,
    #[serde(default)]
    pub max_cost: Option<f64>,
    #[serde(default)]
    pub source_preference: Option<SourcePreference>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Assignment {
    pub task_id: String,
    pub worker_id: String,
    pub cost: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssignmentResponse {
    pub assignments: Vec<Assignment>,
    pub unassigned: Vec<String>,
}

// --- Handlers --------------------------------------------------------------

async fn eta_handler(
    State(state): State<ToolsState>,
    headers: HeaderMap,
    body: Result<Json<EtaRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state.sessions, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };
    if let Some(resp) = require_tools_use(&caller, &request_id) {
        return resp;
    }
    if let Some(resp) = require_tools_allowed(&state, caller.tenant_id(), &request_id).await {
        return resp;
    }

    let request = match body {
        Ok(Json(request)) => request,
        Err(_) => {
            return problem(
                ErrorCode::ValidationFailed,
                &request_id,
                "request body must be a JSON EtaRequest",
            );
        }
    };

    let source = request
        .source_preference
        .unwrap_or(SourcePreference::Estimated);
    // `PROVIDER` is explicit: with no gateway configured we refuse rather than
    // degrade silently (docs/23 §4).
    if source == SourcePreference::Provider {
        return problem(
            ErrorCode::ProviderUnavailable,
            &request_id,
            "no provider configured for the requested port",
        );
    }

    let cells = (request.origins.len() as u64) * (request.destinations.len() as u64);
    if cells == 0 || cells > state.limits.eta_max_matrix_cells {
        return problem(
            ErrorCode::ValidationFailed,
            &request_id,
            "origins*destinations exceeds eta_max_matrix_cells",
        );
    }

    let mode = request.mode;
    let eta_mode = to_mode(mode);
    let mut matrix = Vec::with_capacity(request.origins.len());
    for o in &request.origins {
        let mut row = Vec::with_capacity(request.destinations.len());
        for d in &request.destinations {
            // `LEARNED` without a learned route is `None`; `ESTIMATED` always
            // has an answer.
            let known = source == SourcePreference::Estimated;
            row.push(gs_mobility::estimate_eta(
                (o.lat, o.lon),
                (d.lat, d.lon),
                eta_mode,
                gs_mobility::ETA_DEFAULT_CIRCUITY,
                known,
            ));
        }
        matrix.push(row);
    }

    json_ok(
        &request_id,
        EtaResponse {
            matrix,
            source,
            mode,
        },
    )
}

async fn assignments_handler(
    State(state): State<ToolsState>,
    headers: HeaderMap,
    body: Result<Json<AssignmentRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request_id = request_id_from(&headers);
    let caller = match authenticate_caller(&state.sessions, &headers, &request_id).await {
        Ok(c) => c,
        Err(resp) => return *resp,
    };
    if let Some(resp) = require_tools_use(&caller, &request_id) {
        return resp;
    }
    if let Some(resp) = require_tools_allowed(&state, caller.tenant_id(), &request_id).await {
        return resp;
    }

    let request = match body {
        Ok(Json(request)) => request,
        Err(_) => {
            return problem(
                ErrorCode::ValidationFailed,
                &request_id,
                "request body must be a JSON AssignmentRequest",
            );
        }
    };

    let size = (request.workers.len().max(request.tasks.len())) as u64;
    if size == 0 || size > state.limits.assignment_max_size {
        return problem(
            ErrorCode::ValidationFailed,
            &request_id,
            "assignment size exceeds assignment_max_size",
        );
    }

    // Hungarian requires capacity = 1 for every worker.
    if request.algorithm == AssignmentAlgorithm::Hungarian
        && request.workers.iter().any(|w| w.capacity != 1)
    {
        return problem(
            ErrorCode::ValidationFailed,
            &request_id,
            "hungarian requires capacity = 1 on every worker",
        );
    }
    if request
        .workers
        .iter()
        .any(|w| w.id.is_empty() || w.id.len() > 64)
        || request
            .tasks
            .iter()
            .any(|t| t.id.is_empty() || t.id.len() > 64)
    {
        return problem(
            ErrorCode::ValidationFailed,
            &request_id,
            "worker/task ids must be 1..=64 characters",
        );
    }

    // Cost matrix: distance in metres (own estimate). `min_cost_flow` with
    // capacity is approximated by Hungarian on the expanded problem only when
    // every capacity is 1; otherwise we fall back to a greedy capacity-aware
    // assignment (the full MCF solver is T-E5-02's dedicated work).
    let costs: Vec<Vec<f64>> = request
        .workers
        .iter()
        .map(|w| {
            request
                .tasks
                .iter()
                .map(|t| {
                    gs_geo::haversine_distance(
                        gs_geo::LatLon::new(w.position.lat, w.position.lon),
                        gs_geo::LatLon::new(t.position.lat, t.position.lon),
                    )
                })
                .collect()
        })
        .collect();

    let (assignments, unassigned) = if request.algorithm == AssignmentAlgorithm::Hungarian {
        let (_, mapping) = gs_mobility::assign(&costs, 1);
        build_result(&request, &mapping, &costs, request.max_cost)
    } else {
        // `min_cost_flow`: worker capacities **and task demands** are the point
        // of the model, so the solver is a real min-cost flow. A task with
        // `demand > 1` is satisfied by up to that many workers.
        let capacities: Vec<u32> = request.workers.iter().map(|w| w.capacity).collect();
        let demands: Vec<u32> = request.tasks.iter().map(|t| t.demand).collect();
        let (_, mapping) = gs_mobility::assign_min_cost_flow_demand(&costs, &capacities, &demands);
        build_multi_result(&request, &mapping, &costs, request.max_cost)
    };

    json_ok(
        &request_id,
        AssignmentResponse {
            assignments,
            unassigned,
        },
    )
}

/// Builds the flat assignment list for the demand-aware solver: one entry per
/// (task, worker) pair, so a task with `demand = 2` produces two entries.
fn build_multi_result(
    request: &AssignmentRequest,
    workers_of_task: &[Vec<usize>],
    costs: &[Vec<f64>],
    max_cost: Option<f64>,
) -> (Vec<Assignment>, Vec<String>) {
    let mut assignments = Vec::new();
    let mut unassigned = Vec::new();
    for (task_idx, workers) in workers_of_task.iter().enumerate() {
        let mut any = false;
        for w in workers {
            let cost = costs[*w][task_idx];
            if let Some(limit) = max_cost
                && cost > limit
            {
                continue;
            }
            assignments.push(Assignment {
                task_id: request.tasks[task_idx].id.clone(),
                worker_id: request.workers[*w].id.clone(),
                cost,
            });
            any = true;
        }
        if !any {
            unassigned.push(request.tasks[task_idx].id.clone());
        }
    }
    (assignments, unassigned)
}

fn build_result(
    request: &AssignmentRequest,
    worker_of_task: &[Option<usize>],
    costs: &[Vec<f64>],
    max_cost: Option<f64>,
) -> (Vec<Assignment>, Vec<String>) {
    let mut assignments = Vec::new();
    let mut assigned_task = vec![false; request.tasks.len()];
    for (task_idx, worker) in worker_of_task.iter().enumerate() {
        if let Some(w) = worker {
            let cost = costs[*w][task_idx];
            if let Some(limit) = max_cost
                && cost > limit
            {
                continue;
            }
            assignments.push(Assignment {
                task_id: request.tasks[task_idx].id.clone(),
                worker_id: request.workers[*w].id.clone(),
                cost,
            });
            assigned_task[task_idx] = true;
        }
    }
    let unassigned = request
        .tasks
        .iter()
        .enumerate()
        .filter(|(i, _)| !assigned_task[*i])
        .map(|(_, t)| t.id.clone())
        .collect();
    (assignments, unassigned)
}

// --- Authorization helpers ------------------------------------------------

fn require_tools_use(caller: &CallerPrincipal, request_id: &str) -> Option<Response> {
    match caller {
        CallerPrincipal::Application(app) => {
            if app.api_key.scopes.iter().any(|s| s == "tools:use") {
                None
            } else {
                Some(problem(
                    ErrorCode::InsufficientScope,
                    request_id,
                    "application requires tools:use scope",
                ))
            }
        }
        CallerPrincipal::Device(dev) => {
            if dev.claims.has_scope("tools:use") {
                None
            } else {
                Some(problem(
                    ErrorCode::InsufficientScope,
                    request_id,
                    "device requires tools:use scope",
                ))
            }
        }
    }
}

async fn require_tools_allowed(
    state: &ToolsState,
    tenant_id: gs_service::gs_domain::ids::TenantId,
    request_id: &str,
) -> Option<Response> {
    let tenant = match TenantRepository::get_by_id(&state.sessions.pool, tenant_id).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return Some(problem(ErrorCode::NotFound, request_id, "tenant not found"));
        }
        Err(_) => {
            return Some(problem(
                ErrorCode::Internal,
                request_id,
                "failed to load tenant policy",
            ));
        }
    };
    match gs_service::gs_auth::TenantPolicy::from_json_value(tenant.policy.clone()) {
        Ok(policy) if policy.tools_allowed => None,
        Ok(_) => Some(problem(
            ErrorCode::FeatureDisabled,
            request_id,
            "tools are disabled for this tenant",
        )),
        Err(_) => Some(problem(
            ErrorCode::Internal,
            request_id,
            "invalid tenant policy",
        )),
    }
}

fn to_mode(mode: TravelMode) -> gs_mobility::EtaMode {
    match mode {
        TravelMode::Driving => gs_mobility::EtaMode::Driving,
        TravelMode::Motorcycle => gs_mobility::EtaMode::Motorcycle,
        TravelMode::Cycling => gs_mobility::EtaMode::Cycling,
        TravelMode::Walking => gs_mobility::EtaMode::Walking,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn worker(id: &str, lat: f64, capacity: u32) -> Worker {
        Worker {
            id: id.to_string(),
            position: Point { lat, lon: 0.0 },
            capacity,
            cost_per_unit: 0.0,
        }
    }

    fn task(id: &str, lat: f64) -> Task {
        Task {
            id: id.to_string(),
            position: Point { lat, lon: 0.0 },
            demand: 1,
        }
    }

    fn request(workers: Vec<Worker>, tasks: Vec<Task>) -> AssignmentRequest {
        AssignmentRequest {
            workers,
            tasks,
            algorithm: AssignmentAlgorithm::Hungarian,
            max_cost: None,
            source_preference: None,
        }
    }

    #[test]
    fn hungarian_assigns_each_task_to_its_nearest_worker() {
        let req = request(
            vec![worker("w1", 8.9824, 1), worker("w2", 8.9900, 1)],
            vec![task("t1", 8.9824), task("t2", 8.9900)],
        );
        let costs: Vec<Vec<f64>> = req
            .workers
            .iter()
            .map(|w| {
                req.tasks
                    .iter()
                    .map(|t| (w.position.lat - t.position.lat).abs() * 111_195.0)
                    .collect()
            })
            .collect();
        let (_, mapping) = gs_mobility::assign(&costs, 1);
        let (assignments, unassigned) = build_result(&req, &mapping, &costs, None);
        assert!(unassigned.is_empty());
        assert_eq!(assignments.len(), 2);
        let w1 = assignments.iter().find(|a| a.worker_id == "w1").unwrap();
        assert_eq!(w1.task_id, "t1");
    }

    #[test]
    fn max_cost_leaves_expensive_pairs_unassigned() {
        let req = request(
            vec![worker("w1", 0.0, 1)],
            vec![task("near", 0.0), task("far", 1.0)],
        );
        let costs: Vec<Vec<f64>> = req
            .workers
            .iter()
            .map(|w| {
                req.tasks
                    .iter()
                    .map(|t| (w.position.lat - t.position.lat).abs() * 111_195.0)
                    .collect()
            })
            .collect();
        let (_, mapping) = gs_mobility::assign(&costs, 1);
        // Only the near task is within 100 m.
        let (assignments, unassigned) = build_result(&req, &mapping, &costs, Some(100.0));
        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].task_id, "near");
        assert_eq!(unassigned, vec!["far".to_string()]);
    }

    #[test]
    fn min_cost_flow_respects_capacity() {
        let mut req = request(
            vec![worker("w1", 0.0, 2)],
            vec![task("t1", 0.0), task("t2", 0.0001), task("t3", 0.0002)],
        );
        req.algorithm = AssignmentAlgorithm::MinCostFlow;
        let costs: Vec<Vec<f64>> = req
            .workers
            .iter()
            .map(|w| {
                req.tasks
                    .iter()
                    .map(|t| (w.position.lat - t.position.lat).abs() * 111_195.0)
                    .collect()
            })
            .collect();
        let capacities: Vec<u32> = req.workers.iter().map(|w| w.capacity).collect();
        let (_, mapping) = gs_mobility::assign_min_cost_flow(&costs, &capacities);
        let (assignments, unassigned) = build_result(&req, &mapping, &costs, None);
        assert_eq!(assignments.len(), 2, "capacity 2 means two tasks");
        assert_eq!(unassigned.len(), 1);
    }

    #[test]
    fn eta_mode_mapping_is_total() {
        for m in [
            TravelMode::Driving,
            TravelMode::Motorcycle,
            TravelMode::Cycling,
            TravelMode::Walking,
        ] {
            assert!(to_mode(m).speed_mps() > 0.0);
        }
    }
}
