//! `POST /v1/ingest/locations` over HTTP: the contract shape, staleness and the delivery use case
//! of `docs/15` §3.2 (FR-LOC-001, docs/09 §3). Every call is validated against the OpenAPI.
#![forbid(unsafe_code)]

mod common;

use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use gs_api::{IngestState, SessionApiState, StreamTicketConfig, ingest_router, sessions_router};
use gs_service::admin::{create_api_key, create_application, create_tenant};
use gs_service::auth::DeviceTokenKeys;
use gs_service::gs_auth::crypto::{CryptoRotator, DataCipher};
use gs_service::gs_auth::{generate_signing_key, parse_private_pem};
use gs_service::migrate::{connect_pool, run_migrations};
use gs_service::tokens::{TokenTtlBounds, mint_device_token};
use reqwest::Method;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

fn test_database_url() -> String {
    std::env::var("GS_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://geostream:geostream_dev_password@127.0.0.1:5432/geostream_test".to_string()
    })
}

struct World {
    base: String,
    http: reqwest::Client,
    api_key: String,
    driver_token: String,
    session_id: String,
    _server: tokio::task::JoinHandle<()>,
}

async fn world() -> World {
    let pool = connect_pool(&test_database_url()).await.expect("connect");
    run_migrations(&pool).await.expect("migrate");
    let tenant = create_tenant(&pool, "Ingest Tenant").await.expect("tenant");
    let app = create_application(&pool, &tenant.id.to_string(), "ingest-app", None, None)
        .await
        .expect("app");
    let key = create_api_key(
        &pool,
        &app.id.to_string(),
        Some(vec![
            "tokens:mint".into(),
            "sessions:write".into(),
            "sessions:read".into(),
            "locations:read".into(),
        ]),
        None,
        None,
    )
    .await
    .expect("key");

    let generated = generate_signing_key("ingest-test").expect("key pair");
    let signing = parse_private_pem(generated.private_pem()).expect("pem");
    let mut keyset = DeviceTokenKeys::default();
    keyset.insert_signing_key("ingest-test", signing.clone());

    let request = serde_json::from_value(json!({
        "external_subject_id": "driver-1",
        "installation_id": "0192b6c0-a1e4-4c3f-9d2a-7b5e8f1c0d3a",
        "platform": "ANDROID",
        "scopes": ["sharing:write", "ingest:write"]
    }))
    .expect("token request");
    let minted = mint_device_token(
        &pool,
        key.credential.as_str(),
        &request,
        TokenTtlBounds {
            min_seconds: 300,
            default_seconds: 900,
            max_seconds: 3600,
        },
        "https://geostream.example.com",
        &signing,
        "ingest-test",
    )
    .await
    .expect("mint");

    let rotator = CryptoRotator::empty()
        .with_active(DataCipher::new("test_key", &[11u8; 32]).expect("cipher"));
    let sessions = SessionApiState {
        pool,
        signing_keys: Arc::new(keyset),
        issuer: "https://geostream.example.com".to_string(),
        leeway_seconds: 5,
        crypto_rotator: Some(Arc::new(rotator)),
        idempotency_retention: Duration::from_secs(3600),
        ephemeral: gs_service::EphemeralBackend::memory(),
        bus: gs_service::BusBackend::memory(),
        redis_key_prefix: "gs".to_string(),
        stream_ticket: StreamTicketConfig::default(),
        rate_limit_config: gs_api::RateLimitConfig::default(),
    };
    let router = sessions_router(sessions.clone())
        .merge(ingest_router(IngestState::with_defaults(sessions)));
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("bind");
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve");
    });
    let base = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::new();

    // The backend creates the delivery session with the driver as publisher (docs/15 §3.2).
    let created = http
        .post(format!("{base}/v1/sessions"))
        .bearer_auth(key.credential.as_str())
        .json(&json!({
            "mode": "ONE_TO_MANY",
            "expires_in_seconds": 600,
            "initial_members": [{
                "external_subject_id": "driver-1",
                "capabilities": {"publish": true, "read": false, "invite": false, "manage": false}
            }]
        }))
        .send()
        .await
        .expect("create session");
    assert_eq!(created.status(), 201);
    let session_id = created.json::<Value>().await.expect("json")["id"]
        .as_str()
        .expect("id")
        .to_string();
    let sharing = http
        .put(format!(
            "{base}/v1/sessions/{session_id}/members/me/sharing"
        ))
        .bearer_auth(&minted.response.access_token)
        .json(&json!({"sharing": true}))
        .send()
        .await
        .expect("sharing");
    assert_eq!(sharing.status(), 200);

    World {
        base,
        http,
        api_key: key.credential.to_string(),
        driver_token: minted.response.access_token,
        session_id,
        _server: server,
    }
}

impl World {
    async fn ingest(&self, token: &str, body: &Value) -> (u16, Value) {
        let spec = common::spec::load_spec();
        spec.check_request("ingestLocations", body);
        self.ingest_unchecked(token, body).await
    }

    /// Sends a body that may break the contract on purpose; the response is still validated.
    async fn ingest_unchecked(&self, token: &str, body: &Value) -> (u16, Value) {
        let response = self
            .http
            .request(Method::POST, format!("{}/v1/ingest/locations", self.base))
            .bearer_auth(token)
            .json(body)
            .send()
            .await
            .expect("ingest");
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let value: Value = response.json().await.expect("json");
        common::spec::load_spec().check_response("ingestLocations", status, &content_type, &value);
        (status, value)
    }

    fn sample(&self, sequence: u64, at: OffsetDateTime, lat: f64, lon: f64) -> Value {
        json!({
            "sequence": sequence,
            "captured_at": at.format(&Rfc3339).expect("rfc3339"),
            "position": {"latitude": lat, "longitude": lon, "horizontal_accuracy_m": 8},
            "quality": {"kind": "MEASURED", "source": "GNSS", "is_mock": false}
        })
    }

    fn batch(&self, samples: Vec<Value>) -> Value {
        json!({"session_id": self.session_id, "samples": samples})
    }
}

#[tokio::test]
async fn test_the_contract_shape_is_accepted_and_reaches_the_snapshot() {
    // covers: FR-LOC-001, docs/15 §3.2 (delivery with live tracking)
    let w = world().await;
    let now = OffsetDateTime::now_utc();
    let (status, body) = w
        .ingest(
            &w.driver_token,
            &w.batch(vec![w.sample(1, now, 8.9824, -79.5199)]),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["results"][0]["status"], json!("ACCEPTED"));

    let snapshot = w
        .http
        .get(format!("{}/v1/sessions/{}/snapshot", w.base, w.session_id))
        .bearer_auth(&w.api_key)
        .send()
        .await
        .expect("snapshot");
    assert_eq!(snapshot.status(), 200);
    let snapshot: Value = snapshot.json().await.expect("json");
    assert_eq!(snapshot["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        snapshot["items"][0]["sample"]["position"]["latitude"],
        json!(8.9824)
    );
}

#[tokio::test]
async fn test_the_full_protobuf_enum_names_keep_working() {
    // covers: backwards compatibility of the JSON mapping used until now
    let w = world().await;
    let now = OffsetDateTime::now_utc();
    let mut sample = w.sample(1, now, 8.9824, -79.5199);
    sample["quality"] = json!({"kind": "LOCATION_KIND_MEASURED", "source": "LOCATION_SOURCE_GNSS"});
    let (status, body) = w
        .ingest_unchecked(&w.driver_token, &w.batch(vec![sample]))
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["results"][0]["status"], json!("ACCEPTED"));
}

#[tokio::test]
async fn test_old_samples_are_stale_but_recent_ones_are_not() {
    // covers: docs/09 §3.2 (GS_LOCATION_MAX_EVENT_AGE_MS, 120 s by default)
    let w = world().await;
    let now = OffsetDateTime::now_utc();
    let (_, recent) = w
        .ingest(
            &w.driver_token,
            &w.batch(vec![w.sample(
                1,
                now - time::Duration::seconds(30),
                8.98,
                -79.52,
            )]),
        )
        .await;
    assert_eq!(recent["results"][0]["status"], json!("ACCEPTED"));
    let (_, old) = w
        .ingest(
            &w.driver_token,
            &w.batch(vec![w.sample(
                2,
                now - time::Duration::minutes(10),
                8.98,
                -79.52,
            )]),
        )
        .await;
    assert_eq!(old["results"][0]["status"], json!("IGNORED_STALE"));
}

#[tokio::test]
async fn test_null_island_and_duplicates_are_reported_per_sample() {
    // covers: docs/09 §3 rule 6, FR-LOC-001 (idempotent duplicates)
    let w = world().await;
    let now = OffsetDateTime::now_utc();
    let (_, first) = w
        .ingest(
            &w.driver_token,
            &w.batch(vec![
                w.sample(1, now, 8.9824, -79.5199),
                w.sample(2, now, 0.0, 0.0),
            ]),
        )
        .await;
    let statuses: Vec<&str> = first["results"]
        .as_array()
        .unwrap_or_else(|| panic!("{first}"))
        .iter()
        .map(|r| r["status"].as_str().expect("status"))
        .collect();
    assert_eq!(statuses, ["ACCEPTED", "REJECTED"]);

    // Sending the same sequence again is idempotent, never an error.
    let (status, again) = w
        .ingest(
            &w.driver_token,
            &w.batch(vec![w.sample(1, now, 8.9824, -79.5199)]),
        )
        .await;
    assert_eq!(status, 200, "{again}");
    assert_eq!(again["results"][0]["status"], json!("IGNORED_DUPLICATE"));
}

#[tokio::test]
async fn test_an_api_key_cannot_ingest_and_oversized_batches_are_refused() {
    // covers: docs/06 §8.2 (ingest is device-only), docs/07 (50 samples per batch)
    let w = world().await;
    let now = OffsetDateTime::now_utc();
    let (status, _) = w
        .ingest(&w.api_key, &w.batch(vec![w.sample(1, now, 8.98, -79.52)]))
        .await;
    assert!(status == 401 || status == 403, "got {status}");

    let many: Vec<Value> = (1..=51).map(|n| w.sample(n, now, 8.98, -79.52)).collect();
    let (status, body) = w.ingest_unchecked(&w.driver_token, &w.batch(many)).await;
    assert!(status == 413 || status == 422, "got {status}: {body}");
}
