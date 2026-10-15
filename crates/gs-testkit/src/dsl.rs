//! Scenario DSL models and deserialization (docs/18 §6, contracts/README §5).
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// A test scenario defined according to `contracts/scenarios/*.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    /// Unique scenario identifier (e.g. SCN-001).
    pub id: String,
    /// Human-readable title describing the scenario.
    pub title: String,
    /// List of requirement IDs covered by this scenario.
    #[serde(default)]
    pub requirements: Vec<String>,
    /// Whether this scenario is slow/nightly.
    #[serde(default)]
    pub slow: bool,
    /// Configuration overrides for the server.
    #[serde(default)]
    pub config: BTreeMap<String, Value>,
    /// Actor definitions keyed by actor name.
    #[serde(default)]
    pub actors: BTreeMap<String, ActorDef>,
    /// Ordered steps to execute.
    #[serde(default)]
    pub steps: Vec<StepDef>,
}

/// Actor definition within a scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorDef {
    /// Actor kind: "application" or "device".
    pub kind: String,
    /// Requested scopes for the actor.
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
    /// Tenant name (defaults to "default").
    #[serde(default)]
    pub tenant: Option<String>,
    /// External subject id for device actors.
    #[serde(default)]
    pub external_subject_id: Option<String>,
    /// Device platform string (e.g. "PLATFORM_ANDROID").
    #[serde(default)]
    pub platform: Option<String>,
    /// Token TTL in seconds for device actors.
    #[serde(default)]
    pub token_ttl_seconds: Option<u64>,
    /// Optional note or description.
    #[serde(default)]
    pub note: Option<String>,
}

/// A single step in a scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepDef {
    /// The actor executing this step (if applicable).
    #[serde(default)]
    pub actor: Option<String>,
    /// Operation name: "http", "stream.connect", "stream.send", etc.
    pub op: String,
    /// HTTP method (GET, POST, PUT, DELETE, PATCH).
    #[serde(default)]
    pub method: Option<String>,
    /// HTTP request path with template variables.
    #[serde(default)]
    pub path: Option<String>,
    /// HTTP request body.
    #[serde(default)]
    pub body: Option<Value>,
    /// HTTP request headers.
    #[serde(default)]
    pub headers: Option<BTreeMap<String, String>>,
    /// Expected result (status/json object, or string "ready").
    #[serde(default)]
    pub expect: Option<Value>,
    /// Variables to capture from response via JSONPath.
    #[serde(default)]
    pub capture: Option<BTreeMap<String, String>>,
    /// Named mark for timestamp measurement.
    #[serde(default)]
    pub mark: Option<String>,
    /// Connection identifier (default "main").
    #[serde(default)]
    pub connection: Option<String>,
    /// ClientHello override fields.
    #[serde(default)]
    pub hello: Option<Value>,
    /// Expected ServerHello fields.
    #[serde(default)]
    pub expect_hello: Option<Value>,
    /// Stream message body to send or expect.
    #[serde(default)]
    pub message: Option<Value>,
    /// Expected reply message to a sent request.
    #[serde(default)]
    pub expect_reply: Option<Value>,
    /// Expected pattern matching unsolicited message.
    #[serde(rename = "match", default)]
    pub match_: Option<Value>,
    /// Maximum milliseconds to wait for expectation.
    #[serde(default)]
    pub within_ms: Option<u64>,
    /// Milliseconds to wait verifying no matching message arrives.
    #[serde(default)]
    pub for_ms: Option<u64>,
    /// Expected WebSocket close code.
    #[serde(default)]
    pub code: Option<u16>,
    /// Mark name from which to measure latency.
    #[serde(default)]
    pub measure_from: Option<String>,
    /// Metric name to record.
    #[serde(default)]
    pub metric: Option<String>,
    /// Target entity to check in ephemeral store ("snapshot").
    #[serde(default)]
    pub what: Option<String>,
    /// Session ID for ephemeral assertions.
    #[serde(default)]
    pub session: Option<String>,
    /// Member ID for ephemeral assertions.
    #[serde(default)]
    pub member: Option<String>,
    /// Whether target entity exists in ephemeral store.
    #[serde(default)]
    pub exists: Option<bool>,
    /// Sleep duration in milliseconds.
    #[serde(default)]
    pub ms: Option<u64>,
    /// Informational note explaining the step.
    #[serde(default)]
    pub note: Option<String>,
}

/// Parsed HTTP expect definition.
#[derive(Debug, Clone, Default)]
pub struct HttpExpectDef {
    /// Expected HTTP status code.
    pub status: Option<u16>,
    /// Partial JSON body expectation.
    pub json: Option<Value>,
    /// Count expectations per JSONPath expression.
    pub json_count: Option<BTreeMap<String, usize>>,
}

impl StepDef {
    /// Parses the `expect` field as an HTTP expectation if it is a JSON object.
    pub fn parse_http_expect(&self) -> Option<HttpExpectDef> {
        let expect_val = self.expect.as_ref()?;
        let obj = expect_val.as_object()?;
        let status = obj.get("status").and_then(|v| v.as_u64()).map(|s| s as u16);
        let json = obj.get("json").cloned();
        let json_count = obj.get("json_count").and_then(|v| {
            let m = v.as_object()?;
            let mut counts = BTreeMap::new();
            for (k, val) in m {
                if let Some(c) = val.as_u64() {
                    counts.insert(k.clone(), c as usize);
                }
            }
            Some(counts)
        });
        Some(HttpExpectDef {
            status,
            json,
            json_count,
        })
    }
}
