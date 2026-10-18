//! End-to-end `nearby` over a live index populated from a published snapshot.
#![forbid(unsafe_code)]

use gs_service::EphemeralStore as _;
use gs_testkit::TestServer;
use prost::Message as _;
use std::collections::BTreeMap;

#[tokio::test]
async fn nearby_returns_a_publisher_with_a_live_snapshot() {
    // covers: FR-POOL-004, docs/23 §3.1, docs/04 §7.3
    let server = TestServer::start(&BTreeMap::new()).await.expect("server");
    let client = reqwest::Client::new();

    // 1. Tenant + application + API key with the pool scope.
    let tenant = gs_service::admin::create_tenant(&server.pool, "LivePoolTenant")
        .await
        .expect("tenant");
    let app = gs_service::admin::create_application(
        &server.pool,
        &tenant.id.to_string(),
        "live-pool-app",
        None,
        None,
    )
    .await
    .expect("app");
    let scopes: Vec<String> = [
        "sessions:write",
        "sessions:read",
        "pools:query",
        "pools:manage",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();
    let key = gs_service::admin::create_api_key(
        &server.pool,
        &app.id.to_string(),
        Some(scopes),
        Some("live-pool-key"),
        None,
    )
    .await
    .expect("api key");
    let cred = key.credential.to_string();

    // 2. Policy with pools allowed.
    sqlx::query("UPDATE tenants SET policy = '{\"pools_allowed\": true}'::jsonb WHERE id = $1;")
        .bind(tenant.id.into_uuid())
        .execute(&server.pool)
        .await
        .expect("policy");

    // 3. A session + a sharing member, seeded directly.
    let session_uuid = uuid::Uuid::now_v7();
    let member_uuid = uuid::Uuid::now_v7();
    let subject_uuid = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO subjects (id, tenant_id, external_subject_id, status, token_epoch, created_at, updated_at)          VALUES ($1, $2, $3, 'ACTIVE', 0, now(), now());",
    )
    .bind(subject_uuid)
    .bind(tenant.id.into_uuid())
    .bind(format!("ext-{subject_uuid}"))
    .execute(&server.pool)
    .await
    .expect("subject");
    let device_uuid = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO devices (id, tenant_id, subject_id, application_id, installation_id, platform, status, created_at, last_seen_at)          VALUES ($1, $2, $3, $4, $5, 'IOS', 'ACTIVE', now(), now());",
    )
    .bind(device_uuid)
    .bind(tenant.id.into_uuid())
    .bind(subject_uuid)
    .bind(app.id.into_uuid())
    .bind(uuid::Uuid::now_v7())
    .execute(&server.pool)
    .await
    .expect("device");
    let session_id_str = format!("ses_{}", session_uuid.as_simple());
    sqlx::query(
        "INSERT INTO sessions (id, tenant_id, mode, status, created_by_kind, created_by_id, expires_at, max_members, snapshot_ttl_seconds, stale_after_seconds) \
         VALUES ($1, $2, 'MANY_TO_MANY', 'ACTIVE', 'APPLICATION', $3, now() + interval '1 day', 10, 300, 60);",
    )
    .bind(session_uuid)
    .bind(tenant.id.into_uuid())
    .bind(app.id.into_uuid())
    .execute(&server.pool)
    .await
    .expect("session");
    sqlx::query(
        "INSERT INTO memberships (id, tenant_id, session_id, subject_id, role, can_publish, can_read, can_invite, can_manage, status, sharing, publisher_device_id, joined_via, joined_at, version) \
         VALUES ($1, $2, $3, $4, 'MEMBER', true, true, false, false, 'ACTIVE', true, $5, 'DIRECT_ADD', now(), 1);",
    )
    .bind(member_uuid)
    .bind(tenant.id.into_uuid())
    .bind(session_uuid)
    .bind(subject_uuid)
    .bind(device_uuid) // publisher_device_id
    .execute(&server.pool)
    .await
    .expect("membership");

    // 4. A live snapshot for that member (what the ingest would have written).
    let update = gs_protocol::LocationUpdate {
        session_id: session_id_str.clone(),
        member_id: member_uuid.to_string(),
        sample: Some(gs_protocol::LocationSample {
            sequence: 1,
            captured_at: Some(gs_protocol::pbjson_types::Timestamp {
                seconds: time::OffsetDateTime::now_utc().unix_timestamp(),
                nanos: 0,
            }),
            position: Some(gs_protocol::Position {
                latitude: 8.9824,
                longitude: -79.5199,
                horizontal_accuracy_m: 8.0,
                altitude_m: None,
                vertical_accuracy_m: None,
                speed_mps: None,
                speed_accuracy_mps: None,
                heading_deg: None,
                heading_accuracy_deg: None,
            }),
            quality: Some(gs_protocol::LocationQuality {
                source: gs_protocol::LocationSource::Gnss as i32,
                kind: gs_protocol::LocationKind::Measured as i32,
                is_mock: Some(false),
                motion: gs_protocol::MotionActivity::Unspecified as i32,
            }),
        }),
        received_at: Some(gs_protocol::pbjson_types::Timestamp {
            seconds: time::OffsetDateTime::now_utc().unix_timestamp(),
            nanos: 0,
        }),
        delivery: 0,
        estimate: None,
    };
    let member_id_prefixed = format!("mem_{}", member_uuid.as_simple());
    let key = format!(
        "{}:snap:{}:{}",
        server.prefix, session_id_str, member_id_prefixed
    );
    server
        .ephemeral
        .set_snapshot(
            &key,
            &update.encode_to_vec(),
            std::time::Duration::from_secs(300),
        )
        .await
        .expect("snapshot");

    // 5. Query: the member must appear.
    let resp = client
        .get(format!(
            "{}/v1/pools/{session_id_str}/nearby?lat=8.9824&lon=-79.5199&radius_m=5000",
            server.http_base
        ))
        .bearer_auth(&cred)
        .send()
        .await
        .expect("nearby");
    assert_eq!(resp.status(), 200, "body: {:?}", resp.text().await);
    let body: serde_json::Value = resp.json().await.expect("json");
    let data = body["data"].as_array().expect("data array");

    assert_eq!(data.len(), 1, "the publishing member must appear in nearby");
    assert_eq!(data[0]["member_id"], member_id_prefixed);
    assert_eq!(data[0]["availability"], "AVAILABLE");
}
