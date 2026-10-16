//! Scenario runner implementing the full DSL (docs/18 §6, contracts/README §5).
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::time::Duration;

use gs_domain::{Subprotocol, TenantId};
use gs_ephemeral::EphemeralStore;
use gs_protocol::ClientHello;
use gs_protocol::client_message::Body as ClientBody;
use gs_protocol::server_message::Body as ServerBody;
use gs_service::admin::{create_api_key, create_application, create_tenant};
use serde_json::Value;

use crate::client::TestStreamClient;
use crate::dsl::Scenario;
use crate::json_path::{count_path, extract_path};
use crate::matcher::{normalize_json_keys, partial_match};
use crate::server::TestServer;
use crate::template::TemplateContext;

/// Result report from a scenario execution.
#[derive(Debug, Clone)]
pub struct ScenarioReport {
    /// Scenario ID.
    pub id: String,
    /// Metrics measured during the scenario (e.g. `revocation_propagation_ms`).
    pub metrics: HashMap<String, u64>,
}

/// Executes a Scenario using the given subprotocol.
pub async fn run_scenario(
    scenario: &Scenario,
    subprotocol: Subprotocol,
) -> Result<ScenarioReport, String> {
    let server = TestServer::start(&scenario.config).await?;
    let mut ctx = TemplateContext::default();
    let mut actor_installations: HashMap<String, String> = HashMap::new();
    let mut tenant_map: HashMap<String, (TenantId, String)> = HashMap::new();

    // 1. Provision tenants and actors
    for (actor_name, actor_def) in &scenario.actors {
        let tenant_name = actor_def.tenant.as_deref().unwrap_or("default");

        let (tenant_id, bootstrap_cred) = match tenant_map.get(tenant_name) {
            Some((id, cred)) => (*id, cred.clone()),
            None => {
                let suffix = uuid::Uuid::new_v4().to_string();
                let tenant = create_tenant(&server.pool, &format!("Tenant {tenant_name} {suffix}"))
                    .await
                    .map_err(|e| format!("create tenant error: {e}"))?;

                let boot_app = create_application(
                    &server.pool,
                    &tenant.id.to_string(),
                    &format!("boot-{tenant_name}-{suffix}"),
                    None,
                    None,
                )
                .await
                .map_err(|e| format!("create boot app error: {e}"))?;

                let boot_key = create_api_key(
                    &server.pool,
                    &boot_app.id.to_string(),
                    Some(
                        gs_auth::scopes::APPLICATION_SCOPES
                            .iter()
                            .map(|s| s.to_string())
                            .collect(),
                    ),
                    Some("bootstrap"),
                    None,
                )
                .await
                .map_err(|e| format!("create boot key error: {e}"))?;

                let cred = boot_key.credential.to_string();
                tenant_map.insert(tenant_name.to_string(), (tenant.id, cred.clone()));
                (tenant.id, cred)
            }
        };

        if actor_def.kind == "application" {
            let suffix = uuid::Uuid::new_v4().to_string();
            let app_scopes = actor_def.scopes.clone().unwrap_or_else(|| {
                gs_auth::scopes::APPLICATION_SCOPES
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            });

            let app = create_application(
                &server.pool,
                &tenant_id.to_string(),
                &format!("app-{actor_name}-{suffix}"),
                None,
                None,
            )
            .await
            .map_err(|e| format!("create app for {actor_name}: {e}"))?;

            let api_key = create_api_key(
                &server.pool,
                &app.id.to_string(),
                Some(app_scopes),
                Some(actor_name),
                None,
            )
            .await
            .map_err(|e| format!("create api key for {actor_name}: {e}"))?;

            ctx.actor_tokens
                .insert(actor_name.clone(), api_key.credential.to_string());
        } else if actor_def.kind == "device" {
            let installation_id = uuid::Uuid::new_v4().to_string();
            actor_installations.insert(actor_name.clone(), installation_id.clone());

            let external_sub = actor_def
                .external_subject_id
                .as_deref()
                .unwrap_or(actor_name.as_str());
            let raw_platform = actor_def.platform.as_deref().unwrap_or("ANDROID");
            let platform = raw_platform
                .strip_prefix("PLATFORM_")
                .unwrap_or(raw_platform);
            let scopes = actor_def.scopes.clone();

            let token = server
                .mint_token(
                    &bootstrap_cred,
                    external_sub,
                    &installation_id,
                    platform,
                    scopes,
                    actor_def.token_ttl_seconds,
                )
                .await?;

            ctx.actor_tokens.insert(actor_name.clone(), token);
        }
    }

    let http_client = reqwest::Client::new();
    let mut connections: HashMap<(String, String), TestStreamClient> = HashMap::new();
    let mut marks: HashMap<String, tokio::time::Instant> = HashMap::new();
    let mut metrics: HashMap<String, u64> = HashMap::new();

    // 2. Step execution loop
    for (step_idx, step) in scenario.steps.iter().enumerate() {
        tracing::debug!(
            "SCENARIO STEP {step_idx}: {} actor={:?} vars={:?}",
            step.op,
            step.actor,
            ctx.variables
        );
        match step.op.as_str() {
            "http" => {
                let actor_name = step
                    .actor
                    .as_deref()
                    .ok_or_else(|| format!("Step {step_idx}: actor required for http"))?;
                let token = ctx
                    .actor_tokens
                    .get(actor_name)
                    .ok_or_else(|| format!("Unknown actor {actor_name} at step {step_idx}"))?;

                let raw_path = step
                    .path
                    .as_deref()
                    .ok_or_else(|| format!("Step {step_idx}: path required for http"))?;
                let path = ctx.resolve_str(raw_path);
                let url = format!("{}{path}", server.http_base);

                let method_str = step.method.as_deref().unwrap_or("GET");
                let method = match method_str {
                    "GET" => reqwest::Method::GET,
                    "POST" => reqwest::Method::POST,
                    "PUT" => reqwest::Method::PUT,
                    "DELETE" => reqwest::Method::DELETE,
                    "PATCH" => reqwest::Method::PATCH,
                    other => return Err(format!("Unsupported HTTP method {other}")),
                };

                if let Some(mark_name) = &step.mark {
                    marks.insert(mark_name.clone(), tokio::time::Instant::now());
                }

                let mut req = http_client
                    .request(method, &url)
                    .header("Authorization", format!("Bearer {token}"));

                if let Some(headers) = &step.headers {
                    for (k, v) in headers {
                        req = req.header(k, ctx.resolve_str(v));
                    }
                }

                if let Some(body) = &step.body {
                    let resolved_body = ctx.resolve_val(body);
                    req = req.json(&resolved_body);
                }

                let resp = req
                    .send()
                    .await
                    .map_err(|e| format!("HTTP send error at step {step_idx} ({url}): {e}"))?;

                let status = resp.status().as_u16();

                if let Some(expect) = step.parse_http_expect() {
                    if let Some(exp_status) = expect.status
                        && status != exp_status
                    {
                        let text = resp.text().await.unwrap_or_default();
                        return Err(format!(
                            "HTTP status mismatch at step {step_idx} ({method_str} {path}): expected {exp_status}, got {status}. Body: {text}"
                        ));
                    }

                    if expect.json.is_some()
                        || expect.json_count.is_some()
                        || step.capture.is_some()
                    {
                        let text = resp
                            .text()
                            .await
                            .map_err(|e| format!("Failed reading response body: {e}"))?;
                        let resp_json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);

                        if let Some(exp_json) = expect.json {
                            let resolved_exp = ctx.resolve_val(&exp_json);
                            if !partial_match(&resolved_exp, &resp_json) {
                                return Err(format!(
                                    "HTTP json mismatch at step {step_idx} ({method_str} {path}): expected {:?}, got {:?}",
                                    resolved_exp, resp_json
                                ));
                            }
                        }

                        if let Some(counts) = expect.json_count {
                            for (json_path, exp_count) in counts {
                                let actual_count = count_path(&resp_json, &json_path).unwrap_or(0);
                                if actual_count != exp_count {
                                    return Err(format!(
                                        "json_count mismatch at step {step_idx} for {json_path}: expected {exp_count}, got {actual_count}. Body: {:?}",
                                        resp_json
                                    ));
                                }
                            }
                        }

                        if let Some(captures) = &step.capture {
                            for (var_name, json_path) in captures {
                                let extracted = extract_path(&resp_json, json_path).ok_or_else(|| {
                                    format!(
                                        "Capture failed at step {step_idx}: path {json_path} not found in {:?}",
                                        resp_json
                                    )
                                })?;
                                let str_val = match extracted {
                                    Value::String(s) => s.clone(),
                                    _ => extracted.to_string().trim_matches('"').to_string(),
                                };
                                ctx.variables.insert(var_name.clone(), str_val);
                            }
                        }
                    }
                } else if let Some(captures) = &step.capture {
                    let text = resp
                        .text()
                        .await
                        .map_err(|e| format!("Failed reading response body: {e}"))?;
                    let resp_json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                    for (var_name, json_path) in captures {
                        let extracted = extract_path(&resp_json, json_path).ok_or_else(|| {
                            format!(
                                "Capture failed at step {step_idx}: path {json_path} not found in {:?}",
                                resp_json
                            )
                        })?;
                        let str_val = match extracted {
                            Value::String(s) => s.clone(),
                            _ => extracted.to_string().trim_matches('"').to_string(),
                        };
                        ctx.variables.insert(var_name.clone(), str_val);
                    }
                }
            }

            "stream.connect" => {
                let actor_name = step
                    .actor
                    .as_deref()
                    .ok_or_else(|| format!("Step {step_idx}: actor required for stream.connect"))?;
                let conn_name = step.connection.as_deref().unwrap_or("main");
                let token = ctx
                    .actor_tokens
                    .get(actor_name)
                    .ok_or_else(|| format!("Unknown actor {actor_name} at step {step_idx}"))?;

                // 1. Acquire stream ticket
                let ticket_resp = http_client
                    .post(format!("{}/v1/stream/tickets", server.http_base))
                    .header("Authorization", format!("Bearer {token}"))
                    .send()
                    .await
                    .map_err(|e| format!("Ticket request failed at step {step_idx}: {e}"))?;

                if ticket_resp.status() != 201 {
                    let text = ticket_resp.text().await.unwrap_or_default();
                    return Err(format!(
                        "Stream ticket creation failed at step {step_idx}: {text}"
                    ));
                }

                let ticket_val: Value = ticket_resp
                    .json()
                    .await
                    .map_err(|e| format!("Ticket parse failed: {e}"))?;
                let ticket = ticket_val
                    .get("ticket")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "Ticket missing in ticket response".to_string())?;

                // 2. Connect WebSocket
                let ws_url = format!("{}/v1/stream?ticket={ticket}", server.ws_base);
                let client = TestStreamClient::connect(&ws_url, subprotocol)
                    .await
                    .map_err(|e| format!("Stream connect failed at step {step_idx}: {e}"))?;

                // 3. Perform Hello handshake
                let mut last_seq = 0u64;
                if let Some(hello_override) = &step.hello
                    && let Some(ls) = hello_override.get("last_sequence")
                {
                    if let Some(n) = ls.as_u64() {
                        last_seq = n;
                    } else if let Some(s) = ls.as_str() {
                        last_seq = s.parse().unwrap_or(0);
                    }
                }

                let hello = ClientHello {
                    protocol_minor: 0,
                    sdk_name: "geostream-testkit".to_string(),
                    sdk_version: "0.1.0".to_string(),
                    platform: 1, // PLATFORM_ANDROID
                    app_version: "1.0.0".to_string(),
                    last_sequence: last_seq,
                    client_time: None,
                    core_version: String::new(),
                };

                let hello_id = client.next_id().await;
                let reply = client
                    .send_with_reply(hello_id, ClientBody::Hello(hello), Duration::from_secs(2))
                    .await
                    .map_err(|e| format!("Hello reply error at step {step_idx}: {e}"))?;

                if let Some(ServerBody::Hello(hello_ack)) = &reply.body {
                    if let Some(st) = &hello_ack.server_time {
                        let server_ms = st.seconds * 1000 + (st.nanos / 1_000_000) as i64;
                        let local_ms =
                            time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000;
                        ctx.clock_offset_ms = server_ms - local_ms as i64;
                    }
                } else {
                    return Err(format!(
                        "Expected Hello reply at step {step_idx}, got {:?}",
                        reply.body
                    ));
                }

                if let Some(exp_hello) = &step.expect_hello {
                    let resolved_exp = ctx.resolve_val(exp_hello);
                    let norm = normalize_json_keys(
                        &serde_json::to_value(&reply).map_err(|e| e.to_string())?,
                    );
                    let hello_json = norm.get("hello").unwrap_or(&norm);
                    if !partial_match(&resolved_exp, hello_json) {
                        return Err(format!(
                            "expect_hello mismatch at step {step_idx}: expected {:?}, got {:?}",
                            resolved_exp, hello_json
                        ));
                    }
                }

                connections.insert((actor_name.to_string(), conn_name.to_string()), client);
            }

            "stream.send" => {
                let actor_name = step
                    .actor
                    .as_deref()
                    .ok_or_else(|| format!("Step {step_idx}: actor required for stream.send"))?;
                let conn_name = step.connection.as_deref().unwrap_or("main");
                let client = connections
                    .get(&(actor_name.to_string(), conn_name.to_string()))
                    .ok_or_else(|| {
                        format!("No active stream connection for ({actor_name}, {conn_name}) at step {step_idx}")
                    })?;

                let raw_msg = step
                    .message
                    .as_ref()
                    .ok_or_else(|| format!("Step {step_idx}: message required for stream.send"))?;
                let resolved_msg = ctx.resolve_val(raw_msg);

                let mut client_msg: gs_protocol::ClientMessage =
                    serde_json::from_value(resolved_msg.clone()).map_err(|e| {
                        format!(
                            "Failed parsing ClientMessage from {:?} at step {step_idx}: {e}",
                            resolved_msg
                        )
                    })?;

                let req_id = client.next_id().await;
                client_msg.request_id = req_id;

                let body = client_msg.body.ok_or_else(|| {
                    format!(
                        "ClientMessage body missing in {:?} at step {step_idx}",
                        resolved_msg
                    )
                })?;

                if let Some(exp_reply) = &step.expect_reply {
                    let reply = client
                        .send_with_reply(req_id, body, Duration::from_millis(2000))
                        .await
                        .map_err(|e| format!("Send error at step {step_idx}: {e}"))?;

                    let resolved_exp = ctx.resolve_val(exp_reply);
                    let norm = normalize_json_keys(
                        &serde_json::to_value(&reply).map_err(|e| e.to_string())?,
                    );

                    if !partial_match(&resolved_exp, &norm) {
                        return Err(format!(
                            "expect_reply mismatch at step {step_idx}: expected {:?}, got {:?}",
                            resolved_exp, norm
                        ));
                    }
                } else {
                    client
                        .send_fire_and_forget(req_id, body)
                        .await
                        .map_err(|e| {
                            format!("Send fire and forget error at step {step_idx}: {e}")
                        })?;
                }
            }

            "stream.expect" => {
                let actor_name = step
                    .actor
                    .as_deref()
                    .ok_or_else(|| format!("Step {step_idx}: actor required for stream.expect"))?;
                let conn_name = step.connection.as_deref().unwrap_or("main");
                let client = connections
                    .get(&(actor_name.to_string(), conn_name.to_string()))
                    .ok_or_else(|| {
                        format!("No active stream connection for ({actor_name}, {conn_name}) at step {step_idx}")
                    })?;

                let variant = step
                    .message
                    .as_ref()
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        format!("Step {step_idx}: message variant name required for stream.expect")
                    })?;

                let resolved_match = step.match_.as_ref().map(|m| ctx.resolve_val(m));
                let timeout = Duration::from_millis(step.within_ms.unwrap_or(2000));

                let _msg = client
                    .expect_unsolicited(variant, resolved_match.as_ref(), timeout)
                    .await
                    .map_err(|e| format!("stream.expect failed at step {step_idx}: {e}"))?;

                if let (Some(mark_name), Some(metric_name)) = (&step.measure_from, &step.metric)
                    && let Some(mark_time) = marks.get(mark_name)
                {
                    let elapsed = mark_time.elapsed().as_millis() as u64;
                    metrics.insert(metric_name.clone(), elapsed);
                }
            }

            "stream.expect_none" => {
                let actor_name = step.actor.as_deref().ok_or_else(|| {
                    format!("Step {step_idx}: actor required for stream.expect_none")
                })?;
                let conn_name = step.connection.as_deref().unwrap_or("main");
                let client = connections
                    .get(&(actor_name.to_string(), conn_name.to_string()))
                    .ok_or_else(|| {
                        format!("No active stream connection for ({actor_name}, {conn_name}) at step {step_idx}")
                    })?;

                let variant = step
                    .message
                    .as_ref()
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        format!(
                            "Step {step_idx}: message variant name required for stream.expect_none"
                        )
                    })?;

                let resolved_match = step.match_.as_ref().map(|m| ctx.resolve_val(m));
                let duration = Duration::from_millis(step.for_ms.unwrap_or(1000));

                client
                    .expect_none(variant, resolved_match.as_ref(), duration)
                    .await
                    .map_err(|e| format!("stream.expect_none failed at step {step_idx}: {e}"))?;
            }

            "stream.expect_close" => {
                let actor_name = step.actor.as_deref().ok_or_else(|| {
                    format!("Step {step_idx}: actor required for stream.expect_close")
                })?;
                let conn_name = step.connection.as_deref().unwrap_or("main");
                let client = connections
                    .get(&(actor_name.to_string(), conn_name.to_string()))
                    .ok_or_else(|| {
                        format!("No active stream connection for ({actor_name}, {conn_name}) at step {step_idx}")
                    })?;

                let code = step.code.ok_or_else(|| {
                    format!("Step {step_idx}: code required for stream.expect_close")
                })?;
                let timeout = Duration::from_millis(step.within_ms.unwrap_or(2000));

                client
                    .expect_close(code, timeout)
                    .await
                    .map_err(|e| format!("stream.expect_close failed at step {step_idx}: {e}"))?;
            }

            "stream.close" => {
                let actor_name = step
                    .actor
                    .as_deref()
                    .ok_or_else(|| format!("Step {step_idx}: actor required for stream.close"))?;
                let conn_name = step.connection.as_deref().unwrap_or("main");
                let client = connections
                    .get(&(actor_name.to_string(), conn_name.to_string()))
                    .ok_or_else(|| {
                        format!("No active stream connection for ({actor_name}, {conn_name}) at step {step_idx}")
                    })?;

                client
                    .close()
                    .await
                    .map_err(|e| format!("stream.close failed at step {step_idx}: {e}"))?;
            }

            "token.refresh" => {
                let actor_name = step
                    .actor
                    .as_deref()
                    .ok_or_else(|| format!("Step {step_idx}: actor required for token.refresh"))?;
                let actor_def = scenario
                    .actors
                    .get(actor_name)
                    .ok_or_else(|| format!("Unknown actor {actor_name} at step {step_idx}"))?;
                let tenant_name = actor_def.tenant.as_deref().unwrap_or("default");
                let (_, boot_cred) = tenant_map
                    .get(tenant_name)
                    .ok_or_else(|| format!("Missing boot credential for tenant {tenant_name}"))?;

                let installation_id = actor_installations
                    .get(actor_name)
                    .ok_or_else(|| format!("Missing installation for {actor_name}"))?;

                let external_sub = actor_def
                    .external_subject_id
                    .as_deref()
                    .unwrap_or(actor_name);
                let raw_platform = actor_def.platform.as_deref().unwrap_or("ANDROID");
                let platform = raw_platform
                    .strip_prefix("PLATFORM_")
                    .unwrap_or(raw_platform);

                let new_token = server
                    .mint_token(
                        boot_cred,
                        external_sub,
                        installation_id,
                        platform,
                        actor_def.scopes.clone(),
                        actor_def.token_ttl_seconds,
                    )
                    .await?;

                ctx.actor_tokens.insert(actor_name.to_string(), new_token);
            }

            "assert.ephemeral" => {
                let what = step.what.as_deref().ok_or_else(|| {
                    format!("Step {step_idx}: what required for assert.ephemeral")
                })?;
                if what != "snapshot" {
                    return Err(format!("Unsupported ephemeral check: {what}"));
                }

                let session_raw = step.session.as_deref().ok_or_else(|| {
                    format!("Step {step_idx}: session required for assert.ephemeral")
                })?;
                let session_id = ctx.resolve_str(session_raw);

                let member_raw = step.member.as_deref().ok_or_else(|| {
                    format!("Step {step_idx}: member required for assert.ephemeral")
                })?;
                let member_id = ctx.resolve_str(member_raw);

                let expected_exists = step.exists.unwrap_or(false);
                let key = format!("{}:snap:{session_id}:{member_id}", server.prefix);

                let actual_exists = server
                    .ephemeral
                    .get_snapshot(&key)
                    .await
                    .map_err(|e| format!("Ephemeral get error: {e}"))?
                    .is_some();

                if actual_exists != expected_exists {
                    return Err(format!(
                        "assert.ephemeral failed at step {step_idx} ({key}): expected exists={expected_exists}, got {actual_exists}"
                    ));
                }
            }

            "sleep" => {
                let ms = step.ms.unwrap_or(0);
                tokio::time::sleep(Duration::from_millis(ms)).await;
            }

            other => return Err(format!("Step {step_idx}: unsupported operation {other}")),
        }
    }

    Ok(ScenarioReport {
        id: scenario.id.clone(),
        metrics,
    })
}
