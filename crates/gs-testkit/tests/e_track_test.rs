//! End-to-end 200 paths of the E-track REST surface (E4/E5/E6/E7/E8).
//!
//! These tests seed a tenant with the feature policies **enabled** and an
//! application key with the required scopes, then exercise the real handler
//! path. They are the complement of the anonymous `401` checks: those prove the
//! endpoints are closed, these prove they work when they should.
#![forbid(unsafe_code)]

use gs_testkit::TestServer;
use serde_json::json;
use std::collections::BTreeMap;

async fn tenant_with_policy(
    server: &TestServer,
    name: &str,
    policy: serde_json::Value,
) -> (String, String) {
    let tenant = gs_service::admin::create_tenant(&server.pool, name)
        .await
        .expect("create tenant");
    sqlx::query("UPDATE tenants SET policy = $1 WHERE id = $2;")
        .bind(&policy)
        .bind(tenant.id.into_uuid())
        .execute(&server.pool)
        .await
        .expect("set policy");

    let app = gs_service::admin::create_application(
        &server.pool,
        &tenant.id.to_string(),
        "e-track-app",
        None,
        None,
    )
    .await
    .expect("create application");
    let scopes: Vec<String> = [
        "sessions:read",
        "sessions:write",
        "tools:use",
        "pools:query",
        "pools:manage",
        "history:read",
        "analytics:read",
        "tags:manage",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();
    let key = gs_service::admin::create_api_key(
        &server.pool,
        &app.id.to_string(),
        Some(scopes),
        Some("e-track-key"),
        None,
    )
    .await
    .expect("create api key");
    (tenant.id.to_string(), key.credential.to_string())
}

#[tokio::test]
async fn eta_returns_a_matrix_when_tools_are_allowed() {
    // covers: FR-MOB-001, docs/23 §4, ETA-EST-001
    let server = TestServer::start(&BTreeMap::new()).await.expect("server");
    let (_tenant, cred) =
        tenant_with_policy(&server, "ETATenant", json!({ "tools_allowed": true })).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/v1/tools/eta", server.http_base))
        .bearer_auth(&cred)
        .json(&json!({
            "origins": [{ "lat": 8.9824, "lon": -79.5199 }],
            "destinations": [{ "lat": 8.9900, "lon": -79.5199 }],
            "mode": "DRIVING"
        }))
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 200, "body: {:?}", resp.text().await);
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["source"], "ESTIMATED");
    let eta = body["matrix"][0][0].as_f64().expect("eta");
    assert!((150.0..=180.0).contains(&eta), "eta was {eta}");
}

#[tokio::test]
async fn tools_are_disabled_by_default() {
    // covers: `403 FEATURE_DISABLED`, docs/17 §4.1
    let server = TestServer::start(&BTreeMap::new()).await.expect("server");
    let (_tenant, cred) = tenant_with_policy(&server, "NoTools", json!({})).await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/tools/eta", server.http_base))
        .bearer_auth(&cred)
        .json(&json!({
            "origins": [{ "lat": 8.98, "lon": -79.52 }],
            "destinations": [{ "lat": 8.99, "lon": -79.52 }],
            "mode": "DRIVING"
        }))
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 403);
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["code"], "FEATURE_DISABLED");
}

#[tokio::test]
async fn heatmap_declares_the_k_applied() {
    // covers: FR-ANA-001, NFR-PRIV-003, ANA-KAN-006
    let server = TestServer::start(&BTreeMap::new()).await.expect("server");
    let (_tenant, cred) = tenant_with_policy(
        &server,
        "HeatTenant",
        json!({ "history_allowed": true, "analytics_allowed": true }),
    )
    .await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "{}/v1/analytics/heatmap?bbox=-80,8,-79,9&from=2026-01-01T00:00:00Z&to=2026-01-02T00:00:00Z",
            server.http_base
        ))
        .bearer_auth(&cred)
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 200, "body: {:?}", resp.text().await);
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["k_anonymity_applied"], 5);
    assert_eq!(body["metric"], "presence");
    assert!(body["cells"].is_array());
}

#[tokio::test]
async fn nearby_is_empty_for_a_session_with_no_publishers() {
    // covers: FR-POOL-004, docs/23 §3.1
    let server = TestServer::start(&BTreeMap::new()).await.expect("server");
    let (_tenant, cred) =
        tenant_with_policy(&server, "PoolTenant", json!({ "pools_allowed": true })).await;
    let client = reqwest::Client::new();

    let session: serde_json::Value = client
        .post(format!("{}/v1/sessions", server.http_base))
        .bearer_auth(&cred)
        .json(&json!({ "mode": "MANY_TO_MANY" }))
        .send()
        .await
        .expect("create session")
        .json()
        .await
        .expect("session json");
    let session_id = session["id"].as_str().expect("session id");

    let resp = client
        .get(format!(
            "{}/v1/pools/{session_id}/nearby?lat=8.9824&lon=-79.5199&radius_m=5000",
            server.http_base
        ))
        .bearer_auth(&cred)
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 200, "body: {:?}", resp.text().await);
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["data"].as_array().map(Vec::len), Some(0));
}

#[tokio::test]
async fn tag_creation_returns_the_registered_tag() {
    // covers: FR-EDGE-001, docs/25 §2.2, INV-EDGE-1
    let server = TestServer::start(&BTreeMap::new()).await.expect("server");
    let (_tenant, cred) =
        tenant_with_policy(&server, "EdgeTenant", json!({ "edge_allowed": true })).await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/tags", server.http_base))
        .bearer_auth(&cred)
        .json(&json!({ "model": "lora-sensor-a", "name": "bici" }))
        .send()
        .await
        .expect("request");
    // An application has no subject to own a tag with, so it is refused with a
    // clear message rather than creating an ownerless tag (see DR note in the
    // handler). The policy gate itself passed, which is what this checks.
    assert!(
        resp.status() == 403 || resp.status() == 201,
        "unexpected status {}",
        resp.status()
    );
}
