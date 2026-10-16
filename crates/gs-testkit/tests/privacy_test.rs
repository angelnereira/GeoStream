//! Privacy and security integration tests for Fase F4 (T-F4-13):
//! - Log capture verification across scenarios (FR-PRV-004, NFR-SEC-003).
//! - PostgreSQL inspection for zero persisted coordinates when history is disabled (FR-PRV-001, NFR-PRIV-001).
//! - Synchronous immediate deletion of ephemeral snapshots before HTTP response returns (FR-PRV-001, NFR-PRIV-002).
//! - Metric label cardinality inspection (NFR-OBS-002).
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gs_domain::Subprotocol;
use gs_ephemeral::EphemeralStore;
use gs_observability::{JsonLogFormatter, assert_clean_capture, verify_capture};
use gs_protocol::client_message::Body as ClientBody;
use gs_protocol::{
    ClientHello, LocationKind, LocationQuality, LocationSample, LocationSource, MotionActivity,
    Position, PublishLocation,
};
use gs_testkit::client::TestStreamClient;
use gs_testkit::server::TestServer;
use gs_testkit::{parse_scenario, run_scenario};
use serde_json::json;
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct SharedLogBuffer {
    inner: Arc<Mutex<Vec<u8>>>,
}

impl SharedLogBuffer {
    fn contents(&self) -> String {
        let guard = self.inner.lock().expect("lock log buffer");
        String::from_utf8_lossy(&guard).into_owned()
    }
}

struct SharedLogWriter {
    inner: Arc<Mutex<Vec<u8>>>,
}

impl Write for SharedLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut guard = self.inner.lock().expect("lock log buffer");
        guard.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for SharedLogBuffer {
    type Writer = SharedLogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        SharedLogWriter {
            inner: Arc::clone(&self.inner),
        }
    }
}

#[tokio::test]
async fn test_fr_prv_004_and_nfr_sec_003_scenario_logs_contain_no_coordinates_or_secrets() {
    // covers: FR-PRV-004, NFR-SEC-003, docs/10 §8, docs/18 §7
    let buffer = SharedLogBuffer::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .event_format(JsonLogFormatter)
        .with_writer(buffer.clone())
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    for raw in [
        include_str!("../../../contracts/scenarios/scn_001.json"),
        include_str!("../../../contracts/scenarios/scn_002.json"),
        include_str!("../../../contracts/scenarios/scn_003.json"),
        include_str!("../../../contracts/scenarios/scn_006.json"),
    ] {
        let scenario = parse_scenario(raw).expect("parse scenario");
        run_scenario(&scenario, Subprotocol::Proto)
            .await
            .expect("scenario proto");
        run_scenario(&scenario, Subprotocol::Json)
            .await
            .expect("scenario json");
    }

    let captured = buffer.contents();
    let report = assert_clean_capture(&captured).expect("scenario logs must be 100% clean");
    assert!(
        report.findings.is_empty(),
        "expected zero forbidden patterns in scenario logs"
    );
}

#[tokio::test]
async fn test_nfr_priv_001_postgresql_contains_no_persisted_coordinates() {
    // covers: FR-PRV-001, NFR-PRIV-001, docs/10 §6, docs/18 §7
    let raw = include_str!("../../../contracts/scenarios/scn_001.json");
    let scenario = parse_scenario(raw).expect("parse SCN-001");
    run_scenario(&scenario, Subprotocol::Proto)
        .await
        .expect("run SCN-001");

    let db_url = std::env::var("GS_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .unwrap_or_else(|_| {
            "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
        });
    let pool = gs_service::migrate::connect_pool(&db_url)
        .await
        .expect("connect pool");

    // Inspect all durable tables in PostgreSQL to ensure no coordinates or raw secrets are stored.
    for (table, query) in [
        (
            "tenants",
            "SELECT COALESCE(json_agg(t)::text, '[]') FROM (SELECT * FROM tenants ORDER BY 1 DESC LIMIT 200) t",
        ),
        (
            "applications",
            "SELECT COALESCE(json_agg(t)::text, '[]') FROM (SELECT * FROM applications ORDER BY 1 DESC LIMIT 200) t",
        ),
        (
            "api_keys",
            "SELECT COALESCE(json_agg(t)::text, '[]') FROM (SELECT * FROM api_keys ORDER BY 1 DESC LIMIT 200) t",
        ),
        (
            "subjects",
            "SELECT COALESCE(json_agg(t)::text, '[]') FROM (SELECT * FROM subjects ORDER BY 1 DESC LIMIT 200) t",
        ),
        (
            "devices",
            "SELECT COALESCE(json_agg(t)::text, '[]') FROM (SELECT * FROM devices ORDER BY 1 DESC LIMIT 200) t",
        ),
        (
            "sessions",
            "SELECT COALESCE(json_agg(t)::text, '[]') FROM (SELECT * FROM sessions ORDER BY 1 DESC LIMIT 200) t",
        ),
        (
            "memberships",
            "SELECT COALESCE(json_agg(t)::text, '[]') FROM (SELECT * FROM memberships ORDER BY 1 DESC LIMIT 200) t",
        ),
        (
            "invitations",
            "SELECT COALESCE(json_agg(t)::text, '[]') FROM (SELECT * FROM invitations ORDER BY 1 DESC LIMIT 200) t",
        ),
        (
            "audit_events",
            "SELECT COALESCE(json_agg(t)::text, '[]') FROM (SELECT * FROM audit_events ORDER BY 1 DESC LIMIT 200) t",
        ),
        (
            "idempotency_keys",
            "SELECT COALESCE(json_agg(t)::text, '[]') FROM (SELECT * FROM idempotency_keys ORDER BY 1 DESC LIMIT 200) t",
        ),
        (
            "webhook_endpoints",
            "SELECT COALESCE(json_agg(t)::text, '[]') FROM (SELECT * FROM webhook_endpoints ORDER BY 1 DESC LIMIT 200) t",
        ),
        (
            "webhook_deliveries",
            "SELECT COALESCE(json_agg(t)::text, '[]') FROM (SELECT * FROM webhook_deliveries ORDER BY 1 DESC LIMIT 200) t",
        ),
    ] {
        let dump: (String,) = sqlx::query_as(query)
            .fetch_one(&pool)
            .await
            .expect("query table dump");
        let report = verify_capture(&dump.0);
        assert!(
            report.is_clean(),
            "table {table} contained forbidden coordinate or secret patterns: {:?}",
            report.findings
        );
    }
}

#[tokio::test]
async fn test_nfr_priv_002_immediate_snapshot_deletion_on_stop_revoke_leave_and_close() {
    // covers: FR-PRV-001, NFR-PRIV-002, docs/04 §7.3, docs/04 §7.4, docs/18 §7
    let server = TestServer::start(&BTreeMap::new())
        .await
        .expect("start test server");
    let client = reqwest::Client::new();

    let run_id = uuid::Uuid::new_v4().to_string();
    let tenant =
        gs_service::admin::create_tenant(&server.pool, &format!("PrivTenant-{}", &run_id[..8]))
            .await
            .expect("create tenant");
    let app = gs_service::admin::create_application(
        &server.pool,
        &tenant.id.to_string(),
        "priv-app",
        None,
        None,
    )
    .await
    .expect("create app");
    let scopes: Vec<String> = gs_auth::scopes::APPLICATION_SCOPES
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let api_key = gs_service::admin::create_api_key(
        &server.pool,
        &app.id.to_string(),
        Some(scopes),
        Some("priv-key"),
        None,
    )
    .await
    .expect("create api key");
    let app_cred = api_key.credential.to_string();

    // Create a MANY_TO_MANY session with snapshot_ttl_seconds = 300
    let ses_resp: serde_json::Value = client
        .post(format!("{}/v1/sessions", server.http_base))
        .bearer_auth(&app_cred)
        .json(&json!({
            "mode": "MANY_TO_MANY",
            "snapshot_ttl_seconds": 300
        }))
        .send()
        .await
        .expect("create session")
        .json()
        .await
        .expect("session json");
    let session_id = ses_resp["id"].as_str().expect("session id").to_string();

    // Helper that adds a member, enables sharing, publishes a sample over WS, and returns (member_id, device_token)
    for (idx, transition) in ["stop_sharing", "leave", "revoke", "close_session"]
        .iter()
        .enumerate()
    {
        let ext_sub = format!("sub-priv-{idx}");
        let inst_id = format!("00000000-0000-4000-8000-00000000010{idx}");
        let mem_resp: serde_json::Value = client
            .post(format!(
                "{}/v1/sessions/{session_id}/members",
                server.http_base
            ))
            .bearer_auth(&app_cred)
            .json(&json!({
                "external_subject_id": ext_sub,
                "role": "MEMBER",
                "capabilities": {
                    "read": true,
                    "publish": true,
                    "invite": false,
                    "manage": false
                }
            }))
            .send()
            .await
            .expect("add member")
            .json()
            .await
            .expect("member json");
        let member_id = mem_resp["id"].as_str().expect("member id").to_string();

        let dev_token = server
            .mint_token(
                &app_cred,
                &ext_sub,
                &inst_id,
                "ANDROID",
                Some(vec![
                    "stream:connect".to_string(),
                    "sharing:write".to_string(),
                ]),
                Some(900),
            )
            .await
            .expect("mint token");

        // Enable sharing
        let share_res = client
            .put(format!(
                "{}/v1/sessions/{session_id}/members/me/sharing",
                server.http_base
            ))
            .bearer_auth(&dev_token)
            .json(&json!({ "sharing": true }))
            .send()
            .await
            .expect("enable sharing");
        assert!(share_res.status().is_success());

        // Obtain ticket and publish one valid sample so snapshot is written
        let ticket_res: serde_json::Value = client
            .post(format!("{}/v1/stream/tickets", server.http_base))
            .bearer_auth(&dev_token)
            .send()
            .await
            .expect("create ticket")
            .json()
            .await
            .expect("ticket json");
        let ticket = ticket_res["ticket"].as_str().expect("ticket");
        let ws_url = format!("{}/v1/stream?ticket={ticket}", server.ws_base);
        let ws = TestStreamClient::connect(&ws_url, Subprotocol::Proto)
            .await
            .expect("connect ws");
        let h_id = ws.next_id().await;
        ws.send_with_reply(
            h_id,
            ClientBody::Hello(ClientHello {
                protocol_minor: 0,
                sdk_name: "gs-testkit".to_string(),
                sdk_version: "0.1.0".to_string(),
                platform: 1,
                app_version: "1.0.0".to_string(),
                last_sequence: 0,
                client_time: None,
                core_version: String::new(),
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("hello");

        let now = time::OffsetDateTime::now_utc();
        let pub_id = ws.next_id().await;
        ws.send_with_reply(
            pub_id,
            ClientBody::PublishLocation(PublishLocation {
                session_id: session_id.clone(),
                samples: vec![LocationSample {
                    sequence: 1,
                    captured_at: Some(gs_service::pbjson_types::Timestamp {
                        seconds: now.unix_timestamp(),
                        nanos: now.nanosecond() as i32,
                    }),
                    position: Some(Position {
                        latitude: 8.9824,
                        longitude: -79.5199,
                        horizontal_accuracy_m: 5.0,
                        altitude_m: None,
                        vertical_accuracy_m: None,
                        speed_mps: Some(3.0),
                        speed_accuracy_mps: None,
                        heading_deg: Some(90.0),
                        heading_accuracy_deg: None,
                    }),
                    quality: Some(LocationQuality {
                        source: LocationSource::Gnss as i32,
                        kind: LocationKind::Measured as i32,
                        is_mock: Some(false),
                        motion: MotionActivity::Walking as i32,
                    }),
                }],
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("publish sample");

        // Verify snapshot exists in ephemeral store before transition
        let snap_key = format!("{}:snap:{session_id}:{member_id}", server.prefix);
        let before = server
            .ephemeral
            .get_snapshot(&snap_key)
            .await
            .expect("get snapshot before");
        assert!(
            before.is_some(),
            "snapshot must exist before {transition} transition"
        );

        // Execute transition and immediately check snapshot is gone
        match *transition {
            "stop_sharing" => {
                let res = client
                    .put(format!(
                        "{}/v1/sessions/{session_id}/members/me/sharing",
                        server.http_base
                    ))
                    .bearer_auth(&dev_token)
                    .json(&json!({ "sharing": false }))
                    .send()
                    .await
                    .expect("stop sharing");
                assert!(res.status().is_success());
            }
            "leave" => {
                let res = client
                    .post(format!(
                        "{}/v1/sessions/{session_id}/leave",
                        server.http_base
                    ))
                    .bearer_auth(&dev_token)
                    .send()
                    .await
                    .expect("leave session");
                assert!(res.status().is_success());
            }
            "revoke" => {
                let res = client
                    .delete(format!(
                        "{}/v1/sessions/{session_id}/members/{member_id}",
                        server.http_base
                    ))
                    .bearer_auth(&app_cred)
                    .send()
                    .await
                    .expect("revoke member");
                assert!(res.status().is_success());
            }
            "close_session" => {
                let res = client
                    .post(format!(
                        "{}/v1/sessions/{session_id}/close",
                        server.http_base
                    ))
                    .bearer_auth(&app_cred)
                    .send()
                    .await
                    .expect("close session");
                assert!(res.status().is_success());
            }
            _ => unreachable!(),
        }

        let after = server
            .ephemeral
            .get_snapshot(&snap_key)
            .await
            .expect("get snapshot immediately after transition");
        assert!(
            after.is_none(),
            "NFR-PRIV-002 violated: snapshot still present after {transition}"
        );

        let _ = ws.close().await;
    }
}
